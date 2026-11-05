//! Closed additional-quantity planning. Preserved factual line totals are not
//! physical debit quantities. Native complete readers/admission remain mandatory.
use super::*;
use crate::hub::HubId;
use crate::hub::actuals::{
    ActualKnownDeparture, ActualPhysicalReduction, ActualReportError, ActualReportRecord,
    ActualSourceContext, HistoricalHoldApplicability, HistoricalHoldFinding, ResolvedActualSource,
    classify_hold_timing, reported_occurrence_refines,
};
use crate::hub::reference::{ReferenceCatalog, ReferenceKind, SourceProvenance};
use crate::hub::stock::Occurrence;
use crate::hub::treatment::TreatmentState;
use crate::{PublicKey, UnixTimestamp};

mod classification;
pub use classification::*;
mod count;
pub use count::*;
mod known;
use known::KnownCut;
pub use known::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantityEffectError {
    InvalidHistory,
    InvalidFacts,
    InvalidScope,
    StaleRevision,
    UncertainSubmission,
    AlreadyReviewed,
    UnsupportedFamily,
    Actual(ActualReportError),
    Accounting(CorrectionError),
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectedLineFacts {
    pub report: HubRecordId,
    pub target_operation: HubRecordId,
    pub target_group: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_prior: Option<HubRecordId>,
    /// The factual corrected total of this exact homogeneous original line.
    /// It is never a sum of unlike variants or the new physical debit alone.
    pub corrected_line_packs: WholePacks,
    pub recipient: Option<HubRecordId>,
    pub occurred: Occurrence,
    pub source_evidence: SourceProvenance,
    pub quantity_evidence: SourceProvenance,
    pub recipient_evidence: SourceProvenance,
    pub timing_evidence: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AdditionalQuantityCommand {
    pub facts: CorrectedLineFacts,
    pub source_operation: HubRecordId,
    pub source_group: HubRecordId,
    /// Exact present physical group selected for balancing. The historical
    /// source may be its ancestor and does not select a descendant implicitly.
    pub balancing_group: HubRecordId,
    pub expected_balancing_revision: HubRevision,
    pub duplicate_review_basis: SourceProvenance,
}
pub struct AdditionalQuantityContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub latest_recorded_at: UnixTimestamp,
    pub catalog: &'a ReferenceCatalog,
    pub family: CorrectedDepartureFamily,
    /// Complete immutable event projection for this population/variant; other
    /// event populations and issued versions remain in recipient_original.
    pub original: &'a ActualKnownDeparture,
    pub target: &'a DepartureQuantityAccount,
    pub recipient_original: &'a OriginalRecipientAttribution,
    pub recipient_history: &'a [RecipientAttributionCorrection],
    pub report: &'a ActualReportRecord,
    pub prior_report_review: Option<HubRecordId>,
    pub historical_source: ActualSourceContext<'a>,
    pub treatment: &'a TreatmentState,
    /// Complete prior normal/actual/classification event projections at this cut.
    /// Exclude only the exact original event being amended from duplicate matching.
    pub known_departures: Vec<CorrectionKnownDeparture<'a>>,
    pub uncertain_submissions: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AdditionalQuantityPlan {
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub family: CorrectedDepartureFamily,
    pub original: ActualKnownDeparture,
    pub original_line: OriginalDepartureQuantity,
    pub report: ActualReportRecord,
    pub facts: CorrectedLineFacts,
    pub amendment: DepartureQuantityAmendment,
    pub historical_source: ResolvedActualSource,
    pub reduction: ActualPhysicalReduction,
    pub after_causal_floor: Option<UnixTimestamp>,
    pub potential_historical_holds: Vec<HistoricalHoldFinding>,
    pub duplicate_review_basis: SourceProvenance,
}

pub fn additional_correction_id(operation: HubRecordId) -> Result<HubRecordId, CorrectionError> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.additional.record.v1\0");
    hash.update(operation.as_bytes());
    HubRecordId::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| CorrectionError::InvalidHistory)
}

