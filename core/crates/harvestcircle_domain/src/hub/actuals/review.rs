use super::*;
use crate::hub::movement::{PhysicalStockGroup, WorkProgress};
use crate::hub::portions::PhysicalLineage;
use crate::hub::reference::{ReferenceCatalog, ReferenceKind, ReferenceRecord, ReferenceValue};
use crate::hub::treatment::{Hold, TreatmentState};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalHoldTiming {
    EstablishedBreach,
    BeforeHold,
    AfterRelease,
    TimingUnknown,
}

/// Half-open effective restriction interval. Both occurrence and effective
/// timing may be uncertain. Recording order and current release status alone
/// cannot establish a breach or establish its absence.
/// This compares clocks only. Historical applicability of the captured physical
/// scope requires separate native original birth evidence, not current membership.
#[must_use]
pub fn classify_hold_timing(
    departure: Occurrence,
    effective: Occurrence,
    released: Option<Occurrence>,
) -> HistoricalHoldTiming {
    let invalid = |v| matches!(v, Occurrence::Interval { earliest, latest } if earliest > latest);
    if invalid(departure) || invalid(effective) || released.is_some_and(invalid) {
        return HistoricalHoldTiming::TimingUnknown;
    }
    if bounds(effective)
        .zip(released.and_then(bounds))
        .is_some_and(|((h_min, _), (_, r_max))| r_max < h_min)
    {
        return HistoricalHoldTiming::TimingUnknown;
    }
    let Some((d_min, d_max)) = bounds(departure) else {
        return HistoricalHoldTiming::TimingUnknown;
    };
    if bounds(effective).is_some_and(|(h_min, _)| d_max < h_min) {
        return HistoricalHoldTiming::BeforeHold;
    }
    if released
        .and_then(bounds)
        .is_some_and(|(_, r_max)| r_max <= d_min)
    {
        return HistoricalHoldTiming::AfterRelease;
    }
    if bounds(effective).is_some_and(|(_, h_max)| h_max <= d_min)
        && match released {
            None => true,
            Some(release) => bounds(release).is_some_and(|(r_min, _)| d_max < r_min),
        }
    {
        HistoricalHoldTiming::EstablishedBreach
    } else {
        HistoricalHoldTiming::TimingUnknown
    }
}

