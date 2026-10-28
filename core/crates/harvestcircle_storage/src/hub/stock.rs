//! Protected whole-pack inventory with immutable originals and operation results.

use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::HubRevision;
use harvestcircle_domain::hub::reference::{
    HUB_LABEL_UTF8_BYTES, HubLabel, HubRecordId, ReferenceRecord, SampleHandlingBasis,
};
use harvestcircle_domain::hub::stock::{
    HUB_STOCK_PAGE_ROWS, HUB_TRACE_CAPACITY, QuantityWork, ReceiptPlan, ReceiptTrace, StockPosition,
};
use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};

fn receipt_ids(
    operation: &DurableRequestId,
) -> Result<harvestcircle_domain::hub::stock::ReceiptIdentities, SafeError> {
    let bytes =
        HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| invalid_input())?;
    harvestcircle_domain::hub::stock::ReceiptIdentities::from_operation_bytes(bytes)
        .map_err(|_| invalid_input())
}

mod read;
pub(crate) mod schema;
mod write;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubStockReceipt {
    pub operation: DurableRequestId,
    pub original: ReceiptPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubStockRow {
    pub product: ReferenceRecord,
    pub position: StockPosition,
    pub location: ReferenceRecord,
    pub trace: ReceiptTrace,
    pub put_away: QuantityWork,
    pub work_revision: HubRevision,
    pub work_target: Option<ReferenceRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubStockQuery {
    pub expected_catalog_revision: Option<HubRevision>,
    pub expected_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
    pub location: Option<HubRecordId>,
    pub trace: Option<HubRecordId>,
    pub text: String,
}

impl HubStockQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        if !(1..=HUB_STOCK_PAGE_ROWS)
            .contains(&usize::try_from(self.limit).map_err(|_| invalid_input())?)
            || usize::try_from(self.offset).map_err(|_| invalid_input())? > HUB_TRACE_CAPACITY
            || self.text.len() > HUB_LABEL_UTF8_BYTES
        {
            return Err(invalid_input());
        }
        if !self.text.is_empty() {
            HubLabel::new(self.text.clone()).map_err(|_| invalid_input())?;
        }
        if self.offset != 0
            && (self.expected_revision.is_none() || self.expected_catalog_revision.is_none())
        {
            return Err(conflict());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubStockPage {
    pub handling_basis: SampleHandlingBasis,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubStockRow>,
}

const fn invalid_input() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubInvalidInput,
        SafeMessage::new("Review the whole-pack inventory input and its bounded scope."),
    )
}
const fn conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubConflict,
        SafeMessage::new(
            "Inventory or original operation context changed. Retain input and review current records.",
        ),
    )
}
const fn capacity() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubCapacity,
        SafeMessage::new(
            "The local demonstration history is full. Existing records remain available.",
        ),
    )
}

#[cfg(test)]
mod tests;
