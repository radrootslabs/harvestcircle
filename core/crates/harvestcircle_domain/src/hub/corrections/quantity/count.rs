//! Classification of one exact already-posted shortage; no physical effect API.
use super::*;
use crate::hub::actuals::{ActualKnownDepartureLine, CountedShortageSourceContext};
use crate::hub::counts::{CountEffect, CountPostPlan};

pub fn count_explanation_id(operation: HubRecordId) -> Result<HubRecordId, CorrectionError> {
    count_record_id(
        b"harvestcircle.hub.correction.count.explanation.record.v1\0",
        operation,
    )
}
pub fn count_classification_id(operation: HubRecordId) -> Result<HubRecordId, CorrectionError> {
    count_record_id(
        b"harvestcircle.hub.correction.count.classification.record.v1\0",
        operation,
    )
}
fn count_record_id(purpose: &[u8], operation: HubRecordId) -> Result<HubRecordId, CorrectionError> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(purpose);
    hash.update(operation.as_bytes());
    HubRecordId::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| CorrectionError::InvalidHistory)
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountExplanationFacts {
    pub report: HubRecordId,
    pub count_reconciliation: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_prior: Option<HubRecordId>,
    pub packs: WholePacks,
    pub recipient: HubRecordId,
    pub occurred: Occurrence,
    pub source_evidence: SourceProvenance,
    pub quantity_evidence: SourceProvenance,
    pub recipient_evidence: SourceProvenance,
    pub timing_evidence: SourceProvenance,
}
/// Explicit historical source selection. The count's balancing group is fixed
/// by its original; selecting a source never authorizes another physical debit.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountExplanationCommand {
    pub facts: CountExplanationFacts,
    pub source_operation: HubRecordId,
    pub source_group: HubRecordId,
}
impl CountExplanationCommand {
    pub fn plan(
        &self,
        context: &CountExplanationContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CountExplanationPlan, QuantityEffectError> {
        let source = context.source.source.source;
        if source.operation != self.source_operation || source.group.id != self.source_group {
            return Err(QuantityEffectError::InvalidScope);
        }
        self.facts.plan(context, operation, actor, now)
    }
}
pub struct CountExplanationContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub latest_recorded_at: UnixTimestamp,
    pub catalog: &'a ReferenceCatalog,
    pub account: &'a CountLossQuantityAccount,
    pub report: &'a ActualReportRecord,
    pub prior_report_review: Option<HubRecordId>,
    pub source: CountedShortageSourceContext<'a>,
    /// Complete original treatment at exactly the named pre-count lineage cut.
    pub treatment_at_count: &'a TreatmentState,
    pub known_departures: Vec<CorrectionKnownDeparture<'a>>,
    pub uncertain_submissions: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountExplanationPlan {
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub report: ActualReportRecord,
    pub facts: CountExplanationFacts,
    pub count: CountPostPlan,
    pub explanation: CountLossExplanation,
    pub historical_source: ResolvedActualSource,
    pub known_event: ActualKnownDeparture,
    pub unexplained_after: WholePacks,
    pub potential_historical_holds: Vec<HistoricalHoldFinding>,
}
impl CountExplanationFacts {
    pub fn plan(
        &self,
        context: &CountExplanationContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CountExplanationPlan, QuantityEffectError> {
        if context.uncertain_submissions {
            return Err(QuantityEffectError::UncertainSubmission);
        }
        if context.prior_report_review.is_some() {
            return Err(QuantityEffectError::AlreadyReviewed);
        }
        let account = context.account;
        let prior = account
            .explanations()
            .iter()
            .map(|e| (e.stock_revision, e.operation))
            .chain(
                account
                    .amendments()
                    .iter()
                    .map(|a| (a.stock_revision, a.operation)),
            )
            .max_by_key(|(revision, _)| revision.value());
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
            || self.expected_prior != prior.map(|(_, op)| op)
        {
            return Err(QuantityEffectError::StaleRevision);
        }
        context
            .report
            .validate()
            .map_err(QuantityEffectError::Actual)?;
        let count = context.source.count;
        count
            .validate()
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        let CountEffect::Shortage {
            packs: shortage, ..
        } = count.effect
        else {
            return Err(QuantityEffectError::InvalidScope);
        };
        let original = account.original();
        if count.id != original.reconciliation
            || count.operation != original.operation
            || count.stock_revision != original.stock_revision
            || count.before.trace != original.trace
            || count.before.variant != original.variant
            || shortage != original.packs
            || self.count_reconciliation != count.id
            || count.count.scope.hub != context.hub
            || count.stock_revision.value() > context.stock_revision.value()
            || count.catalog_revision.value() > context.catalog.revision().value()
            || count.recorded_at > context.latest_recorded_at
            || now < context.latest_recorded_at
            || prior.is_some_and(|(at, _)| at.value() > context.stock_revision.value())
            || operation == count.operation
            || operation == count.id
            || operation == context.report.operation
            || operation == context.source.source.source.operation
            || account
                .explanations()
                .iter()
                .any(|e| e.operation == operation || e.operation == context.report.operation)
            || account
                .amendments()
                .iter()
                .any(|a| a.operation == operation || a.operation == context.report.operation)
            || self.packs.value() == 0
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let facts = CorrectedLineFacts {
            report: self.report,
            target_operation: count.operation,
            target_group: count.before.id,
            expected_stock_revision: self.expected_stock_revision,
            expected_catalog_revision: self.expected_catalog_revision,
            expected_prior: self.expected_prior,
            corrected_line_packs: self.packs,
            recipient: Some(self.recipient),
            occurred: self.occurred,
            source_evidence: self.source_evidence.clone(),
            quantity_evidence: self.quantity_evidence.clone(),
            recipient_evidence: self.recipient_evidence.clone(),
            timing_evidence: self.timing_evidence.clone(),
        };
        validate_corrected_report(
            &facts,
            context.report,
            original.variant,
            &KnownCut {
                hub: context.hub,
                stock: context.stock_revision,
                catalog: context.catalog,
                latest: context.latest_recorded_at,
            },
            now,
        )?;
        let resolved = context
            .source
            .resolve(context.report, self.occurred, now)
            .map_err(QuantityEffectError::Actual)?;
        if context.known_departures.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
            return Err(QuantityEffectError::Actual(ActualReportError::Capacity));
        }
        let mut known = BTreeSet::new();
        let cut = KnownCut {
            hub: context.hub,
            stock: context.stock_revision,
            catalog: context.catalog,
            latest: context.latest_recorded_at,
        };
        for evidence in &context.known_departures {
            let associations = evidence.validate(&cut)?;
            let event = evidence.original;
            if event.hub != context.hub
                || event.stock_revision.value() > context.stock_revision.value()
                || event.catalog_revision.value() > context.catalog.revision().value()
                || event.recorded_at > context.latest_recorded_at
                || event.operation == operation
                || !known.insert((event.operation, event.trace, event.variant.id))
            {
                return Err(QuantityEffectError::InvalidHistory);
            }
            if event.trace == original.trace
                && event.variant.id == original.variant
                && associations.could_match(Some(self.recipient))
                && occurrences_overlap(event.occurred, self.occurred)
            {
                return Err(QuantityEffectError::Actual(
                    ActualReportError::DuplicateReviewRequired,
                ));
            }
        }
        let graph = context.source.source.lineage;
        context
            .treatment_at_count
            .project(graph, context.catalog.handling_basis())
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        if context
            .treatment_at_count
            .decisions()
            .iter()
            .any(|d| d.stamp.recorded_at > count.recorded_at)
            || context
                .treatment_at_count
                .initial_reviews()
                .iter()
                .any(|r| r.stamp.recorded_at > count.recorded_at)
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let mut holds = Vec::new();
        for hold in context.treatment_at_count.holds() {
            if hold.stamp.recorded_at > count.recorded_at
                || hold
                    .release
                    .as_ref()
                    .is_some_and(|r| r.recorded_at > count.recorded_at)
            {
                return Err(QuantityEffectError::InvalidHistory);
            }
            if hold.scope.anchors().iter().any(|a| {
                graph.captured_member(a.group, hold.scope.captured_revision(), count.before.id)
            }) {
                holds.push(HistoricalHoldFinding {
                    original: hold.clone(),
                    conditional_timing: classify_hold_timing(
                        self.occurred,
                        hold.stamp.occurred,
                        hold.release.as_ref().map(|r| r.occurred),
                    ),
                    applicability: HistoricalHoldApplicability::HistoricalScopeUnverified,
                });
            }
        }
        let revision = context
            .stock_revision
            .next()
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        let explanation = CountLossExplanation {
            operation,
            stock_revision: revision,
            packs: self.packs,
        };
        let mut explanations = account.explanations().to_vec();
        explanations.push(explanation);
        let returns = account
            .effective_explanations()
            .iter()
            .filter(|e| e.established_returns.value() > 0)
            .map(|e| CountExplanationReturns {
                explanation: e.original.operation,
                packs: e.established_returns,
            })
            .collect();
        let after = CountLossQuantityAccount::restore_corrected(
            original,
            explanations,
            account.amendments().to_vec(),
            returns,
        )
        .map_err(QuantityEffectError::Accounting)?;
        let event = ActualKnownDeparture {
            operation,
            actor,
            hub: context.hub,
            stock_revision: revision,
            catalog_revision: context.catalog.revision(),
            trace: original.trace,
            variant: context
                .catalog
                .require_kind(original.variant, ReferenceKind::Pack)
                .map_err(|_| QuantityEffectError::InvalidScope)?
                .clone(),
            recipient: context
                .catalog
                .require_kind(self.recipient, ReferenceKind::Party)
                .map_err(|_| QuantityEffectError::InvalidScope)?
                .clone(),
            occurred: self.occurred,
            recorded_at: now,
            lines: vec![ActualKnownDepartureLine {
                group: count.before.id,
                location: resolved.historical().location.clone(),
                packs: self.packs,
            }],
        };
        event.validate().map_err(QuantityEffectError::Actual)?;
        Ok(CountExplanationPlan {
            operation,
            actor,
            recorded_at: now,
            report: context.report.clone(),
            facts: self.clone(),
            count: count.clone(),
            explanation,
            historical_source: resolved,
            known_event: event,
            unexplained_after: after.unexplained(),
            potential_historical_holds: holds,
        })
    }
}

#[cfg(test)]
mod tests;
