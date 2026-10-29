//! Actual departure is distinct from paper issuance and uses native authority.
use crate::commands::HarvestCircleError;
use crate::dto::SafeErrorDto;
use crate::hub_dispatch::{HubDispatchRecordDto, HubDispatchViewDto};
use crate::hub_handling::HubWorkRequirementDto;
use crate::hub_reference::invalid_input;
use crate::hub_stock::HubOccurrenceDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_storage::{
    HubDepartureReceipt, HubDepartureReview, HubDepartureReviewInput, HubDepartureUse,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureReviewInputDto {
    pub preparation_id: String,
    pub expected_preparation_revision: u64,
    pub identification_id: String,
}
impl HubDepartureReviewInputDto {
    pub fn decode(self) -> Result<HubDepartureReviewInput, HarvestCircleError> {
        Ok(HubDepartureReviewInput {
            preparation: id(&self.preparation_id)?,
            expected_preparation_revision: revision(self.expected_preparation_revision)?,
            identification: id(&self.identification_id)?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureReviewDto {
    pub request: HubDepartureReviewInputDto,
    pub view: HubDispatchViewDto,
    pub ready: bool,
    pub block: Option<SafeErrorDto>,
    pub proposed_work_changes: Vec<HubDepartureWorkChangeDto>,
}
impl TryFrom<HubDepartureReview> for HubDepartureReviewDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubDepartureReview) -> Result<Self, Self::Error> {
        Ok(Self {
            request: HubDepartureReviewInputDto {
                preparation_id: value.request.preparation.to_hex(),
                expected_preparation_revision: value.request.expected_preparation_revision.value(),
                identification_id: value.request.identification.to_hex(),
            },
            view: value.view.try_into()?,
            ready: value.block.is_none(),
            block: value.block.map(Into::into),
            proposed_work_changes: value.proposed_work_changes.iter().map(Into::into).collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub preparation_id: String,
    pub expected_preparation_revision: u64,
    pub identification_id: String,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubDepartureInputDto {
    pub fn command(self) -> Result<DepartureCommand, HarvestCircleError> {
        Ok(DepartureCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            preparation: id(&self.preparation_id)?,
            expected_preparation_revision: revision(self.expected_preparation_revision)?,
            identification: id(&self.identification_id)?,
            occurred: self.occurrence.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureUseDto {
    pub id: String,
    pub operation_id: String,
    pub preparation_id: String,
    pub identification_id: String,
    pub stock_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
}
impl From<HubDepartureUse> for HubDepartureUseDto {
    fn from(value: HubDepartureUse) -> Self {
        Self {
            id: value.id.to_hex(),
            operation_id: value.operation.as_str().to_owned(),
            preparation_id: value.preparation.to_hex(),
            identification_id: value.identification.to_hex(),
            stock_revision: value.revision.value(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureLineDto {
    pub group_id: String,
    pub before_group_revision: u64,
    pub after_group_revision: u64,
    pub whole_packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureWorkChangeDto {
    pub before: HubWorkRequirementDto,
    pub after: HubWorkRequirementDto,
    pub retired_whole_packs: u32,
}
impl From<&harvestcircle_domain::hub::departure::DepartureWorkChange>
    for HubDepartureWorkChangeDto
{
    fn from(value: &harvestcircle_domain::hub::departure::DepartureWorkChange) -> Self {
        Self {
            before: (&value.before).into(),
            after: (&value.after).into(),
            retired_whole_packs: value.retired.value(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDepartureReceiptDto {
    pub id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
    pub paper: HubDispatchRecordDto,
    pub lines: Vec<HubDepartureLineDto>,
    pub work_changes: Vec<HubDepartureWorkChangeDto>,
}
impl TryFrom<HubDepartureReceipt> for HubDepartureReceiptDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubDepartureReceipt) -> Result<Self, Self::Error> {
        let record = value.record;
        Ok(Self {
            id: record.id.to_hex(),
            operation_id: value.operation.as_str().to_owned(),
            stock_revision: record.stock_revision.value(),
            catalog_revision: record.catalog_revision.value(),
            actor_public_key: record.actor.to_hex(),
            recorded_at_unix_s: record.recorded_at.as_seconds(),
            occurrence: record.occurred.into(),
            observation: record.observation.as_str().to_owned(),
            paper: (&record.paper).try_into()?,
            lines: record
                .consumed
                .iter()
                .map(|line| HubDepartureLineDto {
                    group_id: line.before.id.to_hex(),
                    before_group_revision: line.before.revision.value(),
                    after_group_revision: line.after_revision.value(),
                    whole_packs: line.before.packs.value(),
                })
                .collect(),
            work_changes: record.work_changes.iter().map(Into::into).collect(),
        })
    }
}
