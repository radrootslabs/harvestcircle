//! Distinct complete-group physical disposition, without buyer delivery or count loss.
use crate::commands::HarvestCircleError;
use crate::hub_departure::HubDepartureWorkChangeDto;
use crate::hub_handling::HubPhysicalGroupDto;
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::HubOccurrenceDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_domain::hub::{
    disposition::*,
    reference::{SourceProvenance, WholePacks},
};
use harvestcircle_storage::HubDispositionReceipt;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubDispositionKindDto {
    Destroyed,
    OtherObservedRemoval,
}
impl From<DispositionKind> for HubDispositionKindDto {
    fn from(value: DispositionKind) -> Self {
        match value {
            DispositionKind::Destroyed => Self::Destroyed,
            DispositionKind::OtherObservedRemoval => Self::OtherObservedRemoval,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispositionInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_id: String,
    pub expected_group_revision: u64,
    pub whole_packs: u32,
    pub kind: HubDispositionKindDto,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubDispositionInputDto {
    pub fn command(self) -> Result<DispositionCommand, HarvestCircleError> {
        let packs = WholePacks::from_value(self.whole_packs).map_err(|_| invalid_input())?;
        if packs.value() == 0 {
            return Err(invalid_input());
        }
        Ok(DispositionCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            group: id(&self.group_id)?,
            expected_group_revision: revision(self.expected_group_revision)?,
            packs,
            kind: match self.kind {
                HubDispositionKindDto::Destroyed => DispositionKind::Destroyed,
                HubDispositionKindDto::OtherObservedRemoval => {
                    DispositionKind::OtherObservedRemoval
                }
            },
            occurred: self.occurrence.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispositionReceiptDto {
    pub id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub before: HubPhysicalGroupDto,
    pub after_group_revision: u64,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub location: HubReferenceRecordDto,
    pub kind: HubDispositionKindDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
    pub work_changes: Vec<HubDepartureWorkChangeDto>,
}
impl From<HubDispositionReceipt> for HubDispositionReceiptDto {
    fn from(value: HubDispositionReceipt) -> Self {
        let record = value.original;
        Self {
            id: record.identities.disposition.to_hex(),
            operation_id: value.operation.as_str().to_owned(),
            stock_revision: record.stock_revision.value(),
            catalog_revision: record.catalog_revision.value(),
            before: (&record.consumed.before).into(),
            after_group_revision: record.consumed.after_revision.value(),
            product: (&record.product).into(),
            variant: (&record.variant).into(),
            location: (&record.location).into(),
            kind: record.kind.into(),
            actor_public_key: record.actor.to_hex(),
            recorded_at_unix_s: record.recorded_at.as_seconds(),
            occurrence: record.occurred.into(),
            observation: record.observation.as_str().to_owned(),
            work_changes: record.work_changes.iter().map(Into::into).collect(),
        }
    }
}