fn bounds(value: Occurrence) -> Option<(UnixTimestamp, UnixTimestamp)> {
    match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualReviewFacts {
    pub report: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub packs: WholePacks,
    pub recipient: HubRecordId,
    pub occurred: Occurrence,
    /// Explicit factual verification, not a control to override normal policy.
    pub source_evidence: SourceProvenance,
    pub quantity_evidence: SourceProvenance,
    pub recipient_evidence: SourceProvenance,
    pub timing_evidence: SourceProvenance,
}

pub struct ActualReviewContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub original: &'a ActualReportRecord,
    pub group: &'a PhysicalStockGroup,
    pub lineage: &'a PhysicalLineage,
    pub treatment: &'a TreatmentState,
    /// Native submission uncertainty; report preservation itself does not need
    /// this to be clear, but review readiness cannot hide an unknown outcome.
    pub uncertain_submissions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalHoldFinding {
    pub original: Hold,
    pub conditional_timing: HistoricalHoldTiming,
    pub applicability: HistoricalHoldApplicability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalHoldApplicability {
    /// Native recorded-order lineage does not prove physical anchor birth time.
    /// No consumer may promote conditional timing to breach or clearance.
    HistoricalScopeUnverified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualBalanceComparison {
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub recipient: ReferenceRecord,
    pub current_location: ReferenceRecord,
    pub before: PhysicalStockGroup,
    pub compared_departure: WholePacks,
    pub on_hand_after: WholePacks,
    /// Pending scope would retire only after independently authorized native
    /// reconciliation; completed scope retains its actual history.
    pub pending_work_to_retire: Vec<HubRecordId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActualSourceComparison {
    UnknownClaim,
    /// A current identifier/location match still needs historical verification.
    CurrentCandidateRequiresHistoricalVerification,
    /// Movement or lineage changes may explain the difference. This does not
    /// contradict the original or select a source for an effect.
    HistoricalAssociationUnresolved,
    OutsideReportedCandidates,
}

/// Read-only comparison, deliberately incomplete for effect admission. Current
/// position is never historical source evidence. Immutable historical source
/// verification, duplicate-event review and current effect admission remain
/// prerequisites even when identifiers and locations happen to match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualReviewComparison {
    pub original: ActualReportRecord,
    pub facts: ActualReviewFacts,
    pub scope: ActualBalanceComparison,
    pub source_comparison: ActualSourceComparison,
    /// Findings apply to the compared current allocation. Association with the
    /// reported historical departure is still unresolved, not asserted here.
    pub potential_historical_holds: Vec<HistoricalHoldFinding>,
}

impl ActualReviewFacts {
    pub fn review(
        &self,
        context: &ActualReviewContext<'_>,
        now: UnixTimestamp,
    ) -> Result<ActualReviewComparison, ActualReportError> {
        context.original.validate()?;
        let group = context.group;
        group
            .validate_catalog(context.catalog)
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        let graph = context.lineage;
        let node = graph
            .node(group.id)
            .ok_or(ActualReportError::InvalidNativeContext)?;
        if context.hub != context.original.hub
            || graph.hub() != context.hub
            || graph.trace() != group.trace
            || graph.variant() != group.variant
            || graph.revision() != context.stock_revision
            || node.parent != group.parent
            || node.packs != group.packs
            || node.trace != group.trace
            || node.variant != group.variant
            || context.original.stock_revision.value() > context.stock_revision.value()
            || context.original.catalog_revision.value() > context.catalog.revision().value()
            || now < context.original.recorded_at
            || group.revision.value() > context.stock_revision.value()
            || group.causal_floor.is_some_and(|v| v > now)
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        if self.report != context.original.id
            || self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
            || self.group != group.id
            || self.expected_group_revision != group.revision
        {
            return Err(ActualReportError::StaleRevision);
        }
        if context.uncertain_submissions {
            return Err(ActualReportError::UncertainSubmission);
        }
        self.occurred
            .validate(context.original.recorded_at)
            .map_err(|_| ActualReportError::InvalidInput)?;
        if self.packs.value() == 0 || self.packs.value() > group.packs.value() {
            return Err(ActualReportError::InsufficientBalance);
        }
        let report = &context.original.report;
        if report.variant.is_some_and(|v| v != group.variant)
            || report.recipient.is_some_and(|v| v != self.recipient)
            || report.packs.is_some_and(|v| v != self.packs)
            || !refines(report.occurred, self.occurred)
        {
            return Err(ActualReportError::ConflictingEvidence);
        }
        let matches = |source: &ReportedActualSource| {
            source.trace == group.trace
                && source.group.is_none_or(|v| v == group.id)
                && source.location.is_none_or(|v| v == group.location)
        };
        let source_comparison = match &report.sources {
            ActualReportSources::Unknown => ActualSourceComparison::UnknownClaim,
            sources if sources.candidates().iter().any(matches) => {
                ActualSourceComparison::CurrentCandidateRequiresHistoricalVerification
            }
            sources if sources.candidates().iter().any(|s| s.trace == group.trace) => {
                ActualSourceComparison::HistoricalAssociationUnresolved
            }
            _ => ActualSourceComparison::OutsideReportedCandidates,
        };
        // Validate the complete original treatment history, but do not apply
        // its current normal-departure eligibility to an already-taken event.
        context
            .treatment
            .project(graph, context.catalog.handling_basis())
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        let mut potential_historical_holds = Vec::new();
        for hold in context.treatment.holds() {
            if hold.stamp.recorded_at > now
                || hold.release.as_ref().is_some_and(|r| r.recorded_at > now)
            {
                return Err(ActualReportError::InvalidNativeContext);
            }
            let scope = graph
                .resolve(&hold.scope)
                .map_err(|_| ActualReportError::InvalidNativeContext)?;
            if scope.groups().iter().any(|n| n.id == group.id) {
                potential_historical_holds.push(HistoricalHoldFinding {
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
        let reference = |id, kind| {
            context
                .catalog
                .require_kind(id, kind)
                .cloned()
                .map_err(|_| ActualReportError::InvalidNativeContext)
        };
        let variant = reference(group.variant, ReferenceKind::Pack)?;
        let ReferenceValue::Pack { product, .. } = &variant.value else {
            return Err(ActualReportError::InvalidNativeContext);
        };
        let scope = ActualBalanceComparison {
            product: reference(*product, ReferenceKind::Product)?,
            variant,
            recipient: reference(self.recipient, ReferenceKind::Party)?,
            current_location: reference(group.location, ReferenceKind::Location)?,
            before: group.clone(),
            compared_departure: self.packs,
            on_hand_after: group
                .packs
                .checked_sub(self.packs)
                .map_err(|_| ActualReportError::InsufficientBalance)?,
            pending_work_to_retire: group
                .work
                .iter()
                .filter(|a| a.progress == WorkProgress::Pending)
                .map(|a| a.requirement.id)
                .collect(),
        };
        Ok(ActualReviewComparison {
            original: context.original.clone(),
            facts: self.clone(),
            scope,
            source_comparison,
            potential_historical_holds,
        })
    }
}

/// A review may resolve an unknown or narrow a reported interval. Changing an
/// already asserted fact requires a separate linked correction, not review.
pub(super) fn refines(original: Occurrence, verified: Occurrence) -> bool {
    match (bounds(original), bounds(verified)) {
        (None, _) => true,
        (Some((a, b)), Some((c, d))) => a <= c && d <= b,
        (Some(_), None) => false,
    }
}
