//! Bounded count transport; current activity, uncertainty and effects remain native.
use crate::commands::HarvestCircleError;
use crate::hub_handling::{
    HubPhysicalGroupDto, HubPhysicalIdentificationDto, HubWorkProgressDto, HubWorkRequirementDto,
};
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::HubOccurrenceDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_domain::hub::{
    counts::{
        CountBlockReason, CountDifference, CountEffect, CountObservation, CountObserveCommand,
        CountPostCommand,
    },
    reference::{SourceProvenance, WholePacks},
};
use harvestcircle_storage::{HubCountObservationReceipt, HubCountPostReceipt, HubCountReview};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountObserveInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_id: String,
    pub expected_group_revision: u64,
    pub counted_whole_packs: u32,
    pub cutoff: HubOccurrenceDto,
    pub observation: String,
}
impl HubCountObserveInputDto {
    pub fn command(self) -> Result<CountObserveCommand, HarvestCircleError> {
        Ok(CountObserveCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            group: id(&self.group_id)?,
            expected_group_revision: revision(self.expected_group_revision)?,
            counted_packs: WholePacks::from_value(self.counted_whole_packs)
                .map_err(|_| invalid_input())?,
            cutoff: self.cutoff.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountPostInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub count_id: String,
    pub reviewed_activity: bool,
    pub surplus_identification: Option<HubPhysicalIdentificationDto>,
    pub reason: String,
}
impl HubCountPostInputDto {
    pub fn command(self) -> Result<CountPostCommand, HarvestCircleError> {
        Ok(CountPostCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            count: id(&self.count_id)?,
            reviewed_activity: self.reviewed_activity,
            surplus_identification: self
                .surplus_identification
                .map(HubPhysicalIdentificationDto::decode)
                .transpose()?,
            reason: SourceProvenance::new(self.reason).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountObservationDto {
    pub id: String,
    pub operation_id: String,
    pub recorded_stock_revision: u64,
    pub hub_id: String,
    pub captured_stock_revision: u64,
    pub captured_catalog_revision: u64,
    pub population_id: String,
    pub group_id: String,
    pub group_revision: u64,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub location: HubReferenceRecordDto,
    pub recorded_whole_packs: u32,
    pub counted_whole_packs: u32,
    pub latest_physical_recorded_at_unix_s: i64,
    pub previous_reconciliation_id: Option<String>,
    pub cutoff: HubOccurrenceDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub observation: String,
}
impl From<CountObservation> for HubCountObservationDto {
    fn from(value: CountObservation) -> Self {
        let scope = value.scope;
        Self {
            id: value.id.to_hex(),
            operation_id: operation_text(value.operation),
            recorded_stock_revision: value.recorded_revision.value(),
            hub_id: scope
                .hub
                .as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect(),
            captured_stock_revision: scope.stock_revision.value(),
            captured_catalog_revision: scope.catalog_revision.value(),
            population_id: scope.trace.to_hex(),
            group_id: scope.group.to_hex(),
            group_revision: scope.group_revision.value(),
            product: (&scope.product).into(),
            variant: (&scope.variant).into(),
            location: (&scope.location).into(),
            recorded_whole_packs: scope.recorded_packs.value(),
            counted_whole_packs: value.counted_packs.value(),
            latest_physical_recorded_at_unix_s: scope.latest_physical_recorded_at.as_seconds(),
            previous_reconciliation_id: scope.previous_reconciliation.map(|v| v.to_hex()),
            cutoff: value.cutoff.into(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}
impl From<HubCountObservationReceipt> for HubCountObservationDto {
    fn from(value: HubCountObservationReceipt) -> Self {
        value.original.into()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubCountDifferenceDto {
    NoVariance,
    Shortage { whole_packs: u32 },
    Surplus { whole_packs: u32 },
}
impl From<CountDifference> for HubCountDifferenceDto {
    fn from(value: CountDifference) -> Self {
        match value {
            CountDifference::None => Self::NoVariance,
            CountDifference::Shortage(packs) => Self::Shortage {
                whole_packs: packs.value(),
            },
            CountDifference::Surplus(packs) => Self::Surplus {
                whole_packs: packs.value(),
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubCountBlockReasonDto {
    ScopeChanged,
    NewerReconciliation,
    NotLatestObservation,
    CutoffUncertain,
    CutoffPrecedesRecordedActivity,
    UnresolvedActual,
    UncertainPriorOutcome,
}
impl From<CountBlockReason> for HubCountBlockReasonDto {
    fn from(value: CountBlockReason) -> Self {
        match value {
            CountBlockReason::ScopeChanged => Self::ScopeChanged,
            CountBlockReason::NewerReconciliation => Self::NewerReconciliation,
            CountBlockReason::NotLatestObservation => Self::NotLatestObservation,
            CountBlockReason::CutoffUncertain => Self::CutoffUncertain,
            CountBlockReason::CutoffPrecedesRecordedActivity => {
                Self::CutoffPrecedesRecordedActivity
            }
            CountBlockReason::UnresolvedActual => Self::UnresolvedActual,
            CountBlockReason::UncertainPriorOutcome => Self::UncertainPriorOutcome,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountReviewDto {
    pub observation: HubCountObservationDto,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub current: HubPhysicalGroupDto,
    pub difference: HubCountDifferenceDto,
    pub blockers: Vec<HubCountBlockReasonDto>,
    pub ready: bool,
}
impl From<HubCountReview> for HubCountReviewDto {
    fn from(value: HubCountReview) -> Self {
        let ready = value.assessment.ready();
        Self {
            observation: value.observation.into(),
            stock_revision: value.stock_revision.value(),
            catalog_revision: value.catalog_revision.value(),
            current: (&value.current).into(),
            difference: value.assessment.difference.into(),
            blockers: value
                .assessment
                .blockers
                .into_iter()
                .map(Into::into)
                .collect(),
            ready,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountWorkChangeDto {
    pub before: HubWorkRequirementDto,
    pub after: HubWorkRequirementDto,
    pub retired_whole_packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountRemovedWorkDto {
    pub work_id: String,
    pub before_progress: HubWorkProgressDto,
    pub before_scoped_whole_packs: u32,
    pub on_hand_after: u32,
    pub removed_whole_packs: u32,
    pub removed_progress: HubWorkProgressDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountPostReceiptDto {
    pub id: String,
    pub operation_id: String,
    pub observation: HubCountObservationDto,
    pub before: HubPhysicalGroupDto,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub location: HubReferenceRecordDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub confirmed_on_hand_at_unix_s: i64,
    pub difference: HubCountDifferenceDto,
    pub on_hand_after: u32,
    pub group_revision_after: u64,
    pub work_changes: Vec<HubCountWorkChangeDto>,
    pub removed_work: Vec<HubCountRemovedWorkDto>,
    pub surplus: Option<crate::hub_opening::HubCountedStockDto>,
    pub reason: String,
}
impl TryFrom<HubCountPostReceipt> for HubCountPostReceiptDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubCountPostReceipt) -> Result<Self, Self::Error> {
        let plan = value.original;
        let removed_work = plan
            .work_scope_changes()
            .map_err(|_| invalid_input())?
            .into_iter()
            .map(|v| HubCountRemovedWorkDto {
                work_id: v.work.to_hex(),
                before_progress: v.before_progress.into(),
                before_scoped_whole_packs: v.before_scoped.value(),
                on_hand_after: v.on_hand_after.value(),
                removed_whole_packs: v.removed.value(),
                removed_progress: v.removed_progress.into(),
            })
            .collect();
        let (on_hand_after, group_revision_after, work_changes) = match &plan.effect {
            CountEffect::Shortage {
                after_packs,
                after_revision,
                work_changes,
                ..
            } => (
                after_packs.value(),
                after_revision.value(),
                work_changes
                    .iter()
                    .map(|v| HubCountWorkChangeDto {
                        before: (&v.before).into(),
                        after: (&v.after).into(),
                        retired_whole_packs: v.retired.value(),
                    })
                    .collect(),
            ),
            _ => (
                plan.before.packs.value(),
                plan.before.revision.value(),
                vec![],
            ),
        };
        Ok(Self {
            id: plan.id.to_hex(),
            operation_id: value.operation.as_str().to_owned(),
            difference: plan.count.difference().map_err(|_| invalid_input())?.into(),
            observation: plan.count.into(),
            before: (&plan.before).into(),
            stock_revision: plan.stock_revision.value(),
            catalog_revision: plan.catalog_revision.value(),
            product: (&plan.references.product).into(),
            variant: (&plan.references.variant).into(),
            location: (&plan.references.location).into(),
            actor_public_key: plan.actor.to_hex(),
            recorded_at_unix_s: plan.recorded_at.as_seconds(),
            confirmed_on_hand_at_unix_s: plan.confirmed_on_hand_at.as_seconds(),
            on_hand_after,
            group_revision_after,
            work_changes,
            removed_work,
            surplus: value
                .surplus
                .map(|v| crate::hub_opening::HubCountedStockDto {
                    population: (&v.population).into(),
                    group: (&v.group).into(),
                    acceptance: v.acceptance.into(),
                }),
            reason: plan.reason.as_str().to_owned(),
        })
    }
}
fn operation_text(value: harvestcircle_domain::hub::reference::HubRecordId) -> String {
    let hex = value.to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
mod tests;
