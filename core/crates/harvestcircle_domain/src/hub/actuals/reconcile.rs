//! Closed actual-event policy. Native current admission and atomic persistence
//! remain mandatory; neither plans nor historical results are execution grants.
use super::*;
use crate::hub::departure::DepartureWorkChange;
use crate::hub::movement::{PhysicalStockGroup, WorkProgress, WorkRequirement};
use crate::hub::reference::{ReferenceKind, ReferenceRecord};
use crate::hub::stock::{HUB_STOCK_EVENT_CAPACITY, QuantityWork};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualRemovedWorkScope {
    pub work: HubRecordId,
    pub before_progress: WorkProgress,
    pub before_scoped: WholePacks,
    pub on_hand_after: WholePacks,
    pub removed: WholePacks,
    pub removed_progress: WorkProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualPhysicalReduction {
    pub before: PhysicalStockGroup,
    pub actual_departed: WholePacks,
    /// Zero retains an empty historical position, not a fake physical group.
    pub on_hand_after: WholePacks,
    pub after_revision: HubRevision,
    pub work_changes: Vec<DepartureWorkChange>,
    pub removed_work_scope: Vec<ActualRemovedWorkScope>,
}
impl ActualPhysicalReduction {
    /// Already-taken facts may precede a later recorded arrival. Reconciliation
    /// never lowers the remaining cohort's native floor or permits earlier new
    /// planned handling. Unknown occurrence supplies no invented clock.
    pub fn causal_floor_after_reconciliation(
        &self,
        occurred: Occurrence,
        now: UnixTimestamp,
    ) -> Result<Option<UnixTimestamp>, ActualReportError> {
        self.validate()?;
        if self.before.causal_floor.is_some_and(|floor| floor > now) {
            return Err(ActualReportError::InvalidNativeContext);
        }
        occurred
            .validate(now)
            .map_err(|_| ActualReportError::InvalidInput)?;
        let observed = match occurred {
            Occurrence::Known(at) => Some(at),
            Occurrence::Interval { earliest, .. } => Some(earliest),
            Occurrence::Unknown => None,
        };
        Ok(self.before.causal_floor.max(observed))
    }

    pub fn plan(before: &PhysicalStockGroup, packs: WholePacks) -> Result<Self, ActualReportError> {
        before
            .validate()
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        if packs.value() == 0 || packs.value() > before.packs.value() {
            return Err(ActualReportError::InsufficientBalance);
        }
        let remaining = before
            .packs
            .checked_sub(packs)
            .map_err(|_| ActualReportError::InsufficientBalance)?;
        let mut work_changes = Vec::new();
        let mut removed_work_scope = Vec::new();
        for allocation in &before.work {
            if allocation.progress == WorkProgress::Pending {
                let work = &allocation.requirement;
                let quantity = QuantityWork::restore(
                    work.quantity.required(),
                    work.quantity.completed(),
                    work.quantity
                        .no_longer_required()
                        .checked_add(packs)
                        .map_err(|_| ActualReportError::InvalidHistory)?,
                    work.quantity
                        .remaining()
                        .checked_sub(packs)
                        .map_err(|_| ActualReportError::InvalidHistory)?,
                )
                .map_err(|_| ActualReportError::InvalidHistory)?;
                let after = WorkRequirement {
                    quantity,
                    revision: work
                        .revision
                        .next()
                        .map_err(|_| ActualReportError::RevisionExhausted)?,
                    ..work.clone()
                };
                after
                    .validate()
                    .map_err(|_| ActualReportError::InvalidHistory)?;
                work_changes.push(DepartureWorkChange {
                    before: work.clone(),
                    after,
                    retired: packs,
                });
            }
            removed_work_scope.push(ActualRemovedWorkScope {
                work: allocation.requirement.id,
                before_progress: allocation.progress,
                before_scoped: before.packs,
                on_hand_after: remaining,
                removed: packs,
                removed_progress: match allocation.progress {
                    WorkProgress::Pending => WorkProgress::NoLongerRequired,
                    p => p,
                },
            });
        }
        Ok(Self {
            before: before.clone(),
            actual_departed: packs,
            on_hand_after: remaining,
            after_revision: before
                .revision
                .next()
                .map_err(|_| ActualReportError::RevisionExhausted)?,
            work_changes,
            removed_work_scope,
        })
    }
    /// Internal retained-result consistency, not current balance/authority.
    pub fn validate(&self) -> Result<(), ActualReportError> {
        if *self != Self::plan(&self.before, self.actual_departed)? {
            return Err(ActualReportError::InvalidHistory);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualKnownDepartureLine {
    pub group: HubRecordId,
    pub location: ReferenceRecord,
    pub packs: WholePacks,
}

/// Native immutable original, aggregated per operation/population/variant.
/// Adapters must load every original line, not a client-picked subtotal.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualKnownDeparture {
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub trace: HubRecordId,
    pub variant: ReferenceRecord,
    pub recipient: ReferenceRecord,
    pub occurred: Occurrence,
    pub recorded_at: UnixTimestamp,
    pub lines: Vec<ActualKnownDepartureLine>,
}
impl ActualKnownDeparture {
    pub fn packs(&self) -> Result<WholePacks, ActualReportError> {
        self.lines
            .iter()
            .try_fold(WholePacks::from_value(0).expect("zero"), |n, l| {
                n.checked_add(l.packs)
                    .map_err(|_| ActualReportError::InvalidHistory)
            })
    }
    pub fn validate(&self) -> Result<(), ActualReportError> {
        if self.lines.is_empty() || self.lines.len() > crate::hub::dispatch::HUB_DISPATCH_LINES {
            return Err(ActualReportError::Capacity);
        }
        self.occurred
            .validate(self.recorded_at)
            .map_err(|_| ActualReportError::InvalidHistory)?;
        if self.variant.value.kind() != ReferenceKind::Pack
            || self.recipient.value.kind() != ReferenceKind::Party
            || self.variant.revision.value() > self.catalog_revision.value()
            || self.recipient.revision.value() > self.catalog_revision.value()
            || self.stock_revision == HubRevision::INITIAL
        {
            return Err(ActualReportError::InvalidHistory);
        }
        let mut groups = BTreeSet::new();
        for line in &self.lines {
            if line.packs.value() == 0
                || !groups.insert(line.group)
                || line.location.value.kind() != ReferenceKind::Location
                || line.location.revision.value() > self.catalog_revision.value()
            {
                return Err(ActualReportError::InvalidHistory);
            }
        }
        self.packs()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualNewDepartureCommand {
    pub facts: ActualReviewFacts,
    pub source_operation: HubRecordId,
    pub source_group: HubRecordId,
    pub duplicate_review_basis: SourceProvenance,
}
pub struct ActualNewDepartureContext<'a> {
    pub review: ActualReviewContext<'a>,
    pub historical_source: ActualSourceContext<'a>,
    /// Complete native previous departures, including reconciled actual events.
    /// Missing/unknown originals may not be replaced by a client empty list.
    pub known_departures: &'a [ActualKnownDeparture],
    pub prior_review: Option<HubRecordId>,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualNewDeparturePlan {
    pub operation: HubRecordId,
    pub report: ActualReportRecord,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub stock_revision: HubRevision,
    pub comparison: ActualReviewComparison,
    pub historical_source: ResolvedActualSource,
    pub reduction: ActualPhysicalReduction,
    pub after_causal_floor: Option<UnixTimestamp>,
    pub duplicate_review_basis: SourceProvenance,
}
impl ActualNewDeparturePlan {
    /// Scoped physical facts from this result, not a validated full original or
    /// permission. Native persistence first verifies the complete plan/context.
    pub fn physical_projection(&self) -> Result<ActualKnownDeparture, ActualReportError> {
        self.report.validate()?;
        self.reduction.validate()?;
        let before = &self.reduction.before;
        let scope = &self.comparison.scope;
        let source = self.historical_source.historical();
        if self.comparison.original != self.report
            || scope.before != *before
            || scope.compared_departure != self.reduction.actual_departed
            || scope.on_hand_after != self.reduction.on_hand_after
            || self.comparison.facts.group != before.id
            || self.comparison.facts.packs != self.reduction.actual_departed
            || scope.variant.id != before.variant
            || scope.recipient.id != self.comparison.facts.recipient
            || self.historical_source.balancing_group() != before.id
            || source.group.trace != before.trace
            || source.group.variant != before.variant
            || source.hub != self.report.hub
        {
            return Err(ActualReportError::InvalidHistory);
        }
        let projection = ActualKnownDeparture {
            operation: self.operation,
            actor: self.actor,
            hub: self.report.hub,
            stock_revision: self.stock_revision,
            catalog_revision: self.comparison.facts.expected_catalog_revision,
            trace: before.trace,
            variant: scope.variant.clone(),
            recipient: scope.recipient.clone(),
            occurred: self.comparison.facts.occurred,
            recorded_at: self.recorded_at,
            lines: vec![ActualKnownDepartureLine {
                group: before.id,
                location: source.location.clone(),
                packs: self.reduction.actual_departed,
            }],
        };
        projection.validate()?;
        Ok(projection)
    }
}
impl ActualNewDepartureCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.actual.review.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        let facts = &self.facts;
        for id in [
            facts.report,
            facts.group,
            facts.recipient,
            self.source_operation,
            self.source_group,
        ] {
            hash.update(id.as_bytes());
        }
        for revision in [
            facts.expected_stock_revision,
            facts.expected_catalog_revision,
            facts.expected_group_revision,
        ] {
            hash.update(revision.value().to_be_bytes());
        }
        hash.update(facts.packs.value().to_be_bytes());
        hash_occurrence(&mut hash, facts.occurred);
        for text in [
            &facts.source_evidence,
            &facts.quantity_evidence,
            &facts.recipient_evidence,
            &facts.timing_evidence,
            &self.duplicate_review_basis,
        ] {
            hash_text(&mut hash, text.as_str());
        }
        hash.finalize().into()
    }
    pub fn plan(
        &self,
        context: &ActualNewDepartureContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualNewDeparturePlan, ActualReportError> {
        self.plan_inner(context, None, operation, actor, now)
    }

    /// Requires a complete one-to-one interpretation of the supplied native
    /// originals. An empty array cannot hide a nonempty known-event baseline.
    pub fn plan_with_interpretations(
        &self,
        context: &ActualNewDepartureContext<'_>,
        interpretations: &ActualKnownInterpretations,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualNewDeparturePlan, ActualReportError> {
        self.plan_inner(context, Some(interpretations), operation, actor, now)
    }

    fn plan_inner(
        &self,
        context: &ActualNewDepartureContext<'_>,
        interpretations: Option<&ActualKnownInterpretations>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualNewDeparturePlan, ActualReportError> {
        if context.prior_review.is_some() {
            return Err(ActualReportError::AlreadyReviewed);
        }
        if operation == context.review.original.operation || operation == self.source_operation {
            return Err(ActualReportError::InvalidIdentity);
        }
        if context.historical_source.lineage != context.review.lineage
            || context.historical_source.balancing_group != context.review.group
            || context.historical_source.source.operation != self.source_operation
            || context.historical_source.source.group.id != self.source_group
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        let comparison = self.facts.review(&context.review, now)?;
        let source =
            context
                .historical_source
                .resolve(context.review.original, self.facts.occurred, now)?;
        let duplicates = context.known_departures;
        let views = interpretations
            .map(|set| super::interpretation::validate_set(context, set, operation, now))
            .transpose()?;
        if duplicates.len() > HUB_STOCK_EVENT_CAPACITY {
            return Err(ActualReportError::Capacity);
        }
        let mut originals = BTreeSet::new();
        for (index, known) in duplicates.iter().enumerate() {
            known.validate()?;
            if known.operation == operation {
                return Err(ActualReportError::InvalidIdentity);
            }
            if known.hub != context.review.hub
                || known.stock_revision.value() > context.review.stock_revision.value()
                || known.catalog_revision.value() > context.review.catalog.revision().value()
                || known.recorded_at > now
                || !originals.insert((known.operation, known.trace, known.variant.id))
            {
                return Err(ActualReportError::InvalidNativeContext);
            }
            if known.trace == source.historical().group.trace
                && known.variant.id == context.review.group.variant
                && views
                    .as_ref()
                    .map_or(known.recipient.id == self.facts.recipient, |views| {
                        views[index]
                            .recipients
                            .could_match(Some(self.facts.recipient))
                    })
                && overlaps(known.occurred, self.facts.occurred)
            {
                // Explicit typed linking is required; a reason/checkbox or new
                // operation identity cannot make a possible duplicate disappear.
                return Err(ActualReportError::DuplicateReviewRequired);
            }
        }
        let reduction = ActualPhysicalReduction::plan(context.review.group, self.facts.packs)?;
        let after_causal_floor =
            reduction.causal_floor_after_reconciliation(self.facts.occurred, now)?;
        Ok(ActualNewDeparturePlan {
            operation,
            report: context.review.original.clone(),
            actor,
            recorded_at: now,
            stock_revision: context
                .review
                .stock_revision
                .next()
                .map_err(|_| ActualReportError::RevisionExhausted)?,
            comparison,
            historical_source: source,
            reduction,
            after_causal_floor,
            duplicate_review_basis: self.duplicate_review_basis.clone(),
        })
    }
}

fn overlaps(a: Occurrence, b: Occurrence) -> bool {
    let bounds = |v| match v {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    };
    match (bounds(a), bounds(b)) {
        (Some((a, b)), Some((c, d))) => a <= d && c <= b,
        _ => true,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualDuplicateLink {
    pub report: HubRecordId,
    pub known_operation: HubRecordId,
    pub known_trace: HubRecordId,
    pub known_variant: HubRecordId,
    pub verification: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualDuplicateLinkPlan {
    pub operation: HubRecordId,
    pub original: ActualReportRecord,
    pub known: ActualKnownDeparture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<Box<ActualKnownInterpretation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_source: Option<Box<CountExplanationSourceWitness>>,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub verification: SourceProvenance,
}
impl ActualDuplicateLink {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.actual.duplicate.link.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        hash.update(self.report.as_bytes());
        hash.update(self.known_operation.as_bytes());
        hash.update(self.known_trace.as_bytes());
        hash.update(self.known_variant.as_bytes());
        hash_text(&mut hash, self.verification.as_str());
        hash.finalize().into()
    }
    /// Links a preserved duplicate to its immutable existing physical event;
    /// no current positive stock group or another reduction is required.
    pub fn plan(
        &self,
        original: &ActualReportRecord,
        known: &ActualKnownDeparture,
        prior_review: Option<HubRecordId>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualDuplicateLinkPlan, ActualReportError> {
        self.plan_inner(
            DuplicateContext {
                original,
                known,
                interpretation: None,
                count_source: None,
            },
            prior_review,
            operation,
            actor,
            now,
        )
    }

    /// An explicit link retains the immutable event and its independently
    /// captured interpretation; neither a matching total nor Unknown is a grant.
    pub fn plan_with_interpretation(
        &self,
        original: &ActualReportRecord,
        interpretation: &ActualKnownInterpretation,
        prior_review: Option<HubRecordId>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualDuplicateLinkPlan, ActualReportError> {
        self.plan_inner(
            DuplicateContext {
                original,
                known: &interpretation.original,
                interpretation: Some(interpretation),
                count_source: None,
            },
            prior_review,
            operation,
            actor,
            now,
        )
    }

    /// Count-specific original source corroboration. This descriptive plan is
    /// still subject to complete native original/admission/mode validation.
    pub fn plan_with_count_interpretation(
        &self,
        original: &ActualReportRecord,
        context: ActualCountDuplicateContext<'_>,
        prior_review: Option<HubRecordId>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualDuplicateLinkPlan, ActualReportError> {
        self.plan_inner(
            DuplicateContext {
                original,
                known: &context.interpretation.original,
                interpretation: Some(context.interpretation),
                count_source: Some(context.source),
            },
            prior_review,
            operation,
            actor,
            now,
        )
    }

    fn plan_inner(
        &self,
        context: DuplicateContext<'_>,
        prior_review: Option<HubRecordId>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ActualDuplicateLinkPlan, ActualReportError> {
        let DuplicateContext {
            original,
            known,
            interpretation,
            count_source,
        } = context;
        original.validate()?;
        known.validate()?;
        if prior_review.is_some() {
            return Err(ActualReportError::AlreadyReviewed);
        }
        if operation == original.operation || operation == known.operation {
            return Err(ActualReportError::InvalidIdentity);
        }
        if self.report != original.id
            || self.known_operation != known.operation
            || self.known_trace != known.trace
            || self.known_variant != known.variant.id
            || original.hub != known.hub
            || original.recorded_at > now
            || known.recorded_at > now
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        let view = interpretation
            .map(|record| {
                if record.stock_revision.value() < original.stock_revision.value()
                    || record.catalog_revision.value() < original.catalog_revision.value()
                    || record.latest_recorded_at < original.recorded_at
                    || record.latest_recorded_at > now
                {
                    return Err(ActualReportError::InvalidNativeContext);
                }
                let view = record.view()?;
                for (id, revision) in record.operations() {
                    if id == operation || id == original.operation {
                        return Err(ActualReportError::InvalidIdentity);
                    }
                    if revision == original.stock_revision {
                        return Err(ActualReportError::InvalidHistory);
                    }
                }
                Ok(view)
            })
            .transpose()?;
        match (
            interpretation.and_then(|record| record.count_explanation.as_deref()),
            count_source,
        ) {
            (Some(count), Some(source)) => {
                source.validate_for(known, &count.count)?;
                let source = source.historical();
                if source.operation == operation || source.operation == original.operation {
                    return Err(ActualReportError::InvalidIdentity);
                }
                if source.stock_revision == original.stock_revision {
                    return Err(ActualReportError::InvalidHistory);
                }
                if let Some(record) = interpretation {
                    for (id, revision) in record.operations() {
                        if id == source.operation || revision == source.stock_revision {
                            return Err(ActualReportError::InvalidHistory);
                        }
                    }
                }
            }
            (None, None) => {}
            _ => return Err(ActualReportError::InvalidNativeContext),
        }
        let claim = &original.report;
        if claim.variant.is_some_and(|v| v != known.variant.id)
            || !view.as_ref().map_or(
                claim.recipient.is_none_or(|v| v == known.recipient.id),
                |view| view.recipients.could_match(claim.recipient),
            )
            || !view.as_ref().map_or(
                claim.packs.is_none_or(|v| Some(v) == known.packs().ok()),
                |view| view.could_match_packs(claim.packs),
            )
            || !overlaps(claim.occurred, known.occurred)
        {
            return Err(ActualReportError::ConflictingEvidence);
        }
        let source_matches = |s: &ReportedActualSource| {
            s.trace == known.trace
                && known.lines.iter().any(|l| {
                    s.group.is_none_or(|g| g == l.group)
                        && s.location.is_none_or(|v| v == l.location.id)
                })
        };
        if !matches!(&claim.sources, ActualReportSources::Unknown)
            && !claim.sources.candidates().iter().any(|claim| {
                source_matches(claim)
                    || count_source.is_some_and(|source| source.matches_source(claim))
            })
        {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        Ok(ActualDuplicateLinkPlan {
            operation,
            original: original.clone(),
            known: known.clone(),
            interpretation: interpretation.cloned().map(Box::new),
            count_source: count_source.cloned().map(Box::new),
            actor,
            recorded_at: now,
            verification: self.verification.clone(),
        })
    }
}

struct DuplicateContext<'a> {
    original: &'a ActualReportRecord,
    known: &'a ActualKnownDeparture,
    interpretation: Option<&'a ActualKnownInterpretation>,
    count_source: Option<&'a CountExplanationSourceWitness>,
}
