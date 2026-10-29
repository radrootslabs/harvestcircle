//! Bounded recorded-population retrieval and physical accounting, not recall certification.
use super::movement::PhysicalIdentification;
use super::portions::HUB_LINEAGE_GROUPS;
use super::reference::{
    HubRecordId, ReferenceKind, ReferenceRecord, SourceAttribution, SourceCertainty,
    SourceProvenance, WholePacks,
};
use super::stock::{Occurrence, StockPopulation};
use super::treatment::GroupTreatment;
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use std::collections::{BTreeMap, BTreeSet};

pub const TRACE_PAGE_ROWS: u32 = 64;
pub const TRACE_SEARCH_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceError {
    InvalidQuery,
    InvalidSnapshot,
    Accounting,
    Capacity,
}

/// A source impact retrieval deliberately includes every unresolved population.
/// Neither matching code text nor a different tentative source rules one out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraceScope {
    #[default]
    All,
    Unresolved,
    Receipt(HubRecordId),
    SourceImpact(HubRecordId),
}
/// Validated original source facts for a bounded index scan; not a source grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSourceEvidence {
    Unknown,
    Referenced {
        source: HubRecordId,
        certainty: SourceCertainty,
        issuer: Option<HubRecordId>,
    },
}
impl From<&SourceAttribution> for TraceSourceEvidence {
    fn from(value: &SourceAttribution) -> Self {
        match value {
            SourceAttribution::Unknown => Self::Unknown,
            SourceAttribution::Referenced { source, certainty } => Self::Referenced {
                source: source.id,
                certainty: *certainty,
                issuer: source.issuer,
            },
        }
    }
}
impl TraceSourceEvidence {
    #[must_use]
    pub const fn unresolved(self) -> bool {
        !matches!(
            self,
            Self::Referenced {
                certainty: SourceCertainty::Established,
                issuer: Some(_),
                ..
            }
        )
    }
}
impl TraceScope {
    #[must_use]
    pub fn includes(self, trace: &StockPopulation) -> bool {
        self.includes_evidence(trace.id, (&trace.source).into())
    }
    #[must_use]
    pub fn includes_evidence(self, trace: HubRecordId, source: TraceSourceEvidence) -> bool {
        match self {
            Self::All => true,
            Self::Unresolved => source.unresolved(),
            Self::Receipt(id) => trace == id,
            Self::SourceImpact(id) => {
                source.unresolved()
                    || matches!(source, TraceSourceEvidence::Referenced { source, .. } if source == id)
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceSelection {
    pub scope: TraceScope,
    pub text: String,
}
impl TraceSelection {
    pub fn validate(&self) -> Result<(), TraceError> {
        if self.text.len() > TRACE_SEARCH_BYTES
            || self.text.chars().any(char::is_control)
            || (!self.text.is_empty()
                && matches!(
                    self.scope,
                    TraceScope::SourceImpact(_) | TraceScope::Receipt(_)
                ))
        {
            return Err(TraceError::InvalidQuery);
        }
        Ok(())
    }
    /// Literal search is only a field filter, never proof that origin is excluded.
    pub fn includes(&self, trace: &StockPopulation) -> Result<bool, TraceError> {
        let source = match &trace.source {
            SourceAttribution::Unknown => None,
            SourceAttribution::Referenced { source, .. } => Some(source),
        };
        let trace_id = trace.id.to_hex();
        let source_id = source.map(|s| s.id.to_hex());
        let fields = [
            Some(trace_id.as_str()),
            Some(trace.product.value.label().as_str()),
            Some(trace.variant.value.label().as_str()),
            trace.supplier.as_ref().map(|r| r.value.label().as_str()),
            trace.producer.as_ref().map(|r| r.value.label().as_str()),
            trace
                .source_issuer
                .as_ref()
                .map(|r| r.value.label().as_str()),
            source.map(|s| s.code.as_str()),
            source_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        self.includes_fields(trace.id, (&trace.source).into(), &fields)
    }
    /// Original, bounded text fields from the native index, before deep selected-row reads.
    pub fn includes_fields(
        &self,
        trace: HubRecordId,
        source: TraceSourceEvidence,
        fields: &[&str],
    ) -> Result<bool, TraceError> {
        self.validate()?;
        if fields.len() > 8 || fields.iter().any(|field| field.len() > 128) {
            return Err(TraceError::InvalidQuery);
        }
        if !self.scope.includes_evidence(trace, source) {
            return Ok(false);
        }
        let needle = self.text.trim().to_lowercase();
        Ok(needle.is_empty()
            || fields
                .iter()
                .any(|field| field.to_lowercase().contains(&needle)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TracePosition {
    pub group: HubRecordId,
    pub revision: HubRevision,
    pub packs: WholePacks,
    /// Location label as recorded by the last physical event.
    pub location: ReferenceRecord,
    pub identification: Option<PhysicalIdentification>,
    pub treatment: GroupTreatment,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceDeparture {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub preparation: HubRecordId,
    pub identification: HubRecordId,
    pub recipient: ReferenceRecord,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceExit {
    pub group: HubRecordId,
    pub packs: WholePacks,
    pub departure: TraceDeparture,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceBalance {
    pub variant: HubRecordId,
    pub original: WholePacks,
    pub on_hand: WholePacks,
    pub departed: WholePacks,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceSnapshot {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub trace: StockPopulation,
    pub on_hand: Vec<TracePosition>,
    pub exits: Vec<TraceExit>,
}
impl TraceSnapshot {
    /// Reconciles one original population; never sums different pack variants.
    pub fn balance(&self) -> Result<TraceBalance, TraceError> {
        if self
            .on_hand
            .len()
            .checked_add(self.exits.len())
            .ok_or(TraceError::Capacity)?
            > HUB_LINEAGE_GROUPS
        {
            return Err(TraceError::Capacity);
        }
        if self.trace.original_packs.value() == 0 {
            return Err(TraceError::InvalidSnapshot);
        }
        let mut groups = BTreeSet::new();
        let mut departures = BTreeMap::new();
        let mut operations = BTreeMap::new();
        let mut on_hand = WholePacks::from_value(0).map_err(|_| TraceError::Accounting)?;
        let mut departed = on_hand;
        for position in &self.on_hand {
            if position.packs.value() == 0
                || position.location.value.kind() != ReferenceKind::Location
                || !groups.insert(position.group)
            {
                return Err(TraceError::InvalidSnapshot);
            }
            if position.treatment.group != position.group
                || position.treatment.packs != position.packs
                || position.treatment.active_holds.len()
                    > super::treatment::HUB_TREATMENT_RECORDS_PER_TRACE
                || position
                    .treatment
                    .active_holds
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != position.treatment.active_holds.len()
            {
                return Err(TraceError::InvalidSnapshot);
            }
            on_hand = on_hand
                .checked_add(position.packs)
                .map_err(|_| TraceError::Accounting)?;
        }
        for exit in &self.exits {
            let event = &exit.departure;
            if exit.packs.value() == 0
                || !groups.insert(exit.group)
                || event.recipient.value.kind() != ReferenceKind::Party
                || event.stock_revision.value() > self.stock_revision.value()
                || event.occurred.validate(event.recorded_at).is_err()
            {
                return Err(TraceError::InvalidSnapshot);
            }
            if let Some(previous) = departures.insert(event.id, event)
                && previous != event
            {
                return Err(TraceError::InvalidSnapshot);
            }
            if let Some(previous) = operations.insert(event.operation, event.id)
                && previous != event.id
            {
                return Err(TraceError::InvalidSnapshot);
            }
            departed = departed
                .checked_add(exit.packs)
                .map_err(|_| TraceError::Accounting)?;
        }
        if on_hand
            .checked_add(departed)
            .map_err(|_| TraceError::Accounting)?
            != self.trace.original_packs
        {
            return Err(TraceError::Accounting);
        }
        Ok(TraceBalance {
            variant: self.trace.variant.id,
            original: self.trace.original_packs,
            on_hand,
            departed,
        })
    }
}

#[must_use]
pub const fn source_certainty_label(source: &SourceAttribution) -> &'static str {
    match source {
        SourceAttribution::Unknown => "Unknown",
        SourceAttribution::Referenced {
            certainty: SourceCertainty::ReportedPossible,
            ..
        } => "Reported possible",
        SourceAttribution::Referenced {
            certainty: SourceCertainty::Established,
            ..
        } => "Established",
    }
}

#[cfg(test)]
mod tests;
