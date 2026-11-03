use super::*;
use crate::hub::{
    HubRevision,
    portions::{CapturedPhysicalScope, PhysicalLineage},
    reference::{SampleHandlingBasis, SourceProvenance, WholePacks},
    stock::Occurrence,
};
use crate::{PublicKey, UnixTimestamp};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TreatmentStamp {
    pub id: HubRecordId,
    pub stock_revision: HubRevision,
    pub actor: PublicKey,
    pub basis: SampleHandlingBasis,
    pub recorded_at: UnixTimestamp,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AcceptanceDecision {
    pub stamp: TreatmentStamp,
    pub scope: CapturedPhysicalScope,
    pub status: AcceptanceStatus,
}

/// Fixed negative constraint at the birth cut, never an acceptance grant.
/// Ordinary decisions and holds retain their preceding-cut scope rule.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct InitialGroupReview {
    pub stamp: TreatmentStamp,
    pub scope: CapturedPhysicalScope,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Hold {
    pub stamp: TreatmentStamp,
    pub scope: CapturedPhysicalScope,
    pub release: Option<TreatmentStamp>,
}

impl Hold {
    #[must_use]
    pub fn revision(&self) -> HubRevision {
        HubRevision::from_value(if self.release.is_some() { 2 } else { 1 })
            .expect("bounded hold revision")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct TreatmentState {
    pub(super) decisions: Vec<AcceptanceDecision>,
    pub(super) holds: Vec<Hold>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) initial_reviews: Vec<InitialGroupReview>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupTreatment {
    pub group: HubRecordId,
    pub packs: WholePacks,
    pub acceptance: AcceptanceStatus,
    pub acceptance_decision: Option<HubRecordId>,
    pub active_holds: Vec<HubRecordId>,
}

impl GroupTreatment {
    /// Group treatment rule only; projection capacity and native authority also apply.
    #[must_use]
    pub fn allows(&self, basis: SampleHandlingBasis, action: PhysicalAction) -> bool {
        match (basis, action) {
            (SampleHandlingBasis::WHOLE_PACK_V1, PhysicalAction::NormalDeparture) => {
                self.acceptance == AcceptanceStatus::Accepted && self.active_holds.is_empty()
            }
            (
                SampleHandlingBasis::WHOLE_PACK_V1,
                PhysicalAction::InternalRelocation
                | PhysicalAction::ObservedSeparation
                | PhysicalAction::Disposition,
            ) => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreatmentSummary {
    pub on_hand: WholePacks,
    pub accepted: WholePacks,
    pub review_required: WholePacks,
    pub rejected: WholePacks,
    pub restricted: WholePacks,
    pub departure_eligible: WholePacks,
}

impl TreatmentSummary {
    pub fn validate(self) -> Result<(), TreatmentError> {
        if self
            .accepted
            .checked_add(self.review_required)
            .and_then(|v| v.checked_add(self.rejected))
            .map_err(|_| TreatmentError::InvalidHistory)?
            != self.on_hand
            || self.restricted.value() > self.on_hand.value()
            || self.departure_eligible.value() > self.accepted.value()
            || self.departure_eligible.value()
                < self
                    .accepted
                    .value()
                    .saturating_sub(self.restricted.value())
            || self.departure_eligible.value() > self.on_hand.value() - self.restricted.value()
        {
            return Err(TreatmentError::InvalidHistory);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreatmentProjection {
    pub revision: HubRevision,
    pub basis: SampleHandlingBasis,
    pub history_capacity_reached: bool,
    pub summary: TreatmentSummary,
    pub groups: Vec<GroupTreatment>,
}

impl TreatmentProjection {
    /// Readiness at this captured revision; never a future mutation grant.
    #[must_use]
    pub fn action_ready(
        &self,
        group: HubRecordId,
        action: PhysicalAction,
        normal_operation_capacity: bool,
    ) -> bool {
        normal_operation_capacity
            && !(action == PhysicalAction::NormalDeparture && self.history_capacity_reached)
            && self
                .groups
                .iter()
                .find(|candidate| candidate.group == group)
                .is_some_and(|group| group.allows(self.basis, action))
    }
    pub fn require(
        &self,
        selection: &TreatmentSelection,
        action: PhysicalAction,
    ) -> Result<(), TreatmentError> {
        if action == PhysicalAction::NormalDeparture && self.history_capacity_reached {
            return Err(TreatmentError::Capacity);
        }
        for id in selection.groups() {
            let group = self
                .groups
                .iter()
                .find(|group| group.group == *id)
                .ok_or(TreatmentError::InvalidSelection)?;
            if !group.allows(self.basis, action) {
                return Err(TreatmentError::ActionBlocked);
            }
        }
        Ok(())
    }
}

impl TreatmentState {
    pub fn restore(
        graph: &PhysicalLineage,
        decisions: Vec<AcceptanceDecision>,
        holds: Vec<Hold>,
    ) -> Result<Self, TreatmentError> {
        Self::restore_with_initial_reviews(graph, decisions, holds, Vec::new())
    }
    /// Native readers must supply every original birth review at the selected cut.
    /// No accepted status or hold release can be supplied through this constructor.
    pub fn restore_with_initial_reviews(
        graph: &PhysicalLineage,
        decisions: Vec<AcceptanceDecision>,
        holds: Vec<Hold>,
        initial_reviews: Vec<InitialGroupReview>,
    ) -> Result<Self, TreatmentError> {
        let state = Self {
            decisions,
            holds,
            initial_reviews,
        };
        state.validate(graph)?;
        Ok(state)
    }
    #[must_use]
    pub fn decisions(&self) -> &[AcceptanceDecision] {
        &self.decisions
    }
    #[must_use]
    pub fn holds(&self) -> &[Hold] {
        &self.holds
    }
    #[must_use]
    pub fn initial_reviews(&self) -> &[InitialGroupReview] {
        &self.initial_reviews
    }
    /// Primary decisions, birth reviews and holds. Each hold retains one bounded release slot.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.decisions.len() + self.holds.len() + self.initial_reviews.len()
    }

    fn validate(&self, graph: &PhysicalLineage) -> Result<(), TreatmentError> {
        if self.decisions.len() > HUB_TREATMENT_RECORDS_PER_TRACE
            || self.holds.len() > HUB_TREATMENT_RECORDS_PER_TRACE
            || self.initial_reviews.len() > HUB_TREATMENT_RECORDS_PER_TRACE
            || self.record_count() > HUB_TREATMENT_RECORDS_PER_TRACE
        {
            return Err(TreatmentError::Capacity);
        }
        let mut ids = BTreeSet::new();
        let mut revisions = BTreeSet::new();
        let mut stamp = |value: &TreatmentStamp| -> Result<(), TreatmentError> {
            if value.stock_revision.value() < 2
                || value.stock_revision.value() > graph.revision().value()
                || !ids.insert(*value.id.as_bytes())
                || !revisions.insert(value.stock_revision.value())
            {
                return Err(TreatmentError::InvalidHistory);
            }
            value
                .occurred
                .validate(value.recorded_at)
                .map_err(|_| TreatmentError::InvalidOccurrence)
        };
        for decision in &self.decisions {
            stamp(&decision.stamp)?;
            validate_scope(graph, &decision.scope, &decision.stamp)?;
        }
        for hold in &self.holds {
            stamp(&hold.stamp)?;
            validate_scope(graph, &hold.scope, &hold.stamp)?;
            if let Some(release) = &hold.release {
                stamp(release)?;
                if release.stock_revision.value() <= hold.stamp.stock_revision.value() {
                    return Err(TreatmentError::InvalidHistory);
                }
                if occurrence_bounds(hold.stamp.occurred)
                    .map(|(earliest, _)| earliest)
                    .zip(occurrence_bounds(release.occurred).map(|(_, latest)| latest))
                    .is_some_and(|(placed, released)| released < placed)
                {
                    return Err(TreatmentError::InvalidOccurrence);
                }
            }
        }
        for review in &self.initial_reviews {
            stamp(&review.stamp)?;
            if review.scope.captured_revision() != review.stamp.stock_revision
                || review.scope.anchors().len() > HUB_TREATMENT_SELECTION_GROUPS
                || review.stamp.occurred != Occurrence::Known(review.stamp.recorded_at)
            {
                return Err(TreatmentError::InvalidHistory);
            }
            let mut captured = WholePacks::from_value(0).expect("zero packs");
            for anchor in review.scope.anchors() {
                let node = graph
                    .node(anchor.group)
                    .ok_or(TreatmentError::InvalidHistory)?;
                if anchor.created_revision != review.stamp.stock_revision
                    || node.created_revision != review.stamp.stock_revision
                    || node.parent.is_none()
                    || anchor.captured_packs.value() == 0
                {
                    return Err(TreatmentError::InvalidHistory);
                }
                let single =
                    graph.restore_scope(review.scope.captured_revision(), vec![*anchor])?;
                if graph.resolve(&single)?.packs().value() > anchor.captured_packs.value() {
                    return Err(TreatmentError::InvalidHistory);
                }
                captured = captured
                    .checked_add(anchor.captured_packs)
                    .map_err(|_| TreatmentError::InvalidHistory)?;
            }
            if graph.resolve(&review.scope)?.packs().value() > captured.value() {
                return Err(TreatmentError::InvalidHistory);
            }
        }
        Ok(())
    }

    pub fn project(
        &self,
        graph: &PhysicalLineage,
        basis: SampleHandlingBasis,
    ) -> Result<TreatmentProjection, TreatmentError> {
        self.validate(graph)?;
        let mut acceptance: BTreeMap<[u8; 16], (&TreatmentStamp, AcceptanceStatus)> =
            BTreeMap::new();
        for decision in &self.decisions {
            for node in graph.resolve(&decision.scope)?.groups() {
                let slot = acceptance.entry(*node.id.as_bytes());
                match slot {
                    std::collections::btree_map::Entry::Occupied(mut entry)
                        if entry.get().0.stock_revision.value()
                            < decision.stamp.stock_revision.value() =>
                    {
                        entry.insert((&decision.stamp, decision.status));
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert((&decision.stamp, decision.status));
                    }
                    _ => {}
                }
            }
        }
        for review in &self.initial_reviews {
            for node in graph.resolve(&review.scope)?.groups() {
                let slot = acceptance.entry(*node.id.as_bytes());
                match slot {
                    std::collections::btree_map::Entry::Occupied(mut entry)
                        if entry.get().0.stock_revision.value()
                            < review.stamp.stock_revision.value() =>
                    {
                        entry.insert((&review.stamp, AcceptanceStatus::ReviewRequired));
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert((&review.stamp, AcceptanceStatus::ReviewRequired));
                    }
                    _ => {}
                }
            }
        }
        let mut restrictions: BTreeMap<[u8; 16], Vec<HubRecordId>> = BTreeMap::new();
        for hold in self.holds.iter().filter(|hold| hold.release.is_none()) {
            for node in graph.resolve(&hold.scope)?.groups() {
                restrictions
                    .entry(*node.id.as_bytes())
                    .or_default()
                    .push(hold.stamp.id);
            }
        }
        let zero = WholePacks::from_value(0).expect("zero packs");
        let mut summary = TreatmentSummary {
            on_hand: zero,
            accepted: zero,
            review_required: zero,
            rejected: zero,
            restricted: zero,
            departure_eligible: zero,
        };
        let mut groups = Vec::new();
        for node in graph.nodes().iter().filter(|node| node.packs.value() > 0) {
            let decision = acceptance.get(node.id.as_bytes());
            let mut holds = restrictions.remove(node.id.as_bytes()).unwrap_or_default();
            holds.sort_by_key(|id| *id.as_bytes());
            let group = GroupTreatment {
                group: node.id,
                packs: node.packs,
                acceptance: decision
                    .map_or(AcceptanceStatus::ReviewRequired, |(_, status)| *status),
                acceptance_decision: decision.map(|(stamp, _)| stamp.id),
                active_holds: holds,
            };
            let add = |total: &mut WholePacks| -> Result<(), TreatmentError> {
                *total = total
                    .checked_add(node.packs)
                    .map_err(|_| TreatmentError::InvalidHistory)?;
                Ok(())
            };
            add(&mut summary.on_hand)?;
            match group.acceptance {
                AcceptanceStatus::Accepted => add(&mut summary.accepted)?,
                AcceptanceStatus::ReviewRequired => add(&mut summary.review_required)?,
                AcceptanceStatus::Rejected => add(&mut summary.rejected)?,
            }
            if !group.active_holds.is_empty() {
                add(&mut summary.restricted)?;
            }
            if group.allows(basis, PhysicalAction::NormalDeparture) {
                add(&mut summary.departure_eligible)?;
            }
            groups.push(group);
        }
        summary.validate()?;
        Ok(TreatmentProjection {
            revision: graph.revision(),
            basis,
            history_capacity_reached: self.record_count() == HUB_TREATMENT_RECORDS_PER_TRACE,
            summary,
            groups,
        })
    }
}

fn validate_scope(
    graph: &PhysicalLineage,
    scope: &CapturedPhysicalScope,
    stamp: &TreatmentStamp,
) -> Result<(), TreatmentError> {
    if scope.anchors().len() > HUB_TREATMENT_SELECTION_GROUPS
        || scope.captured_revision().value().checked_add(1) != Some(stamp.stock_revision.value())
    {
        return Err(TreatmentError::InvalidHistory);
    }
    graph.resolve(scope)?;
    Ok(())
}

fn occurrence_bounds(value: Occurrence) -> Option<(UnixTimestamp, UnixTimestamp)> {
    match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    }
}