impl AdditionalQuantityCommand {
    pub fn plan(
        &self,
        context: &AdditionalQuantityContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<AdditionalQuantityPlan, QuantityEffectError> {
        let f = &self.facts;
        let a = context.target;
        let target = a.original();
        let original = context.original;
        if context.uncertain_submissions {
            return Err(QuantityEffectError::UncertainSubmission);
        }
        if context.prior_report_review.is_some() {
            return Err(QuantityEffectError::AlreadyReviewed);
        }
        if context.family == CorrectedDepartureFamily::ExplainedCountLoss {
            return Err(QuantityEffectError::UnsupportedFamily);
        }
        if f.expected_stock_revision != context.stock_revision
            || f.expected_catalog_revision != context.catalog.revision()
            || f.expected_prior != a.amendments().last().map(|m| m.operation)
            || self.expected_balancing_revision
                != context.historical_source.balancing_group.revision
        {
            return Err(QuantityEffectError::StaleRevision);
        }
        original.validate().map_err(QuantityEffectError::Actual)?;
        context
            .report
            .validate()
            .map_err(QuantityEffectError::Actual)?;
        if f.target_operation != target.operation
            || f.target_group != target.group
            || original.operation != target.operation
            || original.trace != target.trace
            || original.variant.id != target.variant
            || original.stock_revision != target.stock_revision
            || original.hub != context.hub
            || original.recorded_at > context.latest_recorded_at
            || original.catalog_revision.value() > context.catalog.revision().value()
            || original
                .lines
                .iter()
                .filter(|l| l.group == target.group && l.packs == target.packs)
                .count()
                != 1
            || target.stock_revision.value() > context.stock_revision.value()
            || a.amendments()
                .iter()
                .any(|m| m.stock_revision.value() > context.stock_revision.value())
            || now < context.latest_recorded_at
            || operation == target.operation
            || operation == context.report.operation
            || operation == self.source_operation
            || a.amendments().iter().any(|m| m.operation == operation)
            || a.amendments()
                .iter()
                .any(|m| m.operation == context.report.operation)
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let recipient_original = context.recipient_original;
        if recipient_original.hub != original.hub
            || recipient_original.operation != original.operation
            || recipient_original.family != context.family
            || recipient_original.stock_revision != original.stock_revision
            || recipient_original.catalog_revision != original.catalog_revision
            || recipient_original.actor != original.actor
            || recipient_original.recorded_at != original.recorded_at
            || recipient_original.recipient != original.recipient
            || !recipient_original.populations.contains(&target.trace)
            || context.recipient_history.iter().any(|r| {
                r.stamp.stock_revision.value() > context.stock_revision.value()
                    || r.stamp.catalog_revision.value() > context.catalog.revision().value()
                    || r.stamp.recorded_at > context.latest_recorded_at
            })
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let recipient =
            current_recipient_attribution(recipient_original, context.recipient_history)
                .map_err(|_| QuantityEffectError::InvalidHistory)?;
        let recipient_id = match recipient {
            AmendedRecipient::Unknown => None,
            AmendedRecipient::Known(r) => Some(r.id),
        };
        if f.recipient != recipient_id {
            return Err(QuantityEffectError::InvalidFacts);
        }
        validate_corrected_report(
            f,
            context.report,
            target.variant,
            &KnownCut {
                hub: context.hub,
                stock: context.stock_revision,
                catalog: context.catalog,
                latest: context.latest_recorded_at,
            },
            now,
        )?;
        if !reported_occurrence_refines(original.occurred, f.occurred) {
            return Err(QuantityEffectError::InvalidFacts);
        }
        let delta = f
            .corrected_line_packs
            .checked_sub(a.effective())
            .map_err(|_| QuantityEffectError::InvalidFacts)?;
        if delta.value() == 0 {
            return Err(QuantityEffectError::InvalidFacts);
        }
        let source = &context.historical_source;
        if source.source.operation != self.source_operation
            || source.source.group.id != self.source_group
            || source.balancing_group.id != self.balancing_group
            || source.lineage.hub() != context.hub
            || source.lineage.trace() != target.trace
            || source.lineage.variant() != target.variant
            || source.lineage.revision() != context.stock_revision
            || source.source.recorded_at > context.latest_recorded_at
            || source.source.catalog_revision.value() > context.catalog.revision().value()
            || source
                .balancing_group
                .causal_floor
                .is_some_and(|at| at > context.latest_recorded_at)
            || source
                .subsequent_activity
                .iter()
                .any(|a| a.recorded_at > context.latest_recorded_at)
        {
            return Err(QuantityEffectError::InvalidScope);
        }
        let resolved = source
            .resolve(context.report, f.occurred, now)
            .map_err(QuantityEffectError::Actual)?;
        validate_duplicates(context, operation, f.recipient, f.occurred)?;
        let projected = context
            .treatment
            .project(source.lineage, context.catalog.handling_basis())
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        if context
            .treatment
            .decisions()
            .iter()
            .any(|d| d.stamp.recorded_at > context.latest_recorded_at)
            || context
                .treatment
                .initial_reviews()
                .iter()
                .any(|r| r.stamp.recorded_at > context.latest_recorded_at)
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        if !projected
            .groups
            .iter()
            .any(|g| g.group == source.balancing_group.id)
        {
            return Err(QuantityEffectError::InvalidScope);
        }
        let mut holds = Vec::new();
        for hold in context.treatment.holds() {
            if hold.stamp.recorded_at > context.latest_recorded_at
                || hold
                    .release
                    .as_ref()
                    .is_some_and(|r| r.recorded_at > context.latest_recorded_at)
            {
                return Err(QuantityEffectError::InvalidHistory);
            }
            if source
                .lineage
                .resolve(&hold.scope)
                .map_err(|_| QuantityEffectError::InvalidHistory)?
                .groups()
                .iter()
                .any(|g| g.id == source.balancing_group.id)
            {
                holds.push(HistoricalHoldFinding {
                    original: hold.clone(),
                    conditional_timing: classify_hold_timing(
                        f.occurred,
                        hold.stamp.occurred,
                        hold.release.as_ref().map(|r| r.occurred),
                    ),
                    applicability: HistoricalHoldApplicability::HistoricalScopeUnverified,
                });
            }
        }
        let reduction = ActualPhysicalReduction::plan(source.balancing_group, delta)
            .map_err(QuantityEffectError::Actual)?;
        let floor = reduction
            .causal_floor_after_reconciliation(f.occurred, now)
            .map_err(QuantityEffectError::Actual)?;
        let amendment = DepartureQuantityAmendment {
            operation,
            stock_revision: context
                .stock_revision
                .next()
                .map_err(|_| QuantityEffectError::InvalidHistory)?,
            before: a.effective(),
            after: f.corrected_line_packs,
            delta: DepartureQuantityDelta::AdditionalDeparture(delta),
        };
        let mut history = a.amendments().to_vec();
        history.push(amendment);
        DepartureQuantityAccount::restore(target, history, a.established_returns())
            .map_err(QuantityEffectError::Accounting)?;
        Ok(AdditionalQuantityPlan {
            operation,
            actor,
            recorded_at: now,
            family: context.family,
            original: original.clone(),
            original_line: target,
            report: context.report.clone(),
            facts: f.clone(),
            amendment,
            historical_source: resolved,
            reduction,
            after_causal_floor: floor,
            potential_historical_holds: holds,
            duplicate_review_basis: self.duplicate_review_basis.clone(),
        })
    }
}

fn validate_corrected_report(
    facts: &CorrectedLineFacts,
    report: &ActualReportRecord,
    variant: HubRecordId,
    cut: &KnownCut<'_>,
    now: UnixTimestamp,
) -> Result<(), QuantityEffectError> {
    if facts.report != report.id
        || report.hub != cut.hub
        || report.stock_revision.value() > cut.stock.value()
        || report.catalog_revision.value() > cut.catalog.revision().value()
        || report.recorded_at > cut.latest
        || report.operation == facts.target_operation
    {
        return Err(QuantityEffectError::InvalidHistory);
    }
    facts
        .occurred
        .validate(report.recorded_at)
        .map_err(|_| QuantityEffectError::InvalidFacts)?;
    let raw = &report.report;
    if raw.variant.is_some_and(|v| v != variant)
        || raw.recipient.is_some_and(|v| Some(v) != facts.recipient)
        || raw.packs.is_some_and(|v| v != facts.corrected_line_packs)
        || !reported_occurrence_refines(raw.occurred, facts.occurred)
        || now < report.recorded_at
    {
        return Err(QuantityEffectError::InvalidFacts);
    }
    cut.catalog
        .require_kind(variant, ReferenceKind::Pack)
        .map_err(|_| QuantityEffectError::InvalidScope)?;
    if let Some(recipient) = facts.recipient {
        cut.catalog
            .require_kind(recipient, ReferenceKind::Party)
            .map_err(|_| QuantityEffectError::InvalidScope)?;
    }
    Ok(())
}
fn validate_duplicates(
    context: &AdditionalQuantityContext<'_>,
    operation: HubRecordId,
    recipient: Option<HubRecordId>,
    occurred: Occurrence,
) -> Result<(), QuantityEffectError> {
    if context.known_departures.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(QuantityEffectError::Actual(ActualReportError::Capacity));
    }
    let mut identities = BTreeSet::new();
    let mut target_seen = false;
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
            || !identities.insert((event.operation, event.trace, event.variant.id))
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        if event.operation == context.original.operation {
            if event.trace == context.original.trace
                && event.variant.id == context.original.variant.id
            {
                if event != context.original {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                if evidence.recipient_original != context.recipient_original
                    || evidence.recipient_history != context.recipient_history
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                let KnownCorrectionQuantity::Physical(accounts) = evidence.quantity else {
                    return Err(QuantityEffectError::InvalidHistory);
                };
                if !accounts.iter().any(|a| a == context.target) {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                target_seen = true;
            }
            continue;
        }
        if event.trace == context.target.original().trace
            && event.variant.id == context.target.original().variant
            && associations.could_match(recipient)
            && occurrences_overlap(event.occurred, occurred)
        {
            return Err(QuantityEffectError::Actual(
                ActualReportError::DuplicateReviewRequired,
            ));
        }
    }
    if !target_seen {
        return Err(QuantityEffectError::InvalidHistory);
    }
    Ok(())
}
fn occurrences_overlap(a: Occurrence, b: Occurrence) -> bool {
    match (occurrence_bounds(a), occurrence_bounds(b)) {
        (Some((a, b)), Some((c, d))) => a <= d && c <= b,
        _ => true,
    }
}
fn occurrence_bounds(value: Occurrence) -> Option<(UnixTimestamp, UnixTimestamp)> {
    match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    }
}

#[cfg(test)]
mod tests;
