//! Current sample treatment over immutable captured physical populations.
//! These checked facts never establish actor credentials or future authority.

mod command;
mod result;
mod state;
pub use command::{
    TreatmentAction, TreatmentChange, TreatmentCommand, TreatmentPlan, treatment_record_id,
};
pub use result::treatment_result_digest;
pub use state::{
    AcceptanceDecision, GroupTreatment, Hold, TreatmentProjection, TreatmentStamp, TreatmentState,
    TreatmentSummary,
};

use super::portions::PhysicalScopeError;
use super::reference::HubRecordId;

pub const HUB_TREATMENT_SELECTION_GROUPS: usize = 64;
pub const HUB_TREATMENT_RECORDS_PER_TRACE: usize = 256;
pub const HUB_TREATMENT_CAPACITY: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcceptanceStatus {
    ReviewRequired,
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalAction {
    InternalRelocation,
    ObservedSeparation,
    NormalDeparture,
    Disposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreatmentError {
    Capacity,
    InvalidSelection,
    InvalidHistory,
    InvalidOccurrence,
    StaleRevision,
    RevisionExhausted,
    HoldClosed,
    ActionBlocked,
    Scope(PhysicalScopeError),
}

impl From<PhysicalScopeError> for TreatmentError {
    fn from(value: PhysicalScopeError) -> Self {
        Self::Scope(value)
    }
}

/// Whole currently identified groups; partial treatment first requires observed separation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreatmentSelection(Vec<HubRecordId>);

impl TreatmentSelection {
    pub fn new(mut groups: Vec<HubRecordId>) -> Result<Self, TreatmentError> {
        if groups.is_empty() {
            return Err(TreatmentError::InvalidSelection);
        }
        if groups.len() > HUB_TREATMENT_SELECTION_GROUPS {
            return Err(TreatmentError::Capacity);
        }
        groups.sort_by_key(|id| *id.as_bytes());
        if groups.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(TreatmentError::InvalidSelection);
        }
        Ok(Self(groups))
    }
    #[must_use]
    pub fn groups(&self) -> &[HubRecordId] {
        &self.0
    }
}

#[cfg(test)]
mod tests;
