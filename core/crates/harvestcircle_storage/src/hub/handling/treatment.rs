//! Immutable sample treatment over the existing stock operation journal.
mod history;
pub(crate) mod schema;
pub(super) use history::original_on;
pub(in crate::hub) mod capacity;
pub(super) mod read;
mod write;

use super::*;
use harvestcircle_domain::hub::reference::SampleHandlingBasis;
use harvestcircle_domain::hub::treatment::{
    TreatmentChange, TreatmentProjection, TreatmentSummary,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTreatmentResult {
    pub change: TreatmentChange,
    pub summary: TreatmentSummary,
    pub affected_packs: WholePacks,
    pub history_capacity_reached: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTreatmentQuery {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
}

impl HubTreatmentQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        if self.limit == 0 || self.limit as usize > HUB_STOCK_PAGE_ROWS || self.offset > 512 {
            return Err(invalid_input());
        }
        if self.offset > 0
            && (self.expected_revision.is_none() || self.expected_catalog_revision.is_none())
        {
            return Err(conflict());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTreatmentPage {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub basis: SampleHandlingBasis,
    pub rows: Vec<HubHandlingReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTreatmentScope {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubHoldRow {
    pub hold: harvestcircle_domain::hub::treatment::Hold,
    pub created_operation: DurableRequestId,
    pub released_operation: Option<DurableRequestId>,
    pub current_packs: WholePacks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTreatmentSnapshot {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub trace: ReceiptTrace,
    pub projection: TreatmentProjection,
    pub normal_operations_available: bool,
    pub holds: Vec<HubHoldRow>,
}

fn treatment_error(error: harvestcircle_domain::hub::treatment::TreatmentError) -> SafeError {
    use harvestcircle_domain::hub::treatment::TreatmentError;
    match error {
        TreatmentError::Capacity | TreatmentError::RevisionExhausted => capacity(),
        TreatmentError::StaleRevision | TreatmentError::HoldClosed => conflict(),
        TreatmentError::InvalidSelection
        | TreatmentError::InvalidOccurrence
        | TreatmentError::ActionBlocked
        | TreatmentError::Scope(
            harvestcircle_domain::hub::portions::PhysicalScopeError::InvalidSelection,
        ) => invalid_input(),
        TreatmentError::InvalidHistory | TreatmentError::Scope(_) => crate::db::corrupt_storage(),
    }
}

#[cfg(test)]
mod tests;
