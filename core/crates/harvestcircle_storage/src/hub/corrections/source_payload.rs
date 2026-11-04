//! Committed source-root description only. No pending archive decoder or grant.
use super::*;
use harvestcircle_domain::hub::{
    corrections::SourceAttributionCommand,
    reference::{
        SourceAttribution, SourceCertainty, SourceCode, SourceProvenance, SourceReference,
        SourceReferenceKind,
    },
};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    trace: String,
    stock_revision: u64,
    catalog_revision: u64,
    prior: Option<String>,
    source: Option<Source>,
    basis: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    issuer: Option<String>,
    kind: String,
    code: String,
    provenance: String,
    certainty: String,
}

pub(super) fn encode(command: &SourceAttributionCommand) -> Result<Vec<u8>, SafeError> {
    let source = match &command.source {
        SourceAttribution::Unknown => None,
        SourceAttribution::Referenced { source, certainty } => Some(Source {
            id: source.id.to_hex(),
            issuer: source.issuer.map(HubRecordId::to_hex),
            kind: match source.kind {
                SourceReferenceKind::ProducerBatch => "producer_batch",
                SourceReferenceKind::SupplierReference => "supplier_reference",
                SourceReferenceKind::Other => "other",
            }
            .into(),
            code: source.code.as_str().into(),
            provenance: source.provenance.as_str().into(),
            certainty: match certainty {
                SourceCertainty::Established => "established",
                SourceCertainty::ReportedPossible => "possible",
            }
            .into(),
        }),
    };
    let data = Data {
        schema: 1,
        trace: command.trace.to_hex(),
        stock_revision: command.expected_stock_revision.value(),
        catalog_revision: command.expected_catalog_revision.value(),
        prior: command.expected_prior.map(HubRecordId::to_hex),
        source,
        basis: command.basis.as_str().into(),
    };
    let bytes = serde_json::to_vec(&data).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> Result<SourceAttributionCommand, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if data.schema != 1 {
        return Err(corrupt_storage());
    }
    let id = super::additional_payload::id;
    let source = match data.source {
        None => SourceAttribution::Unknown,
        Some(s) => SourceAttribution::Referenced {
            source: SourceReference {
                id: id(&s.id)?,
                issuer: s.issuer.as_deref().map(id).transpose()?,
                kind: match s.kind.as_str() {
                    "producer_batch" => SourceReferenceKind::ProducerBatch,
                    "supplier_reference" => SourceReferenceKind::SupplierReference,
                    "other" => SourceReferenceKind::Other,
                    _ => return Err(corrupt_storage()),
                },
                code: SourceCode::new(s.code).map_err(|_| corrupt_storage())?,
                provenance: SourceProvenance::new(s.provenance).map_err(|_| corrupt_storage())?,
            },
            certainty: match s.certainty.as_str() {
                "established" => SourceCertainty::Established,
                "possible" => SourceCertainty::ReportedPossible,
                _ => return Err(corrupt_storage()),
            },
        },
    };
    let command = SourceAttributionCommand {
        trace: id(&data.trace)?,
        expected_stock_revision: HubRevision::from_value(data.stock_revision)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: HubRevision::from_value(data.catalog_revision)
            .map_err(|_| corrupt_storage())?,
        expected_prior: data.prior.as_deref().map(id).transpose()?,
        source,
        basis: SourceProvenance::new(data.basis).map_err(|_| corrupt_storage())?,
    };
    if encode(&command)? != bytes {
        return Err(corrupt_storage());
    }
    Ok(command)
}
