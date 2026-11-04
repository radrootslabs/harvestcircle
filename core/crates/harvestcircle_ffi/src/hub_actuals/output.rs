//! Original reports and native review results remain distinct across the boundary.
use super::input::HubActualReportInputDto;
use crate::{
    commands::HarvestCircleError,
    hub_counts::operation_text,
    hub_reference::{HubReferenceRecordDto, invalid_input},
    hub_stock::HubOccurrenceDto,
    hub_trace::HubTraceActualDepartureDto,
    hub_treatment::HubHoldDto,
};
use harvestcircle_domain::hub::actuals::{
    ActualDuplicateLinkPlan, ActualKnownDeparture, ActualReportRecord,
    ActualReportedClockAssessment, HistoricalHoldScopeAssessment, HistoricalHoldScopeUncertainty,
    HistoricalHoldTiming,
};
use harvestcircle_storage::{
    HubActualDeparturePreview, HubActualLinkPreview, HubActualReportPage, HubActualReportReceipt,
    HubActualReportResolutionKind, HubActualReviewOriginal, HubActualReviewReceipt,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualClockAssessmentDto {
    Unknown,
    NotAfterRecordedTime,
    RecordingClockConflict,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReportDto {
    pub id: String,
    pub operation_id: String,
    pub hub_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub clock_assessment: HubActualClockAssessmentDto,
    pub reported: HubActualReportInputDto,
}
impl From<&ActualReportRecord> for HubActualReportDto {
    fn from(value: &ActualReportRecord) -> Self {
        Self {
            id: value.id.to_hex(),
            operation_id: operation_text(value.operation),
            hub_id: value
                .hub
                .as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect(),
            stock_revision: value.stock_revision.value(),
            catalog_revision: value.catalog_revision.value(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            clock_assessment: match value.recording_clock_assessment() {
                ActualReportedClockAssessment::Unknown => HubActualClockAssessmentDto::Unknown,
                ActualReportedClockAssessment::NotAfterRecordedTime => {
                    HubActualClockAssessmentDto::NotAfterRecordedTime
                }
                ActualReportedClockAssessment::RecordingClockConflict => {
                    HubActualClockAssessmentDto::RecordingClockConflict
                }
            },
            reported: (&value.report).into(),
        }
    }
}
impl From<HubActualReportReceipt> for HubActualReportDto {
    fn from(value: HubActualReportReceipt) -> Self {
        (&value.original).into()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReportRowDto {
    pub report: HubActualReportDto,
    pub review_operation_id: Option<String>,
    pub resolution_kind: Option<HubActualReportResolutionKindDto>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualReportResolutionKindDto {
    ActualReview,
    AdditionalQuantity,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReportPageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubActualReportRowDto>,
}
impl From<HubActualReportPage> for HubActualReportPageDto {
    fn from(value: HubActualReportPage) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value
                .rows
                .into_iter()
                .map(|row| HubActualReportRowDto {
                    report: row.report.into(),
                    review_operation_id: row.review_operation.map(|v| v.as_str().to_owned()),
                    resolution_kind: row.resolution_kind.map(|kind| match kind {
                        HubActualReportResolutionKind::ActualReview => {
                            HubActualReportResolutionKindDto::ActualReview
                        }
                        HubActualReportResolutionKind::AdditionalQuantity => {
                            HubActualReportResolutionKindDto::AdditionalQuantity
                        }
                    }),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualHoldTimingDto {
    EstablishedBreach,
    BeforeHold,
    AfterRelease,
    TimingUnknown,
}
impl From<HistoricalHoldTiming> for HubActualHoldTimingDto {
    fn from(value: HistoricalHoldTiming) -> Self {
        match value {
            HistoricalHoldTiming::EstablishedBreach => Self::EstablishedBreach,
            HistoricalHoldTiming::BeforeHold => Self::BeforeHold,
            HistoricalHoldTiming::AfterRelease => Self::AfterRelease,
            HistoricalHoldTiming::TimingUnknown => Self::TimingUnknown,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualHoldUncertaintyDto {
    ScopeNotEstablished,
    MissingBirth,
    UnknownBirthTime,
    GroupNotPresentAtDeparture,
    AnchorNotPresentAtEffectiveTime,
    UncertainPhysicalAncestry,
    ExpandedPhysicalScope,
    AnchorCohortUnverified,
}
impl From<HistoricalHoldScopeUncertainty> for HubActualHoldUncertaintyDto {
    fn from(value: HistoricalHoldScopeUncertainty) -> Self {
        match value {
            HistoricalHoldScopeUncertainty::ScopeNotEstablished => Self::ScopeNotEstablished,
            HistoricalHoldScopeUncertainty::MissingBirth => Self::MissingBirth,
            HistoricalHoldScopeUncertainty::UnknownBirthTime => Self::UnknownBirthTime,
            HistoricalHoldScopeUncertainty::GroupNotPresentAtDeparture => {
                Self::GroupNotPresentAtDeparture
            }
            HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime => {
                Self::AnchorNotPresentAtEffectiveTime
            }
            HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry => {
                Self::UncertainPhysicalAncestry
            }
            HistoricalHoldScopeUncertainty::ExpandedPhysicalScope => Self::ExpandedPhysicalScope,
            HistoricalHoldScopeUncertainty::AnchorCohortUnverified => Self::AnchorCohortUnverified,
        }
    }
}
/// Conditional clock comparisons never share a variant with verified scope.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualHoldAssessmentDto {
    Unresolved {
        original: HubHoldDto,
        conditional_timing: HubActualHoldTimingDto,
        reason: HubActualHoldUncertaintyDto,
    },
    Verified {
        original: HubHoldDto,
        group_id: String,
        occurrence: HubOccurrenceDto,
        lineage_revision: u64,
        timing: HubActualHoldTimingDto,
    },
}
impl From<&HistoricalHoldScopeAssessment> for HubActualHoldAssessmentDto {
    fn from(value: &HistoricalHoldScopeAssessment) -> Self {
        match value {
            HistoricalHoldScopeAssessment::Unresolved {
                original,
                conditional_timing,
                reason,
            } => Self::Unresolved {
                original: original.into(),
                conditional_timing: (*conditional_timing).into(),
                reason: (*reason).into(),
            },
            HistoricalHoldScopeAssessment::Verified(value) => Self::Verified {
                original: value.original().into(),
                group_id: value.group().to_hex(),
                occurrence: value.occurred().into(),
                lineage_revision: value.lineage_revision().value(),
                timing: value.timing().into(),
            },
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualKnownLineDto {
    pub group_id: String,
    pub location: HubReferenceRecordDto,
    pub whole_packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualKnownDepartureDto {
    pub operation_id: String,
    pub actor_public_key: String,
    pub hub_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub trace_id: String,
    pub variant: HubReferenceRecordDto,
    pub recipient: HubReferenceRecordDto,
    pub occurrence: HubOccurrenceDto,
    pub recorded_at_unix_s: i64,
    pub whole_packs: u32,
    pub lines: Vec<HubActualKnownLineDto>,
}
impl TryFrom<&ActualKnownDeparture> for HubActualKnownDepartureDto {
    type Error = HarvestCircleError;
    fn try_from(value: &ActualKnownDeparture) -> Result<Self, Self::Error> {
        value.validate().map_err(|_| invalid_input())?;
        Ok(Self {
            operation_id: operation_text(value.operation),
            actor_public_key: value.actor.to_hex(),
            hub_id: value
                .hub
                .as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect(),
            stock_revision: value.stock_revision.value(),
            catalog_revision: value.catalog_revision.value(),
            trace_id: value.trace.to_hex(),
            variant: (&value.variant).into(),
            recipient: (&value.recipient).into(),
            occurrence: value.occurred.into(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            whole_packs: value.packs().map_err(|_| invalid_input())?.value(),
            lines: value
                .lines
                .iter()
                .map(|line| HubActualKnownLineDto {
                    group_id: line.group.to_hex(),
                    location: (&line.location).into(),
                    whole_packs: line.packs.value(),
                })
                .collect(),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualDuplicateDto {
    pub operation_id: String,
    pub report: HubActualReportDto,
    pub known: HubActualKnownDepartureDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub verification: String,
}
impl TryFrom<&ActualDuplicateLinkPlan> for HubActualDuplicateDto {
    type Error = HarvestCircleError;
    fn try_from(value: &ActualDuplicateLinkPlan) -> Result<Self, Self::Error> {
        Ok(Self {
            operation_id: operation_text(value.operation),
            report: (&value.original).into(),
            known: (&value.known).try_into()?,
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            verification: value.verification.as_str().to_owned(),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualDeparturePreviewDto {
    pub report: HubActualReportDto,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub proposed: HubTraceActualDepartureDto,
    pub historical_holds: Vec<HubActualHoldAssessmentDto>,
}
impl From<HubActualDeparturePreview> for HubActualDeparturePreviewDto {
    fn from(value: HubActualDeparturePreview) -> Self {
        Self {
            report: (&value.plan.report).into(),
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            proposed: (&value.plan).into(),
            historical_holds: value.historical_holds.iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualLinkPreviewDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub proposed: HubActualDuplicateDto,
}
impl TryFrom<HubActualLinkPreview> for HubActualLinkPreviewDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubActualLinkPreview) -> Result<Self, Self::Error> {
        Ok(Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            proposed: (&value.plan).try_into()?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualReviewOriginalDto {
    Departure {
        report: Box<HubActualReportDto>,
        original: Box<HubTraceActualDepartureDto>,
        historical_holds: Vec<HubActualHoldAssessmentDto>,
    },
    Duplicate {
        original: Box<HubActualDuplicateDto>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReviewReceiptDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub original: HubActualReviewOriginalDto,
}
impl TryFrom<HubActualReviewReceipt> for HubActualReviewReceiptDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubActualReviewReceipt) -> Result<Self, Self::Error> {
        let original = match value.original {
            HubActualReviewOriginal::Departure(plan) => HubActualReviewOriginalDto::Departure {
                report: Box::new((&plan.report).into()),
                original: Box::new((&*plan).into()),
                historical_holds: value.historical_holds.iter().map(Into::into).collect(),
            },
            HubActualReviewOriginal::Duplicate(plan) => HubActualReviewOriginalDto::Duplicate {
                original: Box::new((&*plan).try_into()?),
            },
        };
        Ok(Self {
            operation_id: value.operation.as_str().to_owned(),
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            original,
        })
    }
}
