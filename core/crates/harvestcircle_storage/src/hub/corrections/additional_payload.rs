//! Bounded descriptive root data. Domain requests have no Deserialize impl;
//! original reads must separately reconstruct canonical native facts and seals.
use super::*;
use harvestcircle_domain::hub::{corrections::AdditionalQuantityCommand, stock::Occurrence};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Description {
    pub schema: u8,
    pub target_operation: String,
    pub target_group: String,
    pub report: String,
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub expected_prior: Option<String>,
    pub corrected_line_packs: u32,
    pub recipient: Option<String>,
    pub occurrence_kind: String,
    pub occurrence_start: Option<i64>,
    pub occurrence_end: Option<i64>,
    pub source_operation: String,
    pub source_group: String,
    pub balancing_group: String,
    pub expected_balancing_revision: u64,
    pub source_evidence: String,
    pub quantity_evidence: String,
    pub recipient_evidence: String,
    pub timing_evidence: String,
    pub duplicate_review_basis: String,
}

pub(super) fn encode(command: &AdditionalQuantityCommand) -> Result<Vec<u8>, SafeError> {
    let f = &command.facts;
    let (kind, start, end) = match f.occurred {
        Occurrence::Unknown => ("unknown", None, None),
        Occurrence::Known(at) => ("known", Some(at.as_seconds()), Some(at.as_seconds())),
        Occurrence::Interval { earliest, latest } => (
            "interval",
            Some(earliest.as_seconds()),
            Some(latest.as_seconds()),
        ),
    };
    bytes(&Description {
        schema: 1,
        target_operation: f.target_operation.to_hex(),
        target_group: f.target_group.to_hex(),
        report: f.report.to_hex(),
        expected_stock_revision: f.expected_stock_revision.value(),
        expected_catalog_revision: f.expected_catalog_revision.value(),
        expected_prior: f.expected_prior.map(HubRecordId::to_hex),
        corrected_line_packs: f.corrected_line_packs.value(),
        recipient: f.recipient.map(HubRecordId::to_hex),
        occurrence_kind: kind.into(),
        occurrence_start: start,
        occurrence_end: end,
        source_operation: command.source_operation.to_hex(),
        source_group: command.source_group.to_hex(),
        balancing_group: command.balancing_group.to_hex(),
        expected_balancing_revision: command.expected_balancing_revision.value(),
        source_evidence: f.source_evidence.as_str().into(),
        quantity_evidence: f.quantity_evidence.as_str().into(),
        recipient_evidence: f.recipient_evidence.as_str().into(),
        timing_evidence: f.timing_evidence.as_str().into(),
        duplicate_review_basis: command.duplicate_review_basis.as_str().into(),
    })
}

fn bytes(value: &Description) -> Result<Vec<u8>, SafeError> {
    let data = serde_json::to_vec(value).map_err(|_| corrupt_storage())?;
    if value.schema != 1 || data.is_empty() || data.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    Ok(data)
}

pub(super) fn read(data: &[u8]) -> Result<Description, SafeError> {
    if data.is_empty() || data.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let value: Description = serde_json::from_slice(data).map_err(|_| corrupt_storage())?;
    if bytes(&value)? != data {
        return Err(corrupt_storage());
    }
    Ok(value)
}

pub(super) fn id(value: &str) -> Result<HubRecordId, SafeError> {
    let id = HubRecordId::from_hex(value).map_err(|_| corrupt_storage())?;
    if id.to_hex() != value {
        return Err(corrupt_storage());
    }
    Ok(id)
}

pub(super) fn occurrence(value: &Description) -> Result<Occurrence, SafeError> {
    let timestamp = |v: i64| UnixTimestamp::from_seconds(v).ok_or_else(corrupt_storage);
    match (
        value.occurrence_kind.as_str(),
        value.occurrence_start,
        value.occurrence_end,
    ) {
        ("unknown", None, None) => Ok(Occurrence::Unknown),
        ("known", Some(start), Some(end)) if start == end => {
            Ok(Occurrence::Known(timestamp(start)?))
        }
        ("interval", Some(start), Some(end)) if start <= end => Ok(Occurrence::Interval {
            earliest: timestamp(start)?,
            latest: timestamp(end)?,
        }),
        _ => Err(corrupt_storage()),
    }
}
