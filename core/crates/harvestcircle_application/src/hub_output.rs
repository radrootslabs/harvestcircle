//! Pure local documents. Rendering, saving and printing never submit a physical command.
use harvestcircle_domain::hub::dispatch::{DispatchRecord, IdentificationVersion};
use harvestcircle_domain::hub::reference::{HubRecordId, ReferenceRecord, SourceAttribution};
use harvestcircle_domain::hub::stock::{
    Occurrence, OpeningHistory, PopulationOrigin, StockPopulation,
};
use harvestcircle_domain::hub::trace::{TraceSnapshot, source_certainty_label};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const LOCAL_OUTPUT_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const TRACE_OUTPUT_SCOPE: &str = "One recorded population at the stated stock and catalog revisions. Includes all retained current positions and actual departure lines in this local snapshot, not a complete action-history export. Unknown or possible origin, unrecorded activity and activity outside this desktop cannot be ruled out. Saving or printing does not establish external delivery or acknowledgement.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalOutputError {
    InvalidSnapshot,
    InvalidVersion,
    InvalidTime,
    Capacity,
}

/// Immutable rendering for one local output attempt. Retry this value, not a new read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalOutput {
    pub identity: String,
    pub suggested_filename: String,
    pub html: String,
}
impl LocalOutput {
    fn finish(html: Html, filename: String) -> Result<Self, LocalOutputError> {
        let html = html.finish()?;
        let digest = Sha256::digest(html.as_bytes());
        Ok(Self {
            identity: format!("harvestcircle.local-html.v1:{digest:x}"),
            suggested_filename: filename,
            html,
        })
    }
}

pub fn identification_output(
    record: &DispatchRecord,
    identification: HubRecordId,
) -> Result<LocalOutput, LocalOutputError> {
    let version = record
        .versions
        .iter()
        .find(|v| v.id == identification)
        .ok_or(LocalOutputError::InvalidVersion)?;
    version
        .material
        .validate()
        .map_err(|_| LocalOutputError::InvalidSnapshot)?;
    if version.number == 0
        || version.number > harvestcircle_domain::hub::dispatch::HUB_IDENTIFICATION_VERSIONS as u32
    {
        return Err(LocalOutputError::InvalidVersion);
    }
    let mut out = Html::new("As-issued sample identification")?;
    out.paragraph("Sample data only. This identification is not proof of actual departure, acceptance, external delivery or acknowledgement.")?;
    out.field(
        "Hub",
        &record
            .hub
            .as_bytes()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>(),
    )?;
    out.field("Preparation", &record.id.to_hex())?;
    issued_version(&mut out, version)?;
    LocalOutput::finish(
        out,
        format!(
            "harvestcircle-identification-{}-v{}.html",
            version.id.to_hex(),
            version.number
        ),
    )
}
fn issued_version(out: &mut Html, version: &IdentificationVersion) -> Result<(), LocalOutputError> {
    out.field("Identification", &version.id.to_hex())?;
    out.field("Version", &version.number.to_string())?;
    if let Some(previous) = version.previous {
        out.field("Replaces identification", &previous.to_hex())?;
    }
    // Mutable closure/status and current stock/reference labels never enter as-issued output.
    out.field("Issued at (UTC)", &utc(version.issued.recorded_at)?)?;
    out.field("Issued by", &version.issued.actor.to_hex())?;
    out.field(
        "Original issue operation",
        &operation_text(version.issued.operation),
    )?;
    out.field("Issue observation", version.issued.observation.as_str())?;
    reference(out, "Recipient as issued", &version.material.recipient)?;
    out.field(
        "Captured stock revision",
        &version.material.stock_revision.value().to_string(),
    )?;
    out.field(
        "Captured catalog revision",
        &version.material.catalog_revision.value().to_string(),
    )?;
    out.field("Sample procedure", version.material.basis.label())?;
    out.field(
        "Material observation",
        version.material.observation.as_str(),
    )?;
    for line in &version.material.lines {
        out.heading("Original source line")?;
        reference(out, "Product as issued", &line.product)?;
        reference(out, "Pack as issued", &line.variant)?;
        out.field("Original whole packs", &line.packs.value().to_string())?;
        out.field("Group", &line.group.to_hex())?;
        out.field("Group revision", &line.group_revision.value().to_string())?;
        reference(out, "Location as issued", &line.location)?;
        if let Some(tag) = &line.identification {
            out.field("Physical label", tag.label.as_str())?;
            out.field(
                "Physical identification observation",
                tag.observation.as_str(),
            )?;
        }
        trace_source(out, &line.trace)?;
    }
    out.paragraph("This preserves the selected as-issued version. Current unused, superseded or departure state and later corrections remain separate local records.")
}

