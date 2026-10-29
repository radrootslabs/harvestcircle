//! Closed identified-handling transport. Native authority supplies actor and time.

use crate::commands::HarvestCircleError;
use crate::hub_portions::HubSplitResultDto;
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::{HubOccurrenceDto, HubQuantityWorkDto, HubStockPopulationDto};
use crate::hub_treatment::HubTreatmentResultDto;
use crate::hub_treatment_input::{HubAcceptanceInputDto, HubHoldInputDto, HubHoldReleaseInputDto};
use harvestcircle_domain::hub::HubRevision;
use harvestcircle_domain::hub::movement::{
    DiscrepancyAction, DiscrepancyCommand, HUB_REQUIREMENT_GROUPS, HandlingCommand,
    MovementCommand, NewWorkCommand, PhysicalIdentification, ReplanCommand, WorkProgress,
    WorkRequirement,
};
use harvestcircle_domain::hub::portions::SplitCommand;
use harvestcircle_domain::hub::reference::{HubLabel, HubRecordId, SourceProvenance, WholePacks};
use harvestcircle_storage::{
    HubActionGroup, HubDiscrepancyRow, HubGroupView, HubHandlingKind, HubHandlingPage,
    HubHandlingQuery, HubHandlingReceipt, HubHandlingResult, HubMovementResult,
    HubWorkActionResult, HubWorkChange, HubWorkOrigin, HubWorkRow,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubPhysicalIdentificationDto {
    pub label: String,
    pub observation: String,
}

impl HubPhysicalIdentificationDto {
    fn decode(self) -> Result<PhysicalIdentification, HarvestCircleError> {
        Ok(PhysicalIdentification {
            label: HubLabel::new(self.label).map_err(|_| invalid_input())?,
            observation: provenance(self.observation)?,
        })
    }
}
impl From<&PhysicalIdentification> for HubPhysicalIdentificationDto {
    fn from(value: &PhysicalIdentification) -> Self {
        Self {
            label: value.label.as_str().to_owned(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubMovementInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_id: String,
    pub expected_group_revision: u64,
    pub to_location_id: String,
    pub whole_packs: u32,
    pub separated_portion: Option<HubPhysicalIdentificationDto>,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubSplitInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_id: String,
    pub expected_group_revision: u64,
    pub whole_packs: u32,
    pub identification: HubPhysicalIdentificationDto,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReplanInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub requirement_id: String,
    pub expected_work_revision: u64,
    pub target_location_id: String,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubNewWorkInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_ids: Vec<String>,
    pub target_location_id: String,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDiscrepancyOpenInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub trace_id: String,
    pub group_id: Option<String>,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDiscrepancyResolveInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub discrepancy_id: String,
    pub expected_revision: u64,
    pub resolution: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubHandlingInputDto {
    SetAcceptance {
        input: HubAcceptanceInputDto,
    },
    PlaceHold {
        input: HubHoldInputDto,
    },
    ReleaseHold {
        input: HubHoldReleaseInputDto,
    },
    Split {
        input: HubSplitInputDto,
    },
    Move {
        input: HubMovementInputDto,
    },
    Replan {
        input: HubReplanInputDto,
    },
    NewWork {
        input: HubNewWorkInputDto,
    },
    OpenDiscrepancy {
        input: HubDiscrepancyOpenInputDto,
    },
    ResolveDiscrepancy {
        input: HubDiscrepancyResolveInputDto,
    },
}

impl HubHandlingInputDto {
    pub fn command(self) -> Result<HandlingCommand, HarvestCircleError> {
        Ok(match self {
            Self::SetAcceptance { input } => HandlingCommand::Treatment(input.command()?),
            Self::PlaceHold { input } => HandlingCommand::Treatment(input.command()?),
            Self::ReleaseHold { input } => HandlingCommand::Treatment(input.command()?),
            Self::Move { input } => {
                let packs =
                    WholePacks::from_value(input.whole_packs).map_err(|_| invalid_input())?;
                if packs.value() == 0 {
                    return Err(invalid_input());
                }
                HandlingCommand::Move(MovementCommand {
                    expected_stock_revision: revision(input.expected_stock_revision)?,
                    expected_catalog_revision: revision(input.expected_catalog_revision)?,
                    group: id(&input.group_id)?,
                    expected_group_revision: revision(input.expected_group_revision)?,
                    to: id(&input.to_location_id)?,
                    packs,
                    separated_portion: input
                        .separated_portion
                        .map(HubPhysicalIdentificationDto::decode)
                        .transpose()?,
                    occurred: input.occurrence.decode()?,
                    observation: provenance(input.observation)?,
                })
            }
            Self::Split { input } => {
                let packs =
                    WholePacks::from_value(input.whole_packs).map_err(|_| invalid_input())?;
                if packs.value() == 0 {
                    return Err(invalid_input());
                }
                HandlingCommand::Split(SplitCommand {
                    expected_stock_revision: revision(input.expected_stock_revision)?,
                    expected_catalog_revision: revision(input.expected_catalog_revision)?,
                    group: id(&input.group_id)?,
                    expected_group_revision: revision(input.expected_group_revision)?,
                    packs,
                    identification: input.identification.decode()?,
                    occurred: input.occurrence.decode()?,
                    observation: provenance(input.observation)?,
                })
            }
            Self::Replan { input } => HandlingCommand::Replan(ReplanCommand {
                expected_stock_revision: revision(input.expected_stock_revision)?,
                expected_catalog_revision: revision(input.expected_catalog_revision)?,
                requirement: id(&input.requirement_id)?,
                expected_work_revision: revision(input.expected_work_revision)?,
                target: id(&input.target_location_id)?,
                observation: provenance(input.observation)?,
            }),
            Self::NewWork { input } => {
                if input.group_ids.is_empty() || input.group_ids.len() > HUB_REQUIREMENT_GROUPS {
                    return Err(invalid_input());
                }
                let mut groups = input
                    .group_ids
                    .iter()
                    .map(|value| id(value))
                    .collect::<Result<Vec<_>, _>>()?;
                groups.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                let command = NewWorkCommand {
                    expected_stock_revision: revision(input.expected_stock_revision)?,
                    expected_catalog_revision: revision(input.expected_catalog_revision)?,
                    groups,
                    target: id(&input.target_location_id)?,
                    observation: provenance(input.observation)?,
                };
                command.validate_shape().map_err(|_| invalid_input())?;
                HandlingCommand::NewWork(command)
            }
            Self::OpenDiscrepancy { input } => HandlingCommand::Discrepancy(DiscrepancyCommand {
                expected_stock_revision: revision(input.expected_stock_revision)?,
                expected_catalog_revision: revision(input.expected_catalog_revision)?,
                action: DiscrepancyAction::Open {
                    trace: id(&input.trace_id)?,
                    group: input.group_id.as_deref().map(id).transpose()?,
                    observation: provenance(input.observation)?,
                },
            }),
            Self::ResolveDiscrepancy { input } => {
                HandlingCommand::Discrepancy(DiscrepancyCommand {
                    expected_stock_revision: revision(input.expected_stock_revision)?,
                    expected_catalog_revision: revision(input.expected_catalog_revision)?,
                    action: DiscrepancyAction::Resolve {
                        discrepancy: id(&input.discrepancy_id)?,
                        expected_revision: revision(input.expected_revision)?,
                        resolution: provenance(input.resolution)?,
                    },
                })
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHandlingQueryDto {
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub trace_id: Option<String>,
    pub open_only: bool,
    pub text: String,
}
impl HubHandlingQueryDto {
    pub fn decode(self) -> Result<HubHandlingQuery, HarvestCircleError> {
        let query = HubHandlingQuery {
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
            trace: self.trace_id.as_deref().map(id).transpose()?,
            open_only: self.open_only,
            text: self.text,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubWorkProgressDto {
    Pending,
    Completed,
    NoLongerRequired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubHandlingKindDto {
    Acceptance,
    PlaceHold,
    ReleaseHold,
    Split,
    Move,
    Replan,
    NewWork,
    OpenDiscrepancy,
    ResolveDiscrepancy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubWorkOriginDto {
    Intake,
    Replan,
    NewHandling,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubWorkRequirementDto {
    pub id: String,
    pub trace_id: String,
    pub variant_id: String,
    pub quantity: HubQuantityWorkDto,
}
impl From<&WorkRequirement> for HubWorkRequirementDto {
    fn from(work: &WorkRequirement) -> Self {
        Self {
            id: work.id.to_hex(),
            trace_id: work.trace.to_hex(),
            variant_id: work.variant.to_hex(),
            quantity: HubQuantityWorkDto::from_work(
                work.quantity,
                work.revision,
                work.target.as_ref(),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubGroupWorkDto {
    pub requirement: HubWorkRequirementDto,
    pub progress: HubWorkProgressDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubPhysicalGroupDto {
    pub id: String,
    pub trace_id: String,
    pub variant_id: String,
    pub parent_id: Option<String>,
    pub identification: Option<HubPhysicalIdentificationDto>,
    pub location_id: String,
    pub whole_packs: u32,
    pub revision: u64,
    pub causal_floor_unix_s: Option<i64>,
    pub work: Vec<HubGroupWorkDto>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubGroupViewDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub group: HubPhysicalGroupDto,
    pub location: HubReferenceRecordDto,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub trace: HubStockPopulationDto,
}
impl From<HubGroupView> for HubGroupViewDto {
    fn from(view: HubGroupView) -> Self {
        let group = &view.group;
        Self {
            stock_revision: view.revision.value(),
            catalog_revision: view.catalog_revision.value(),
            location: (&view.location).into(),
            product: (&view.product).into(),
            variant: (&view.variant).into(),
            trace: (&view.trace).into(),
            group: HubPhysicalGroupDto {
                id: group.id.to_hex(),
                trace_id: group.trace.to_hex(),
                variant_id: group.variant.to_hex(),
                parent_id: group.parent.map(|id| id.to_hex()),
                identification: group.identification.as_ref().map(Into::into),
                location_id: group.location.to_hex(),
                whole_packs: group.packs.value(),
                revision: group.revision.value(),
                causal_floor_unix_s: group.causal_floor.map(|at| at.as_seconds()),
                work: group
                    .work
                    .iter()
                    .map(|allocation| HubGroupWorkDto {
                        requirement: (&allocation.requirement).into(),
                        progress: match allocation.progress {
                            WorkProgress::Pending => HubWorkProgressDto::Pending,
                            WorkProgress::Completed => HubWorkProgressDto::Completed,
                            WorkProgress::NoLongerRequired => HubWorkProgressDto::NoLongerRequired,
                        },
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubWorkChangeDto {
    pub before: Option<HubWorkRequirementDto>,
    pub after: HubWorkRequirementDto,
}
impl From<HubWorkChange> for HubWorkChangeDto {
    fn from(value: HubWorkChange) -> Self {
        Self {
            before: value.before.as_ref().map(Into::into),
            after: (&value.after).into(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubMovementResultDto {
    pub selected_group_id: String,
    pub expected_group_revision: u64,
    pub source_before_whole_packs: u32,
    pub from: HubReferenceRecordDto,
    pub to: HubReferenceRecordDto,
    pub whole_packs: u32,
    pub moved_group_id: String,
    pub remaining_whole_packs: u32,
    pub separated_portion: Option<HubPhysicalIdentificationDto>,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
    pub work_changes: Vec<HubWorkChangeDto>,
}
impl From<HubMovementResult> for HubMovementResultDto {
    fn from(value: HubMovementResult) -> Self {
        Self {
            selected_group_id: value.selected_group.to_hex(),
            expected_group_revision: value.expected_group_revision.value(),
            source_before_whole_packs: value.source_before_packs.value(),
            from: (&value.from).into(),
            to: (&value.to).into(),
            whole_packs: value.packs.value(),
            moved_group_id: value.moved_group.to_hex(),
            remaining_whole_packs: value.remaining_packs.value(),
            separated_portion: value.separated_portion.as_ref().map(Into::into),
            occurrence: value.occurred.into(),
            observation: value.observation.as_str().to_owned(),
            work_changes: value.work_changes.into_iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActionGroupDto {
    pub group_id: String,
    pub before_revision: u64,
    pub whole_packs: u32,
    pub location: HubReferenceRecordDto,
}
impl From<HubActionGroup> for HubActionGroupDto {
    fn from(value: HubActionGroup) -> Self {
        Self {
            group_id: value.group.to_hex(),
            before_revision: value.before_revision.value(),
            whole_packs: value.packs.value(),
            location: (&value.location).into(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubWorkActionResultDto {
    pub replacement: HubWorkRequirementDto,
    pub previous: Option<HubWorkChangeDto>,
    pub groups: Vec<HubActionGroupDto>,
    pub observation: String,
}
impl From<HubWorkActionResult> for HubWorkActionResultDto {
    fn from(value: HubWorkActionResult) -> Self {
        Self {
            replacement: (&value.replacement).into(),
            previous: value.previous.map(Into::into),
            groups: value.groups.into_iter().map(Into::into).collect(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDiscrepancyDto {
    pub id: String,
    pub trace_id: String,
    pub group_id: Option<String>,
    pub observation: String,
    pub revision: u64,
    pub resolution: Option<String>,
}
impl From<harvestcircle_domain::hub::movement::Discrepancy> for HubDiscrepancyDto {
    fn from(value: harvestcircle_domain::hub::movement::Discrepancy) -> Self {
        Self {
            id: value.id.to_hex(),
            trace_id: value.trace.to_hex(),
            group_id: value.group.map(|id| id.to_hex()),
            observation: value.observation.as_str().to_owned(),
            revision: value.revision.value(),
            resolution: value.resolution.map(|text| text.as_str().to_owned()),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubHandlingResultDto {
    Treatment { value: Box<HubTreatmentResultDto> },
    Separated { value: Box<HubSplitResultDto> },
    Moved { value: Box<HubMovementResultDto> },
    WorkChanged { value: Box<HubWorkActionResultDto> },
    DiscrepancyChanged { value: HubDiscrepancyDto },
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHandlingReceiptDto {
    pub operation_id: String,
    pub kind: HubHandlingKindDto,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub trace_id: String,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub result: HubHandlingResultDto,
}
impl From<HubHandlingReceipt> for HubHandlingReceiptDto {
    fn from(value: HubHandlingReceipt) -> Self {
        Self {
            operation_id: value.operation.as_str().to_owned(),
            kind: match value.kind {
                HubHandlingKind::Acceptance => HubHandlingKindDto::Acceptance,
                HubHandlingKind::PlaceHold => HubHandlingKindDto::PlaceHold,
                HubHandlingKind::ReleaseHold => HubHandlingKindDto::ReleaseHold,
                HubHandlingKind::Split => HubHandlingKindDto::Split,
                HubHandlingKind::Move => HubHandlingKindDto::Move,
                HubHandlingKind::Replan => HubHandlingKindDto::Replan,
                HubHandlingKind::NewWork => HubHandlingKindDto::NewWork,
                HubHandlingKind::OpenDiscrepancy => HubHandlingKindDto::OpenDiscrepancy,
                HubHandlingKind::ResolveDiscrepancy => HubHandlingKindDto::ResolveDiscrepancy,
            },
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            trace_id: value.trace.to_hex(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            result: match value.result {
                HubHandlingResult::Treatment(value) => HubHandlingResultDto::Treatment {
                    value: Box::new((*value).into()),
                },
                HubHandlingResult::Separated(value) => HubHandlingResultDto::Separated {
                    value: Box::new(value.into()),
                },
                HubHandlingResult::Moved(value) => HubHandlingResultDto::Moved {
                    value: Box::new(value.into()),
                },
                HubHandlingResult::WorkChanged(value) => HubHandlingResultDto::WorkChanged {
                    value: Box::new(value.into()),
                },
                HubHandlingResult::DiscrepancyChanged(value) => {
                    HubHandlingResultDto::DiscrepancyChanged {
                        value: value.into(),
                    }
                }
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubWorkRowDto {
    pub work: HubWorkRequirementDto,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub source_needs_review: bool,
    pub pending_groups: u32,
    pub origin: HubWorkOriginDto,
    pub observation: String,
    pub created_by: String,
    pub created_at_unix_s: i64,
    pub created_operation_id: String,
}
impl From<HubWorkRow> for HubWorkRowDto {
    fn from(value: HubWorkRow) -> Self {
        Self {
            work: (&value.work).into(),
            product: (&value.product).into(),
            variant: (&value.variant).into(),
            source_needs_review: value.source_needs_review,
            pending_groups: value.pending_groups,
            origin: match value.origin {
                HubWorkOrigin::Intake => HubWorkOriginDto::Intake,
                HubWorkOrigin::Replan => HubWorkOriginDto::Replan,
                HubWorkOrigin::NewHandling => HubWorkOriginDto::NewHandling,
            },
            observation: value.observation.as_str().to_owned(),
            created_by: value.created_by.to_hex(),
            created_at_unix_s: value.created_at.as_seconds(),
            created_operation_id: value.created_operation.as_str().to_owned(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubWorkPageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubWorkRowDto>,
}
impl From<HubHandlingPage<HubWorkRow>> for HubWorkPageDto {
    fn from(value: HubHandlingPage<HubWorkRow>) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value.rows.into_iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDiscrepancyRowDto {
    pub value: HubDiscrepancyDto,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub created_by: String,
    pub created_at_unix_s: i64,
    pub created_operation_id: String,
    pub resolved_by: Option<String>,
    pub resolved_at_unix_s: Option<i64>,
    pub resolved_operation_id: Option<String>,
}
impl From<HubDiscrepancyRow> for HubDiscrepancyRowDto {
    fn from(value: HubDiscrepancyRow) -> Self {
        Self {
            value: value.value.into(),
            product: (&value.product).into(),
            variant: (&value.variant).into(),
            created_by: value.created_by.to_hex(),
            created_at_unix_s: value.created_at.as_seconds(),
            created_operation_id: value.created_operation.as_str().to_owned(),
            resolved_by: value.resolved_by.map(|key| key.to_hex()),
            resolved_at_unix_s: value.resolved_at.map(|at| at.as_seconds()),
            resolved_operation_id: value
                .resolved_operation
                .map(|operation| operation.as_str().to_owned()),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDiscrepancyPageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubDiscrepancyRowDto>,
}
impl From<HubHandlingPage<HubDiscrepancyRow>> for HubDiscrepancyPageDto {
    fn from(value: HubHandlingPage<HubDiscrepancyRow>) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value.rows.into_iter().map(Into::into).collect(),
        }
    }
}

fn id(value: &str) -> Result<HubRecordId, HarvestCircleError> {
    HubRecordId::from_hex(value).map_err(|_| invalid_input())
}
fn revision(value: u64) -> Result<HubRevision, HarvestCircleError> {
    HubRevision::from_value(value).map_err(|_| invalid_input())
}
fn provenance(value: String) -> Result<SourceProvenance, HarvestCircleError> {
    SourceProvenance::new(value).map_err(|_| invalid_input())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> HubHandlingQueryDto {
        HubHandlingQueryDto {
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            trace_id: None,
            open_only: true,
            text: String::new(),
        }
    }

    #[test]
    fn handling_pages_reject_unpinned_continuations_and_unbounded_input() {
        assert!(query().decode().is_ok());
        let pinned = HubHandlingQueryDto {
            offset: 64,
            expected_stock_revision: Some(1),
            expected_catalog_revision: Some(5),
            ..query()
        };
        assert!(pinned.clone().decode().is_ok());
        for invalid in [
            HubHandlingQueryDto {
                limit: 0,
                ..query()
            },
            HubHandlingQueryDto {
                limit: 65,
                ..query()
            },
            HubHandlingQueryDto {
                offset: 1,
                ..query()
            },
            HubHandlingQueryDto {
                expected_catalog_revision: None,
                ..pinned.clone()
            },
            HubHandlingQueryDto {
                expected_stock_revision: None,
                ..pinned
            },
            HubHandlingQueryDto {
                text: "x".repeat(121),
                ..query()
            },
            HubHandlingQueryDto {
                trace_id: Some("invalid".into()),
                ..query()
            },
        ] {
            assert!(invalid.decode().is_err());
        }
    }

    #[test]
    fn new_work_canonicalizes_scope_but_rejects_duplicates_and_oversize() {
        let first = "01".repeat(16);
        let second = "02".repeat(16);
        let input = HubNewWorkInputDto {
            expected_stock_revision: 1,
            expected_catalog_revision: 5,
            group_ids: vec![second.clone(), first.clone()],
            target_location_id: "03".repeat(16),
            observation: "Explicit new handling".into(),
        };
        let HandlingCommand::NewWork(command) = (HubHandlingInputDto::NewWork {
            input: input.clone(),
        })
        .command()
        .unwrap() else {
            panic!("expected new work");
        };
        assert_eq!(
            command.groups,
            vec![id(&first).unwrap(), id(&second).unwrap()]
        );
        for group_ids in [
            vec![],
            vec![first.clone(), first.clone()],
            vec![first; HUB_REQUIREMENT_GROUPS + 1],
        ] {
            assert!(
                (HubHandlingInputDto::NewWork {
                    input: HubNewWorkInputDto {
                        group_ids,
                        ..input.clone()
                    }
                })
                .command()
                .is_err()
            );
        }
    }

    #[test]
    fn movement_transport_preserves_unknown_time_and_requires_bounded_values() {
        let input = HubMovementInputDto {
            expected_stock_revision: 1,
            expected_catalog_revision: 5,
            group_id: "01".repeat(16),
            expected_group_revision: 1,
            to_location_id: "02".repeat(16),
            whole_packs: 8,
            separated_portion: Some(HubPhysicalIdentificationDto {
                label: "Blue stack".into(),
                observation: "Physically tagged".into(),
            }),
            occurrence: HubOccurrenceDto {
                kind: crate::HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            observation: "Actual arrival".into(),
        };
        let HandlingCommand::Move(command) = (HubHandlingInputDto::Move {
            input: input.clone(),
        })
        .command()
        .unwrap() else {
            panic!("expected movement");
        };
        assert_eq!(
            command.occurred,
            harvestcircle_domain::hub::stock::Occurrence::Unknown
        );
        for invalid in [
            HubMovementInputDto {
                whole_packs: 0,
                ..input.clone()
            },
            HubMovementInputDto {
                whole_packs: 1_000_001,
                ..input.clone()
            },
            HubMovementInputDto {
                expected_group_revision: 0,
                ..input.clone()
            },
            HubMovementInputDto {
                observation: String::new(),
                ..input.clone()
            },
            HubMovementInputDto {
                occurrence: HubOccurrenceDto {
                    start_unix_s: Some(10),
                    ..input.occurrence.clone()
                },
                ..input
            },
        ] {
            assert!(
                (HubHandlingInputDto::Move { input: invalid })
                    .command()
                    .is_err()
            );
        }
    }
}
