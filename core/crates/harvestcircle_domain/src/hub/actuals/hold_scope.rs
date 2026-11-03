//! Original physical birth evidence, never arrival timestamps or recording order.
//! Native adapters must resolve each canonical immutable birth operation. No
//! request decoder or current authority is supplied by these historical facts.
use super::*;
use crate::hub::movement::HandlingIdentities;
use crate::hub::portions::{HUB_LINEAGE_GROUPS, PhysicalLineage, PhysicalScopeAnchor};
use crate::hub::stock::{OpeningIdentities, ReceiptIdentities};
use crate::hub::treatment::{Hold, TreatmentState};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActualPhysicalBirthKind {
    Receipt,
    CountedOpening,
    Returned,
    CountedSurplus,
    ObservedPortion,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualPhysicalBirth {
    pub kind: ActualPhysicalBirthKind,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub group: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub stock_revision: HubRevision,
    pub packs: WholePacks,
    pub actor: PublicKey,
    pub occurred: Occurrence,
    pub recorded_at: UnixTimestamp,
}
impl ActualPhysicalBirth {
    pub fn validate(
        &self,
        graph: &PhysicalLineage,
        now: UnixTimestamp,
    ) -> Result<(), ActualReportError> {
        let invalid = || ActualReportError::InvalidNativeContext;
        let (group, population) = match self.kind {
            ActualPhysicalBirthKind::Receipt => {
                let ids = ReceiptIdentities::from_operation_bytes(self.operation)
                    .map_err(|_| invalid())?;
                (ids.group, Some(ids.trace))
            }
            ActualPhysicalBirthKind::CountedOpening => {
                let ids = OpeningIdentities::from_operation_bytes(self.operation)
                    .map_err(|_| invalid())?;
                (ids.group, Some(ids.population))
            }
            ActualPhysicalBirthKind::Returned => {
                let ids =
                    crate::hub::returns::ReturnIdentities::from_operation_bytes(self.operation)
                        .map_err(|_| invalid())?;
                (ids.group, Some(ids.population))
            }
            ActualPhysicalBirthKind::CountedSurplus => {
                let ids =
                    crate::hub::counts::CountSurplusIdentities::from_operation(self.operation)
                        .map_err(|_| invalid())?;
                (ids.group, Some(ids.population))
            }
            ActualPhysicalBirthKind::ObservedPortion => {
                let ids = HandlingIdentities::from_operation_bytes(self.operation)
                    .map_err(|_| invalid())?;
                (ids.group, None)
            }
        };
        let node = graph.node(self.group).ok_or_else(invalid)?;
        if self.group != group
            || self.hub != graph.hub()
            || self.trace != graph.trace()
            || self.variant != graph.variant()
            || population.is_some_and(|v| v != self.trace)
            || self.parent != node.parent
            || (self.kind == ActualPhysicalBirthKind::ObservedPortion) != self.parent.is_some()
            || self.stock_revision != node.created_revision
            || self.stock_revision == HubRevision::INITIAL
            || self.packs.value() == 0
            || self.recorded_at > now
        {
            return Err(invalid());
        }
        self.occurred
            .validate(self.recorded_at)
            .map_err(|_| invalid())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalHoldScopeUncertainty {
    ScopeNotEstablished,
    MissingBirth,
    UnknownBirthTime,
    GroupNotPresentAtDeparture,
    AnchorNotPresentAtEffectiveTime,
    UncertainPhysicalAncestry,
    ExpandedPhysicalScope,
    AnchorCohortUnverified,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct VerifiedHistoricalHoldScope {
    // Facts at one supplied native cut, never current permission. Original
    // recovery must retain that cut, not rehydrate from later stock quantities.
    original: Hold,
    group: HubRecordId,
    occurred: Occurrence,
    lineage_revision: HubRevision,
    timing: HistoricalHoldTiming,
    /// Original anchor, inherited branch and every detachment used to establish
    /// its whole cohort at the effective time, including depleted siblings.
    births: Vec<ActualPhysicalBirth>,
}
impl VerifiedHistoricalHoldScope {
    #[must_use]
    pub const fn original(&self) -> &Hold {
        &self.original
    }
    #[must_use]
    pub const fn group(&self) -> HubRecordId {
        self.group
    }
    #[must_use]
    pub const fn occurred(&self) -> Occurrence {
        self.occurred
    }
    #[must_use]
    pub const fn lineage_revision(&self) -> HubRevision {
        self.lineage_revision
    }
    #[must_use]
    pub const fn timing(&self) -> HistoricalHoldTiming {
        self.timing
    }
    #[must_use]
    pub fn births(&self) -> &[ActualPhysicalBirth] {
        &self.births
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalHoldScopeAssessment {
    Unresolved {
        original: Hold,
        conditional_timing: HistoricalHoldTiming,
        reason: HistoricalHoldScopeUncertainty,
    },
    Verified(VerifiedHistoricalHoldScope),
}

pub struct HistoricalHoldScopeContext<'a> {
    pub lineage: &'a PhysicalLineage,
    pub group: HubRecordId,
    pub occurred: Occurrence,
    /// Native canonical births, including every relevant original anchor and
    /// physical branch. An arrival must never be passed as a birth substitute.
    pub births: &'a [ActualPhysicalBirth],
    pub now: UnixTimestamp,
}
impl HistoricalHoldScopeContext<'_> {
    pub fn verify(&self, hold: &Hold) -> Result<HistoricalHoldScopeAssessment, ActualReportError> {
        let graph = self.lineage;
        self.occurred
            .validate(self.now)
            .map_err(|_| ActualReportError::InvalidInput)?;
        graph
            .node(self.group)
            .ok_or(ActualReportError::InvalidNativeContext)?;
        TreatmentState::restore(graph, Vec::new(), vec![hold.clone()])
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        if hold.stamp.recorded_at > self.now
            || hold
                .release
                .as_ref()
                .is_some_and(|r| r.recorded_at > self.now)
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        let timing = classify_hold_timing(
            self.occurred,
            hold.stamp.occurred,
            hold.release.as_ref().map(|r| r.occurred),
        );
        let unresolved = |reason| HistoricalHoldScopeAssessment::Unresolved {
            original: hold.clone(),
            conditional_timing: timing,
            reason,
        };
        if self.births.len() > HUB_LINEAGE_GROUPS {
            return Err(ActualReportError::Capacity);
        }
        let mut births = BTreeMap::new();
        let mut operations = BTreeSet::new();
        for birth in self.births {
            birth.validate(graph, self.now)?;
            if births.insert(birth.group, birth).is_some() || !operations.insert(birth.operation) {
                return Err(ActualReportError::InvalidNativeContext);
            }
        }
        let Some((depart_min, _)) = bounds(self.occurred) else {
            return Ok(unresolved(
                HistoricalHoldScopeUncertainty::GroupNotPresentAtDeparture,
            ));
        };
        let mut reason = HistoricalHoldScopeUncertainty::ScopeNotEstablished;
        for anchor in hold.scope.anchors() {
            let single = graph
                .restore_scope(hold.scope.captured_revision(), vec![*anchor])
                .map_err(|_| ActualReportError::InvalidNativeContext)?;
            if !captured_member(graph, anchor.group, single.captured_revision(), self.group) {
                continue;
            }
            if graph
                .resolve(&single)
                .map_err(|_| ActualReportError::InvalidNativeContext)?
                .packs()
                .value()
                > anchor.captured_packs.value()
            {
                reason = HistoricalHoldScopeUncertainty::ExpandedPhysicalScope;
                continue;
            }
            let mut path = vec![self.group];
            while *path.last().expect("nonempty path") != anchor.group {
                let Some(parent) = graph
                    .node(*path.last().expect("path"))
                    .and_then(|n| n.parent)
                else {
                    return Err(ActualReportError::InvalidNativeContext);
                };
                path.push(parent);
            }
            path.reverse();
            let mut proofs = Vec::new();
            let mut failure = None;
            let mut previous = None;
            for group in path {
                let Some(birth) = births.get(&group) else {
                    failure = Some(HistoricalHoldScopeUncertainty::MissingBirth);
                    break;
                };
                let Some((earliest, latest)) = bounds(birth.occurred) else {
                    failure = Some(HistoricalHoldScopeUncertainty::UnknownBirthTime);
                    break;
                };
                if group == anchor.group && anchor.captured_packs.value() > birth.packs.value() {
                    failure = Some(HistoricalHoldScopeUncertainty::ExpandedPhysicalScope);
                    break;
                }
                if group == anchor.group
                    && (anchor.captured_packs != birth.packs
                        || graph.nodes().iter().any(|n| {
                            n.parent == Some(group)
                                && n.created_revision.value()
                                    <= hold.scope.captured_revision().value()
                        }))
                {
                    // A remaining cohort is not its original whole birth cohort.
                    // Canonical physical cuts/counts, not birth equality or a
                    // currently depleted branch, must establish that exact scope.
                    failure = Some(HistoricalHoldScopeUncertainty::AnchorCohortUnverified);
                    break;
                }
                if latest > depart_min {
                    failure = Some(HistoricalHoldScopeUncertainty::GroupNotPresentAtDeparture);
                    break;
                }
                if group == anchor.group
                    && bounds(hold.stamp.occurred).is_some_and(|(at, _)| latest > at)
                {
                    failure = Some(HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime);
                    break;
                }
                if previous.is_some_and(|at| earliest < at) {
                    failure = Some(HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry);
                    break;
                }
                // Native creation order can record a physical split late. A
                // parent capture before that entry does not establish that an
                // already separated child was physically included in the hold.
                if group != anchor.group
                    && !bounds(hold.stamp.occurred).is_some_and(|(_, latest)| latest <= earliest)
                {
                    failure = Some(HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry);
                    break;
                }
                if let Err(reason) = birth_subtree(graph, birth) {
                    failure = Some(reason);
                    break;
                }
                previous = Some(latest);
                proofs.push((*birth).clone());
            }
            if let Some(failed) = failure {
                reason = failed;
                continue;
            }
            let detachments = match cohort_detachments(
                graph,
                anchor.group,
                hold.scope.captured_revision(),
                hold.stamp.occurred,
                &births,
            ) {
                Ok(proofs) => proofs,
                Err(failed) => {
                    reason = failed;
                    continue;
                }
            };
            proofs.extend(detachments);
            proofs.sort_by_key(|b| (b.stock_revision.value(), b.group));
            proofs.dedup_by_key(|b| b.group);
            return Ok(HistoricalHoldScopeAssessment::Verified(
                VerifiedHistoricalHoldScope {
                    original: hold.clone(),
                    group: self.group,
                    occurred: self.occurred,
                    lineage_revision: graph.revision(),
                    timing,
                    births: proofs,
                },
            ));
        }
        Ok(unresolved(reason))
    }
}

fn bounds(value: Occurrence) -> Option<(UnixTimestamp, UnixTimestamp)> {
    match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    }
}

// Historical membership retains zero-pack nodes. A branch that already existed
// when its ancestor was captured is detached, even if it later has descendants.
fn captured_member(
    graph: &PhysicalLineage,
    anchor: HubRecordId,
    captured: HubRevision,
    mut group: HubRecordId,
) -> bool {
    loop {
        if group == anchor {
            return true;
        }
        let Some(node) = graph.node(group) else {
            return false;
        };
        if node.created_revision.value() <= captured.value() {
            return false;
        }
        let Some(parent) = node.parent else {
            return false;
        };
        group = parent;
    }
}

fn cohort_detachments(
    graph: &PhysicalLineage,
    anchor: HubRecordId,
    captured: HubRevision,
    effective: Occurrence,
    births: &BTreeMap<HubRecordId, &ActualPhysicalBirth>,
) -> Result<Vec<ActualPhysicalBirth>, HistoricalHoldScopeUncertainty> {
    let mut proofs = Vec::new();
    for node in graph.nodes() {
        if node.id == anchor || !captured_member(graph, anchor, captured, node.id) {
            continue;
        }
        let birth = births
            .get(&node.id)
            .ok_or(HistoricalHoldScopeUncertainty::MissingBirth)?;
        let (earliest, _) =
            bounds(birth.occurred).ok_or(HistoricalHoldScopeUncertainty::UnknownBirthTime)?;
        if !bounds(effective).is_some_and(|(_, latest)| latest <= earliest) {
            return Err(HistoricalHoldScopeUncertainty::AnchorCohortUnverified);
        }
        if let Some(parent) = node.parent {
            let parent = births
                .get(&parent)
                .ok_or(HistoricalHoldScopeUncertainty::MissingBirth)?;
            let (_, latest) =
                bounds(parent.occurred).ok_or(HistoricalHoldScopeUncertainty::UnknownBirthTime)?;
            if earliest < latest {
                return Err(HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry);
            }
        }
        birth_subtree(graph, birth)?;
        proofs.push((**birth).clone());
    }
    Ok(proofs)
}

fn birth_subtree(
    graph: &PhysicalLineage,
    birth: &ActualPhysicalBirth,
) -> Result<(), HistoricalHoldScopeUncertainty> {
    let node = graph
        .node(birth.group)
        .ok_or(HistoricalHoldScopeUncertainty::MissingBirth)?;
    let original = graph
        .restore_scope(
            birth.stock_revision,
            vec![PhysicalScopeAnchor {
                group: birth.group,
                created_revision: node.created_revision,
                captured_packs: birth.packs,
            }],
        )
        .map_err(|_| HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry)?;
    if graph
        .resolve(&original)
        .map_err(|_| HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry)?
        .packs()
        .value()
        > birth.packs.value()
    {
        return Err(HistoricalHoldScopeUncertainty::ExpandedPhysicalScope);
    }
    Ok(())
}