pub fn trace_output(snapshot: &TraceSnapshot) -> Result<LocalOutput, LocalOutputError> {
    let balance = snapshot
        .balance()
        .map_err(|_| LocalOutputError::InvalidSnapshot)?;
    let mut out = Html::new("Sample population trace snapshot")?;
    out.paragraph("Sample data only")?;
    out.paragraph(TRACE_OUTPUT_SCOPE)?;
    out.field(
        "Hub",
        &snapshot
            .hub
            .as_bytes()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>(),
    )?;
    out.field(
        "Stock revision",
        &snapshot.stock_revision.value().to_string(),
    )?;
    out.field(
        "Catalog revision",
        &snapshot.catalog_revision.value().to_string(),
    )?;
    trace_source(&mut out, &snapshot.trace)?;
    out.heading("Physical accounting in this pack unit")?;
    reference(&mut out, "Pack", &snapshot.trace.variant)?;
    out.field("Original intake", &balance.original.value().to_string())?;
    out.field("On hand", &balance.on_hand.value().to_string())?;
    out.field("Actual departed", &balance.departed.value().to_string())?;
    out.paragraph("Original intake equals current on-hand packs plus retained actual departure. This is counted stock, not inferred measured mass.")?;
    out.heading("All current identified groups in this population")?;
    if snapshot.on_hand.is_empty() {
        out.paragraph("No packs remain on hand in this population.")?;
    }
    for position in &snapshot.on_hand {
        out.field("Group", &position.group.to_hex())?;
        out.field("Whole packs", &position.packs.value().to_string())?;
        reference(&mut out, "Location as last recorded", &position.location)?;
        if let Some(tag) = &position.identification {
            out.field("Physical label", tag.label.as_str())?;
        }
        out.field(
            "Current sample acceptance",
            match position.treatment.acceptance {
                harvestcircle_domain::hub::treatment::AcceptanceStatus::ReviewRequired => {
                    "Review required"
                }
                harvestcircle_domain::hub::treatment::AcceptanceStatus::Accepted => "Accepted",
                harvestcircle_domain::hub::treatment::AcceptanceStatus::Rejected => "Rejected",
            },
        )?;
        out.field(
            "Current active holds",
            &position.treatment.active_holds.len().to_string(),
        )?;
        for hold in &position.treatment.active_holds {
            out.field("Active hold reference", &hold.to_hex())?;
        }
    }
    out.heading("All retained actual departure lines in this population")?;
    if snapshot.exits.is_empty() {
        out.paragraph("No recorded actual departure in this population.")?;
    }
    for exit in &snapshot.exits {
        let event = &exit.departure;
        out.field("Departure", &event.id.to_hex())?;
        out.field("Original operation", &operation_text(event.operation))?;
        out.field("Group", &exit.group.to_hex())?;
        out.field("Whole packs", &exit.packs.value().to_string())?;
        reference(&mut out, "Recipient as issued", &event.recipient)?;
        out.field("Identification", &event.identification.to_hex())?;
        out.field("Preparation", &event.preparation.to_hex())?;
        actor_time(&mut out, event.actor, event.recorded_at)?;
        occurrence(&mut out, event.occurred)?;
        out.field("Departure observation", event.observation.as_str())?;
    }
    LocalOutput::finish(
        out,
        format!(
            "harvestcircle-trace-{}-r{}.html",
            snapshot.trace.id.to_hex(),
            snapshot.stock_revision.value()
        ),
    )
}

