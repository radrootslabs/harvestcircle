//! Non-reserving native preparation with immutable issued identification.
mod material;
pub(super) mod read;
pub(crate) mod schema;
mod write;

use super::*;
use harvestcircle_domain::hub::dispatch::{
    DispatchCommand, DispatchError, DispatchIdentities, DispatchRecord,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubDispatchKind {
    Prepare,
    Revise,
    Issue,
    Replace,
    MarkUnused,
}
impl HubDispatchKind {
    fn parse(value: &str) -> Result<Self, SafeError> {
        match value {
            "dispatch_prepare" => Ok(Self::Prepare),
            "dispatch_revise" => Ok(Self::Revise),
            "dispatch_issue" => Ok(Self::Issue),
            "dispatch_replace" => Ok(Self::Replace),
            "dispatch_unused" => Ok(Self::MarkUnused),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDispatchReceipt {
    pub operation: DurableRequestId,
    pub kind: HubDispatchKind,
    pub catalog_revision: HubRevision,
    pub record: DispatchRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubDispatchFilter {
    All,
    Draft,
    Issued,
    Unused,
    Departed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubDispatchState {
    Draft,
    Issued,
    Unused,
    Departed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDispatchRow {
    pub id: HubRecordId,
    pub revision: HubRevision,
    pub state: HubDispatchState,
    pub recipient: ReferenceRecord,
    pub whole_packs: u32,
    pub line_count: u32,
    pub version_count: u32,
    pub identification: Option<HubRecordId>,
    pub updated: harvestcircle_domain::hub::dispatch::DispatchStamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDispatchQuery {
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
    pub filter: HubDispatchFilter,
    pub text: String,
}
impl HubDispatchQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        if !(1..=64).contains(&self.limit) || self.offset > 1_024 || self.text.len() > 120 {
            return Err(invalid_input());
        }
        if !self.text.is_empty() {
            HubLabel::new(self.text.clone()).map_err(|_| invalid_input())?;
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
pub struct HubDispatchPage {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubDispatchRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDispatchView {
    pub state: HubDispatchState,
    pub departure: Option<super::departure::HubDepartureUse>,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub record: DispatchRecord,
}

fn identities(operation: &DurableRequestId) -> Result<DispatchIdentities, SafeError> {
    let id =
        HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| invalid_input())?;
    DispatchIdentities::from_operation_bytes(id).map_err(dispatch_error)
}
fn corrupt() -> SafeError {
    crate::db::corrupt_storage()
}
fn dispatch_error(error: DispatchError) -> SafeError {
    let (code, message) = match error {
        DispatchError::StaleRevision => (
            SafeErrorCode::HubConflict,
            "Stock, references or this preparation changed. Review the current context before submitting.",
        ),
        DispatchError::PreviewChanged => (
            SafeErrorCode::HubConflict,
            "The saved preparation differs from current material. Revise the preparation before issuing identification.",
        ),
        DispatchError::InvalidState => (
            SafeErrorCode::HubConflict,
            "This preparation no longer permits that action. Check its current identification and status.",
        ),
        DispatchError::NoMaterialChange => (
            SafeErrorCode::HubConflict,
            "Replacement needs changed material. Retain the current identification or explicitly mark it unused.",
        ),
        DispatchError::Capacity => (
            SafeErrorCode::HubCapacity,
            "The preparation history limit is reached. Existing issued identification can still be marked unused.",
        ),
        DispatchError::RevisionExhausted => (
            SafeErrorCode::HubCapacity,
            "The revision limit is reached. Keep the original identification for review.",
        ),
        DispatchError::InvalidHistory => return corrupt(),
        DispatchError::InvalidSelection => (
            SafeErrorCode::HubInvalidInput,
            "Choose a recipient and valid whole identified stock groups for this preparation.",
        ),
        DispatchError::PhysicalSeparationRequired => (
            SafeErrorCode::HubInvalidInput,
            "Select the whole identified group. Record observed physical separation first to identify fewer packs.",
        ),
    };
    SafeError::new(code, SafeMessage::new(message))
}

#[cfg(test)]
mod tests;

fn current_state(record: &DispatchRecord, used: bool) -> Result<HubDispatchState, SafeError> {
    if used {
        return Ok(HubDispatchState::Departed);
    }
    if record.draft.is_some() {
        return Ok(HubDispatchState::Draft);
    }
    match record.versions.last().ok_or_else(corrupt)?.status {
        harvestcircle_domain::hub::dispatch::IdentificationStatus::Issued => {
            Ok(HubDispatchState::Issued)
        }
        harvestcircle_domain::hub::dispatch::IdentificationStatus::Unused => {
            Ok(HubDispatchState::Unused)
        }
        harvestcircle_domain::hub::dispatch::IdentificationStatus::Superseded => Err(corrupt()),
    }
}
