//! Closed committed recipient command codec; never a pending-input decoder.
use super::*;
use harvestcircle_domain::hub::{
    corrections::RecipientAttributionCommand, reference::SourceProvenance,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    departure: String,
    stock_revision: u64,
    catalog_revision: u64,
    prior: Option<String>,
    recipient: Option<String>,
    basis: String,
}
pub(super) fn encode(command: &RecipientAttributionCommand) -> Result<Vec<u8>, SafeError> {
    let bytes = serde_json::to_vec(&Data {
        schema: 1,
        departure: command.departure_operation.to_hex(),
        stock_revision: command.expected_stock_revision.value(),
        catalog_revision: command.expected_catalog_revision.value(),
        prior: command.expected_prior.map(HubRecordId::to_hex),
        recipient: command.recipient.map(HubRecordId::to_hex),
        basis: command.basis.as_str().into(),
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}
pub(super) fn decode(bytes: &[u8]) -> Result<RecipientAttributionCommand, SafeError> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if data.schema != 1 {
        return Err(corrupt_storage());
    }
    let id = super::additional_payload::id;
    let command = RecipientAttributionCommand {
        departure_operation: id(&data.departure)?,
        expected_stock_revision: HubRevision::from_value(data.stock_revision)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: HubRevision::from_value(data.catalog_revision)
            .map_err(|_| corrupt_storage())?,
        expected_prior: data.prior.as_deref().map(id).transpose()?,
        recipient: data.recipient.as_deref().map(id).transpose()?,
        basis: SourceProvenance::new(data.basis).map_err(|_| corrupt_storage())?,
    };
    if encode(&command)? != bytes {
        return Err(corrupt_storage());
    }
    Ok(command)
}