fn trace_source(out: &mut Html, trace: &StockPopulation) -> Result<(), LocalOutputError> {
    out.field("Original population", &trace.id.to_hex())?;
    match &trace.origin {
        PopulationOrigin::Receipt => (),
        PopulationOrigin::CountedOpening { history } => {
            out.field("Population entry", "Counted opening")?;
            reference(out, "Counted location", &trace.initial_location)?;
            out.paragraph("This is stock counted at the stated location, not a historical receipt or evidence of earlier arrival or completed handling.")?;
            match history {
                OpeningHistory::Unknown => out.field("Earlier intake history", "Unknown")?,
                OpeningHistory::ExternalReference(value) => {
                    out.field("Earlier-history reference as reported", value.as_str())?
                }
            }
        }
        PopulationOrigin::CountedSurplus {
            count,
            related_population,
        } => {
            out.field(
                "Population entry",
                "Separately counted surplus of unknown origin",
            )?;
            out.field("Count reference", &count.to_hex())?;
            out.field("Related counted population", &related_population.to_hex())?;
            out.paragraph("The related population does not establish the surplus source. Earlier receipt or acceptance is not inherited.")?;
        }
    }
    reference(out, "Original product", &trace.product)?;
    reference(out, "Original pack", &trace.variant)?;
    out.field(
        "Original counted packs",
        &trace.original_packs.value().to_string(),
    )?;
    optional_reference(out, "Supplier as recorded", trace.supplier.as_ref())?;
    optional_reference(out, "Producer as recorded", trace.producer.as_ref())?;
    out.field("Source certainty", source_certainty_label(&trace.source))?;
    match &trace.source {
        SourceAttribution::Unknown => out.paragraph("Origin is not established. Unknown origin is not evidence of no relationship to another source.")?,
        SourceAttribution::Referenced{source,..}=>{
            out.field("Distinct source reference",&source.id.to_hex())?;
            out.field("Source code as recorded",source.code.as_str())?;
            optional_reference(out,"Source issuer as recorded",trace.source_issuer.as_ref())?;
            out.field("Source type",match source.kind {
                harvestcircle_domain::hub::reference::SourceReferenceKind::ProducerBatch=>"Producer batch",
                harvestcircle_domain::hub::reference::SourceReferenceKind::SupplierReference=>"Supplier reference",
                harvestcircle_domain::hub::reference::SourceReferenceKind::Other=>"Other",
            })?;
            out.field("Source provenance",source.provenance.as_str())?;
            out.paragraph("Matching source code text never merges populations or establishes issuer identity.")?;
        }
    }
    actor_time(out, trace.actor, trace.recorded_at)?;
    occurrence(out, trace.occurred)?;
    out.field("Original intake observation", trace.observation.as_str())
}
fn actor_time(
    out: &mut Html,
    actor: PublicKey,
    recorded_at: UnixTimestamp,
) -> Result<(), LocalOutputError> {
    out.field("Recorded at (UTC)", &utc(recorded_at)?)?;
    out.field("Recorded by", &actor.to_hex())
}
fn occurrence(out: &mut Html, value: Occurrence) -> Result<(), LocalOutputError> {
    match value {
        Occurrence::Unknown => out.field(
            "Actual occurrence",
            "Unknown; recording time does not establish actual time",
        ),
        Occurrence::Known(at) => out.field("Actual occurrence (UTC)", &utc(at)?),
        Occurrence::Interval { earliest, latest } => out.field(
            "Actual occurrence interval (UTC)",
            &format!("{} through {}", utc(earliest)?, utc(latest)?),
        ),
    }
}
fn reference(out: &mut Html, label: &str, value: &ReferenceRecord) -> Result<(), LocalOutputError> {
    out.field(label, value.value.label().as_str())?;
    out.field(
        "Reference identity/version",
        &format!("{} v{}", value.id.to_hex(), value.revision.value()),
    )
}
fn optional_reference(
    out: &mut Html,
    label: &str,
    value: Option<&ReferenceRecord>,
) -> Result<(), LocalOutputError> {
    match value {
        Some(value) => reference(out, label, value),
        None => out.field(label, "Unknown"),
    }
}
fn utc(value: UnixTimestamp) -> Result<String, LocalOutputError> {
    OffsetDateTime::from_unix_timestamp(value.as_seconds())
        .map_err(|_| LocalOutputError::InvalidTime)?
        .format(&Rfc3339)
        .map_err(|_| LocalOutputError::InvalidTime)
}
fn operation_text(value: HubRecordId) -> String {
    let hex = value.to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

struct Html(String);
impl Html {
    fn new(title: &str) -> Result<Self, LocalOutputError> {
        let mut out = Self(String::new());
        out.markup("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>")?;
        out.text(title)?;
        out.markup("</title><style>body{font-family:sans-serif;max-width:60rem;margin:2rem;color:#111;background:#fff}p{overflow-wrap:anywhere}h2{break-after:avoid}strong{font-weight:600}</style></head><body><h1>")?;
        out.text(title)?;
        out.markup("</h1>")?;
        Ok(out)
    }
    fn markup(&mut self, value: &str) -> Result<(), LocalOutputError> {
        if self
            .0
            .len()
            .checked_add(value.len())
            .ok_or(LocalOutputError::Capacity)?
            > LOCAL_OUTPUT_MAX_BYTES
        {
            return Err(LocalOutputError::Capacity);
        }
        self.0.push_str(value);
        Ok(())
    }
    fn text(&mut self, value: &str) -> Result<(), LocalOutputError> {
        for c in value.chars() {
            match c {
                '&' => self.markup("&amp;")?,
                '<' => self.markup("&lt;")?,
                '>' => self.markup("&gt;")?,
                '"' => self.markup("&quot;")?,
                '\'' => self.markup("&#39;")?,
                c => self.markup(c.encode_utf8(&mut [0; 4]))?,
            }
        }
        Ok(())
    }
    fn field(&mut self, label: &str, value: &str) -> Result<(), LocalOutputError> {
        self.markup("<p><strong>")?;
        self.text(label)?;
        self.markup(":</strong> ")?;
        self.text(value)?;
        self.markup("</p>")
    }
    fn paragraph(&mut self, value: &str) -> Result<(), LocalOutputError> {
        self.markup("<p>")?;
        self.text(value)?;
        self.markup("</p>")
    }
    fn heading(&mut self, value: &str) -> Result<(), LocalOutputError> {
        self.markup("<h2>")?;
        self.text(value)?;
        self.markup("</h2>")
    }
    fn finish(mut self) -> Result<String, LocalOutputError> {
        self.markup("</body></html>")?;
        Ok(self.0)
    }
}

#[cfg(test)]
mod tests;
