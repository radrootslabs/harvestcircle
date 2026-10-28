use super::*;
use crate::hub::movement::{hash_occurrence, hash_text};
use crate::hub::{
    HubId, HubRevision,
    portions::PhysicalLineage,
    reference::{ReferenceCatalog, ReferenceKind, SourceProvenance, WholePacks},
    stock::Occurrence,
};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreatmentAction {
    SetAcceptance {
        selection: TreatmentSelection,
        status: AcceptanceStatus,
    },
    PlaceHold {
        selection: TreatmentSelection,
    },
    ReleaseHold {
        hold: HubRecordId,
        expected_revision: HubRevision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreatmentCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub action: TreatmentAction,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreatmentChange {
    Acceptance(Box<AcceptanceDecision>),
    HoldPlaced(Box<Hold>),
    HoldReleased {
        hold: HubRecordId,
        release: Box<TreatmentStamp>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreatmentPlan {
    pub revision: HubRevision,
    pub change: TreatmentChange,
    pub summary: TreatmentSummary,
    pub history_capacity_reached: bool,
    pub affected_packs: WholePacks,
}

pub fn treatment_record_id(operation: HubRecordId) -> Result<HubRecordId, TreatmentError> {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.treatment.record.v1\0");
    hash.update(operation.as_bytes());
    let digest = hash.finalize();
    HubRecordId::from_bytes(digest[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| TreatmentError::InvalidSelection)
}

impl TreatmentCommand {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.action {
            TreatmentAction::SetAcceptance { .. } => "acceptance",
            TreatmentAction::PlaceHold { .. } => "hold_place",
            TreatmentAction::ReleaseHold { .. } => "hold_release",
        }
    }

    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        graph: &PhysicalLineage,
        state: &TreatmentState,
        record: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<TreatmentPlan, TreatmentError> {
        if self.expected_stock_revision != graph.revision()
            || self.expected_catalog_revision != catalog.revision()
        {
            return Err(TreatmentError::StaleRevision);
        }
        catalog
            .require_kind(graph.variant(), ReferenceKind::Pack)
            .map_err(|_| TreatmentError::InvalidSelection)?;
        state.project(graph, catalog.handling_basis())?;
        if !matches!(self.action, TreatmentAction::ReleaseHold { .. })
            && state.record_count() >= HUB_TREATMENT_RECORDS_PER_TRACE
        {
            return Err(TreatmentError::Capacity);
        }
        self.occurred
            .validate(now)
            .map_err(|_| TreatmentError::InvalidOccurrence)?;
        let revision = graph
            .revision()
            .value()
            .checked_add(1)
            .and_then(|value| HubRevision::from_value(value).ok())
            .ok_or(TreatmentError::RevisionExhausted)?;
        let stamp = TreatmentStamp {
            id: record,
            stock_revision: revision,
            actor,
            basis: catalog.handling_basis(),
            recorded_at: now,
            occurred: self.occurred,
            observation: self.observation.clone(),
        };
        let mut after = state.clone();
        let change = match &self.action {
            TreatmentAction::SetAcceptance { selection, status } => {
                let decision = AcceptanceDecision {
                    stamp,
                    scope: graph.capture(selection.groups())?,
                    status: *status,
                };
                after.decisions.push(decision.clone());
                TreatmentChange::Acceptance(Box::new(decision))
            }
            TreatmentAction::PlaceHold { selection } => {
                let hold = Hold {
                    stamp,
                    scope: graph.capture(selection.groups())?,
                    release: None,
                };
                after.holds.push(hold.clone());
                TreatmentChange::HoldPlaced(Box::new(hold))
            }
            TreatmentAction::ReleaseHold {
                hold,
                expected_revision,
            } => {
                let selected = after
                    .holds
                    .iter_mut()
                    .find(|item| item.stamp.id == *hold)
                    .ok_or(TreatmentError::InvalidSelection)?;
                if selected.revision() != *expected_revision {
                    return Err(TreatmentError::StaleRevision);
                }
                if selected.release.is_some() {
                    return Err(TreatmentError::HoldClosed);
                }
                selected.release = Some(stamp.clone());
                TreatmentChange::HoldReleased {
                    hold: *hold,
                    release: Box::new(stamp),
                }
            }
        };
        let scope = match &change {
            TreatmentChange::Acceptance(value) => &value.scope,
            TreatmentChange::HoldPlaced(value) => &value.scope,
            TreatmentChange::HoldReleased { hold, .. } => {
                &state
                    .holds()
                    .iter()
                    .find(|item| item.stamp.id == *hold)
                    .ok_or(TreatmentError::InvalidHistory)?
                    .scope
            }
        };
        let affected_packs = graph.resolve(scope)?.packs();
        let advanced = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            revision,
            graph.nodes().to_vec(),
        )?;
        let projection = after.project(&advanced, catalog.handling_basis())?;
        Ok(TreatmentPlan {
            revision,
            change,
            summary: projection.summary,
            history_capacity_reached: projection.history_capacity_reached,
            affected_packs,
        })
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, record: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.treatment.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(record.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        let mut selection = |tag: u8, groups: &TreatmentSelection| {
            hash.update([tag]);
            hash.update(
                u32::try_from(groups.groups().len())
                    .expect("bounded selection")
                    .to_be_bytes(),
            );
            for group in groups.groups() {
                hash.update(group.as_bytes());
            }
        };
        match &self.action {
            TreatmentAction::SetAcceptance {
                selection: groups,
                status,
            } => {
                selection(0, groups);
                hash.update([match status {
                    AcceptanceStatus::ReviewRequired => 0,
                    AcceptanceStatus::Accepted => 1,
                    AcceptanceStatus::Rejected => 2,
                }]);
            }
            TreatmentAction::PlaceHold { selection: groups } => selection(1, groups),
            TreatmentAction::ReleaseHold {
                hold,
                expected_revision,
            } => {
                hash.update([2]);
                hash.update(hold.as_bytes());
                hash.update(expected_revision.value().to_be_bytes());
            }
        }
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
