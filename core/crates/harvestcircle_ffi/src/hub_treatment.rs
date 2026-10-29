//! Bounded native treatment projections. Readiness never grants future mutation.
use crate::hub_treatment_input::{id, revision};
use crate::{
    commands::HarvestCircleError,
    hub_handling::HubHandlingReceiptDto,
    hub_reference::HubHandlingBasisDto,
    hub_stock::{HubOccurrenceDto, HubStockPopulationDto},
};
use harvestcircle_domain::hub::{
    portions::CapturedPhysicalScope,
    treatment::{
        AcceptanceDecision, AcceptanceStatus, Hold, PhysicalAction, TreatmentChange,
        TreatmentStamp, TreatmentSummary,
    },
};
use harvestcircle_storage::{
    HubHoldRow, HubTreatmentPage, HubTreatmentQuery, HubTreatmentResult, HubTreatmentScope,
    HubTreatmentSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubAcceptanceStatusDto {
    ReviewRequired,
    Accepted,
    Rejected,
}
impl From<HubAcceptanceStatusDto> for AcceptanceStatus {
    fn from(value: HubAcceptanceStatusDto) -> Self {
        match value {
            HubAcceptanceStatusDto::ReviewRequired => Self::ReviewRequired,
            HubAcceptanceStatusDto::Accepted => Self::Accepted,
            HubAcceptanceStatusDto::Rejected => Self::Rejected,
        }
    }
}
impl From<AcceptanceStatus> for HubAcceptanceStatusDto {
    fn from(value: AcceptanceStatus) -> Self {
        match value {
            AcceptanceStatus::ReviewRequired => Self::ReviewRequired,
            AcceptanceStatus::Accepted => Self::Accepted,
            AcceptanceStatus::Rejected => Self::Rejected,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentScopeDto {
    pub trace_id: String,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
}
impl HubTreatmentScopeDto {
    pub fn decode(self) -> Result<HubTreatmentScope, HarvestCircleError> {
        Ok(HubTreatmentScope {
            trace: id(&self.trace_id)?,
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentQueryDto {
    pub trace_id: String,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
}
impl HubTreatmentQueryDto {
    pub fn decode(self) -> Result<HubTreatmentQuery, HarvestCircleError> {
        let query = HubTreatmentQuery {
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

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubPhysicalScopeAnchorDto {
    pub group_id: String,
    pub created_stock_revision: u64,
    pub captured_whole_packs: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCapturedPhysicalScopeDto {
    pub hub_id: String,
    pub trace_id: String,
    pub variant_id: String,
    pub captured_stock_revision: u64,
    pub anchors: Vec<HubPhysicalScopeAnchorDto>,
}
impl From<&CapturedPhysicalScope> for HubCapturedPhysicalScopeDto {
    fn from(value: &CapturedPhysicalScope) -> Self {
        Self {
            hub_id: harvestcircle_domain::hub::reference::HubRecordId::from_bytes(
                *value.hub().as_bytes(),
            )
            .expect("nonzero hub ID")
            .to_hex(),
            trace_id: value.trace().to_hex(),
            variant_id: value.variant().to_hex(),
            captured_stock_revision: value.captured_revision().value(),
            anchors: value
                .anchors()
                .iter()
                .map(|anchor| HubPhysicalScopeAnchorDto {
                    group_id: anchor.group.to_hex(),
                    created_stock_revision: anchor.created_revision.value(),
                    captured_whole_packs: anchor.captured_packs.value(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentStampDto {
    pub id: String,
    pub stock_revision: u64,
    pub actor_public_key: String,
    pub basis: HubHandlingBasisDto,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl From<&TreatmentStamp> for HubTreatmentStampDto {
    fn from(value: &TreatmentStamp) -> Self {
        Self {
            id: value.id.to_hex(),
            stock_revision: value.stock_revision.value(),
            actor_public_key: value.actor.to_hex(),
            basis: value.basis.into(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            occurrence: value.occurred.into(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubAcceptanceDecisionDto {
    pub stamp: HubTreatmentStampDto,
    pub scope: HubCapturedPhysicalScopeDto,
    pub status: HubAcceptanceStatusDto,
}
impl From<&AcceptanceDecision> for HubAcceptanceDecisionDto {
    fn from(value: &AcceptanceDecision) -> Self {
        Self {
            stamp: (&value.stamp).into(),
            scope: (&value.scope).into(),
            status: value.status.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHoldDto {
    pub stamp: HubTreatmentStampDto,
    pub scope: HubCapturedPhysicalScopeDto,
    pub revision: u64,
    pub release: Option<HubTreatmentStampDto>,
}
impl From<&Hold> for HubHoldDto {
    fn from(value: &Hold) -> Self {
        Self {
            stamp: (&value.stamp).into(),
            scope: (&value.scope).into(),
            revision: value.revision().value(),
            release: value.release.as_ref().map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubTreatmentChangeDto {
    Acceptance {
        value: Box<HubAcceptanceDecisionDto>,
    },
    HoldPlaced {
        value: Box<HubHoldDto>,
    },
    HoldReleased {
        hold_id: String,
        release: Box<HubTreatmentStampDto>,
    },
}
impl From<TreatmentChange> for HubTreatmentChangeDto {
    fn from(value: TreatmentChange) -> Self {
        match value {
            TreatmentChange::Acceptance(value) => Self::Acceptance {
                value: Box::new(value.as_ref().into()),
            },
            TreatmentChange::HoldPlaced(value) => Self::HoldPlaced {
                value: Box::new(value.as_ref().into()),
            },
            TreatmentChange::HoldReleased { hold, release } => Self::HoldReleased {
                hold_id: hold.to_hex(),
                release: Box::new(release.as_ref().into()),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentSummaryDto {
    pub on_hand: u32,
    pub accepted: u32,
    pub review_required: u32,
    pub rejected: u32,
    pub restricted: u32,
    pub departure_eligible: u32,
}
impl From<TreatmentSummary> for HubTreatmentSummaryDto {
    fn from(value: TreatmentSummary) -> Self {
        Self {
            on_hand: value.on_hand.value(),
            accepted: value.accepted.value(),
            review_required: value.review_required.value(),
            rejected: value.rejected.value(),
            restricted: value.restricted.value(),
            departure_eligible: value.departure_eligible.value(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentResultDto {
    pub change: HubTreatmentChangeDto,
    pub summary: HubTreatmentSummaryDto,
    pub affected_whole_packs: u32,
    pub history_capacity_reached: bool,
}
impl From<HubTreatmentResult> for HubTreatmentResultDto {
    fn from(value: HubTreatmentResult) -> Self {
        Self {
            change: value.change.into(),
            summary: value.summary.into(),
            affected_whole_packs: value.affected_packs.value(),
            history_capacity_reached: value.history_capacity_reached,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActionEligibilityDto {
    pub internal_relocation: bool,
    pub observed_separation: bool,
    pub normal_departure: bool,
    pub disposition: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubGroupTreatmentDto {
    pub group_id: String,
    pub whole_packs: u32,
    pub acceptance: HubAcceptanceStatusDto,
    pub acceptance_decision_id: Option<String>,
    pub active_hold_ids: Vec<String>,
    pub actions: HubActionEligibilityDto,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHoldRowDto {
    pub hold: HubHoldDto,
    pub created_operation_id: String,
    pub released_operation_id: Option<String>,
    pub current_whole_packs: u32,
}
impl From<HubHoldRow> for HubHoldRowDto {
    fn from(value: HubHoldRow) -> Self {
        Self {
            hold: (&value.hold).into(),
            created_operation_id: value.created_operation.as_str().to_owned(),
            released_operation_id: value
                .released_operation
                .map(|operation| operation.as_str().to_owned()),
            current_whole_packs: value.current_packs.value(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentSnapshotDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub trace: HubStockPopulationDto,
    pub basis: HubHandlingBasisDto,
    pub summary: HubTreatmentSummaryDto,
    pub history_capacity_reached: bool,
    pub normal_operations_available: bool,
    pub groups: Vec<HubGroupTreatmentDto>,
    pub holds: Vec<HubHoldRowDto>,
}
impl From<HubTreatmentSnapshot> for HubTreatmentSnapshotDto {
    fn from(value: HubTreatmentSnapshot) -> Self {
        let projection = value.projection;
        let groups = projection
            .groups
            .iter()
            .map(|group| {
                let ready = |action| {
                    projection.action_ready(group.group, action, value.normal_operations_available)
                };
                HubGroupTreatmentDto {
                    group_id: group.group.to_hex(),
                    whole_packs: group.packs.value(),
                    acceptance: group.acceptance.into(),
                    acceptance_decision_id: group.acceptance_decision.map(|id| id.to_hex()),
                    active_hold_ids: group.active_holds.iter().map(|id| id.to_hex()).collect(),
                    actions: HubActionEligibilityDto {
                        internal_relocation: ready(PhysicalAction::InternalRelocation),
                        observed_separation: ready(PhysicalAction::ObservedSeparation),
                        normal_departure: ready(PhysicalAction::NormalDeparture),
                        disposition: ready(PhysicalAction::Disposition),
                    },
                }
            })
            .collect();
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            trace: (&value.trace).into(),
            basis: projection.basis.into(),
            summary: projection.summary.into(),
            history_capacity_reached: projection.history_capacity_reached,
            normal_operations_available: value.normal_operations_available,
            groups,
            holds: value.holds.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTreatmentPageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub basis: HubHandlingBasisDto,
    pub rows: Vec<HubHandlingReceiptDto>,
}
impl From<HubTreatmentPage> for HubTreatmentPageDto {
    fn from(value: HubTreatmentPage) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            basis: value.basis.into(),
            rows: value.rows.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treatment_history_requires_bounded_pinned_continuations() {
        let query = HubTreatmentQueryDto {
            trace_id: "01".repeat(16),
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
        };
        assert!(query.clone().decode().is_ok());
        let pinned = HubTreatmentQueryDto {
            offset: 64,
            expected_stock_revision: Some(3),
            expected_catalog_revision: Some(6),
            ..query.clone()
        };
        assert!(pinned.clone().decode().is_ok());
        for invalid in [
            HubTreatmentQueryDto {
                limit: 0,
                ..query.clone()
            },
            HubTreatmentQueryDto {
                limit: 65,
                ..query.clone()
            },
            HubTreatmentQueryDto {
                offset: 1,
                ..query.clone()
            },
            HubTreatmentQueryDto {
                offset: 513,
                ..pinned.clone()
            },
            HubTreatmentQueryDto {
                expected_catalog_revision: None,
                ..pinned
            },
            HubTreatmentQueryDto {
                trace_id: "bad".into(),
                ..query
            },
        ] {
            assert!(invalid.decode().is_err());
        }
    }
}
