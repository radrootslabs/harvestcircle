//! Read-only original-backed trace and safe local-document transport.
use crate::commands::HarvestCircleError;
use crate::hub_handling::HubPhysicalIdentificationDto;
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::{HubOccurrenceDto, HubStockPopulationDto};
use crate::hub_treatment::HubAcceptanceStatusDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_domain::hub::trace::{TraceScope, TraceSelection, TraceSnapshot};
use harvestcircle_runtime::HubLocalOutputRequest;
use harvestcircle_storage::{
    HubDispatchKind, HubHandlingKind, HubTraceHistoryKind, HubTraceHistoryPage,
    HubTraceHistoryQuery, HubTracePage, HubTraceQuery, HubTraceSnapshotQuery,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubTraceScopeDto {
    All,
    Unresolved,
    Receipt,
    SourceImpact,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceSelectionDto {
    pub scope: HubTraceScopeDto,
    pub receipt_id: Option<String>,
    pub source_id: Option<String>,
    pub text: String,
}
impl HubTraceSelectionDto {
    fn decode(self) -> Result<TraceSelection, HarvestCircleError> {
        let scope = match (self.scope, self.receipt_id, self.source_id) {
            (HubTraceScopeDto::All, None, None) => TraceScope::All,
            (HubTraceScopeDto::Unresolved, None, None) => TraceScope::Unresolved,
            (HubTraceScopeDto::Receipt, Some(receipt), None) => TraceScope::Receipt(id(&receipt)?),
            (HubTraceScopeDto::SourceImpact, None, Some(source)) => {
                TraceScope::SourceImpact(id(&source)?)
            }
            _ => return Err(invalid_input()),
        };
        let selection = TraceSelection {
            scope,
            text: self.text,
        };
        selection.validate().map_err(|_| invalid_input())?;
        Ok(selection)
    }
}
impl From<TraceSelection> for HubTraceSelectionDto {
    fn from(value: TraceSelection) -> Self {
        let (scope, receipt_id, source_id) = match value.scope {
            TraceScope::All => (HubTraceScopeDto::All, None, None),
            TraceScope::Unresolved => (HubTraceScopeDto::Unresolved, None, None),
            TraceScope::Receipt(id) => (HubTraceScopeDto::Receipt, Some(id.to_hex()), None),
            TraceScope::SourceImpact(id) => {
                (HubTraceScopeDto::SourceImpact, None, Some(id.to_hex()))
            }
        };
        Self {
            scope,
            receipt_id,
            source_id,
            text: value.text,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceQueryDto {
    pub selection: HubTraceSelectionDto,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
}
impl HubTraceQueryDto {
    pub fn decode(self) -> Result<HubTraceQuery, HarvestCircleError> {
        let query = HubTraceQuery {
            selection: self.selection.decode()?,
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceRowDto {
    pub trace: HubStockPopulationDto,
    pub on_hand_whole_packs: u32,
    pub departed_whole_packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTracePageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub selection: HubTraceSelectionDto,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubTraceRowDto>,
}
impl From<HubTracePage> for HubTracePageDto {
    fn from(value: HubTracePage) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            selection: value.selection.into(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value
                .rows
                .into_iter()
                .map(|row| HubTraceRowDto {
                    trace: (&row.trace).into(),
                    on_hand_whole_packs: row.balance.on_hand.value(),
                    departed_whole_packs: row.balance.departed.value(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceSnapshotQueryDto {
    pub trace_id: String,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
}
impl HubTraceSnapshotQueryDto {
    pub fn decode(self) -> Result<HubTraceSnapshotQuery, HarvestCircleError> {
        let query = HubTraceSnapshotQuery {
            trace: id(&self.trace_id)?,
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTracePositionDto {
    pub group_id: String,
    pub group_revision: u64,
    pub whole_packs: u32,
    pub location: HubReferenceRecordDto,
    pub identification: Option<HubPhysicalIdentificationDto>,
    pub acceptance: HubAcceptanceStatusDto,
    pub acceptance_decision_id: Option<String>,
    pub active_hold_ids: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceExitDto {
    pub group_id: String,
    pub whole_packs: u32,
    pub departure_id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub preparation_id: String,
    pub identification_id: String,
    pub recipient: HubReferenceRecordDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceSnapshotDto {
    pub hub_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub trace: HubStockPopulationDto,
    pub on_hand_whole_packs: u32,
    pub departed_whole_packs: u32,
    pub on_hand: Vec<HubTracePositionDto>,
    pub exits: Vec<HubTraceExitDto>,
}
impl TryFrom<TraceSnapshot> for HubTraceSnapshotDto {
    type Error = HarvestCircleError;
    fn try_from(value: TraceSnapshot) -> Result<Self, Self::Error> {
        let balance = value.balance().map_err(|_| invalid_input())?;
        Ok(Self {
            hub_id: value
                .hub
                .as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect(),
            stock_revision: value.stock_revision.value(),
            catalog_revision: value.catalog_revision.value(),
            trace: (&value.trace).into(),
            on_hand_whole_packs: balance.on_hand.value(),
            departed_whole_packs: balance.departed.value(),
            on_hand: value
                .on_hand
                .into_iter()
                .map(|v| HubTracePositionDto {
                    group_id: v.group.to_hex(),
                    group_revision: v.revision.value(),
                    whole_packs: v.packs.value(),
                    location: (&v.location).into(),
                    identification: v.identification.as_ref().map(Into::into),
                    acceptance: v.treatment.acceptance.into(),
                    acceptance_decision_id: v.treatment.acceptance_decision.map(|id| id.to_hex()),
                    active_hold_ids: v
                        .treatment
                        .active_holds
                        .into_iter()
                        .map(|id| id.to_hex())
                        .collect(),
                })
                .collect(),
            exits: value
                .exits
                .into_iter()
                .map(|v| {
                    let d = v.departure;
                    HubTraceExitDto {
                        group_id: v.group.to_hex(),
                        whole_packs: v.packs.value(),
                        departure_id: d.id.to_hex(),
                        operation_id: operation_text(d.operation),
                        stock_revision: d.stock_revision.value(),
                        preparation_id: d.preparation.to_hex(),
                        identification_id: d.identification.to_hex(),
                        recipient: (&d.recipient).into(),
                        actor_public_key: d.actor.to_hex(),
                        recorded_at_unix_s: d.recorded_at.as_seconds(),
                        occurrence: d.occurred.into(),
                        observation: d.observation.as_str().to_owned(),
                    }
                })
                .collect(),
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
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceHistoryQueryDto {
    pub trace_id: String,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
}
impl HubTraceHistoryQueryDto {
    pub fn decode(self) -> Result<HubTraceHistoryQuery, HarvestCircleError> {
        let query = HubTraceHistoryQuery {
            trace: id(&self.trace_id)?,
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubTraceHistoryKindDto {
    Receipt,
    CountedOpening,
    CountObserved,
    Move,
    Split,
    Acceptance,
    PlaceHold,
    ReleaseHold,
    Replan,
    NewWork,
    OpenDiscrepancy,
    ResolveDiscrepancy,
    PrepareIdentification,
    ReviseIdentification,
    IssueIdentification,
    ReplaceIdentification,
    MarkIdentificationUnused,
    Departure,
}
impl From<HubTraceHistoryKind> for HubTraceHistoryKindDto {
    fn from(value: HubTraceHistoryKind) -> Self {
        match value {
            HubTraceHistoryKind::Receipt => Self::Receipt,
            HubTraceHistoryKind::CountedOpening => Self::CountedOpening,
            HubTraceHistoryKind::CountObserved => Self::CountObserved,
            HubTraceHistoryKind::Departure => Self::Departure,
            HubTraceHistoryKind::Handling(kind) => match kind {
                HubHandlingKind::Move => Self::Move,
                HubHandlingKind::Split => Self::Split,
                HubHandlingKind::Acceptance => Self::Acceptance,
                HubHandlingKind::PlaceHold => Self::PlaceHold,
                HubHandlingKind::ReleaseHold => Self::ReleaseHold,
                HubHandlingKind::Replan => Self::Replan,
                HubHandlingKind::NewWork => Self::NewWork,
                HubHandlingKind::OpenDiscrepancy => Self::OpenDiscrepancy,
                HubHandlingKind::ResolveDiscrepancy => Self::ResolveDiscrepancy,
            },
            HubTraceHistoryKind::Dispatch(kind) => match kind {
                HubDispatchKind::Prepare => Self::PrepareIdentification,
                HubDispatchKind::Revise => Self::ReviseIdentification,
                HubDispatchKind::Issue => Self::IssueIdentification,
                HubDispatchKind::Replace => Self::ReplaceIdentification,
                HubDispatchKind::MarkUnused => Self::MarkIdentificationUnused,
            },
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceHistoryEntryDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub kind: HubTraceHistoryKindDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: Option<HubOccurrenceDto>,
    pub observation: String,
    pub result_id: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceHistoryPageDto {
    pub trace_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubTraceHistoryEntryDto>,
}
impl From<HubTraceHistoryPage> for HubTraceHistoryPageDto {
    fn from(value: HubTraceHistoryPage) -> Self {
        Self {
            trace_id: value.trace.to_hex(),
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value
                .rows
                .into_iter()
                .map(|v| HubTraceHistoryEntryDto {
                    operation_id: v.operation.as_str().to_owned(),
                    stock_revision: v.revision.value(),
                    kind: v.kind.into(),
                    actor_public_key: v.actor.to_hex(),
                    recorded_at_unix_s: v.recorded_at.as_seconds(),
                    occurrence: v.occurred.map(Into::into),
                    observation: v.observation.as_str().to_owned(),
                    result_id: v.result.to_hex(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubLocalOutputRequestDto {
    Identification {
        preparation_id: String,
        identification_id: String,
    },
    Trace {
        trace_id: String,
        expected_stock_revision: u64,
        expected_catalog_revision: u64,
    },
}
impl HubLocalOutputRequestDto {
    pub fn decode(self) -> Result<HubLocalOutputRequest, HarvestCircleError> {
        let request = match self {
            Self::Identification {
                preparation_id,
                identification_id,
            } => HubLocalOutputRequest::Identification {
                preparation: id(&preparation_id)?,
                identification: id(&identification_id)?,
            },
            Self::Trace {
                trace_id,
                expected_stock_revision,
                expected_catalog_revision,
            } => HubLocalOutputRequest::Trace(HubTraceSnapshotQuery {
                trace: id(&trace_id)?,
                expected_revision: Some(revision(expected_stock_revision)?),
                expected_catalog_revision: Some(revision(expected_catalog_revision)?),
            }),
        };
        request.validate().map_err(HarvestCircleError::from)?;
        Ok(request)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubLocalOutputDto {
    pub identity: String,
    pub suggested_filename: String,
    pub html: String,
}
impl From<harvestcircle_application::hub_output::LocalOutput> for HubLocalOutputDto {
    fn from(value: harvestcircle_application::hub_output::LocalOutput) -> Self {
        Self {
            identity: value.identity,
            suggested_filename: value.suggested_filename,
            html: value.html,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selected() -> HubTraceSelectionDto {
        HubTraceSelectionDto {
            scope: HubTraceScopeDto::All,
            receipt_id: None,
            source_id: None,
            text: String::new(),
        }
    }
    #[test]
    fn hub_trace_selection_rejects_ambiguous_scope_and_narrowed_impact() {
        let id = "01".repeat(16);
        assert!(
            HubTraceSelectionDto {
                source_id: Some(id.clone()),
                ..selected()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceSelectionDto {
                scope: HubTraceScopeDto::SourceImpact,
                source_id: Some(id.clone()),
                receipt_id: Some(id.clone()),
                text: String::new()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceSelectionDto {
                scope: HubTraceScopeDto::SourceImpact,
                source_id: Some(id.clone()),
                text: "narrower".into(),
                ..selected()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceSelectionDto {
                scope: HubTraceScopeDto::Receipt,
                receipt_id: Some("00".repeat(16)),
                ..selected()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceSelectionDto {
                scope: HubTraceScopeDto::SourceImpact,
                source_id: Some(id),
                ..selected()
            }
            .decode()
            .is_ok()
        );
    }
    #[test]
    fn hub_trace_continuations_and_current_output_require_valid_complete_pins() {
        assert!(
            HubTraceQueryDto {
                selection: selected(),
                expected_stock_revision: Some(2),
                expected_catalog_revision: None,
                offset: 1,
                limit: 64
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceHistoryQueryDto {
                trace_id: "01".repeat(16),
                expected_stock_revision: None,
                expected_catalog_revision: None,
                offset: 1,
                limit: 64
            }
            .decode()
            .is_err()
        );
        assert!(
            HubTraceSnapshotQueryDto {
                trace_id: "01".repeat(16),
                expected_stock_revision: None,
                expected_catalog_revision: Some(2)
            }
            .decode()
            .is_err()
        );
        assert!(
            HubLocalOutputRequestDto::Trace {
                trace_id: "01".repeat(16),
                expected_stock_revision: 0,
                expected_catalog_revision: 2
            }
            .decode()
            .is_err()
        );
        assert!(
            HubLocalOutputRequestDto::Identification {
                preparation_id: "01".repeat(16),
                identification_id: "02".repeat(16)
            }
            .decode()
            .is_ok()
        );
    }
}
