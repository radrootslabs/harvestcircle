//! Sealed original facts and derived results. These digests are integrity checks,
//! never current ownership, submission admission or permission to repeat effects.
use super::*;
use crate::hub::{
    custody_result,
    portions::{CapturedPhysicalScope, PhysicalLineage},
    treatment::{TreatmentStamp, TreatmentState},
};

/// A bounded immutable physical summary, distinct from the full review/context
/// seal. This neither certifies readable decision history nor grants an effect.
/// Native adapters use the closed physical-review header, never duplicate links.
pub fn actual_departure_projection_digest(
    input: &[u8; 32],
    report: HubRecordId,
    source: &HistoricalActualSource,
    record: &ActualKnownDeparture,
) -> Result<[u8; 32], ActualReportError> {
    source.validate()?;
    record.validate()?;
    if record.hub != source.hub
        || record.trace != source.group.trace
        || record.variant.id != source.group.variant
        || record.lines.len() != 1
        || record.lines[0].location != source.location
        || record.packs()?.value() > source.group.packs.value()
        || record.stock_revision.value() <= source.stock_revision.value()
        || record.catalog_revision.value() < source.catalog_revision.value()
        || record.recorded_at < source.recorded_at
    {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let bounds = |value| match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    };
    if !bounds(source.occurred)
        .zip(bounds(record.occurred))
        .is_some_and(|((_, latest), (earliest, _))| latest <= earliest)
    {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.departure.projection.v1\0");
    hash.update(input);
    hash.update(report.as_bytes());
    hash_source(&mut hash, source)?;
    known(&mut hash, record)?;
    Ok(hash.finalize().into())
}

pub fn actual_report_result_digest(
    input: &[u8; 32],
    record: &ActualReportRecord,
) -> Result<[u8; 32], ActualReportError> {
    record.validate()?;
    if record
        .report
        .fingerprint(record.hub, record.actor, record.operation)
        != *input
    {
        return Err(ActualReportError::InvalidHistory);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.report.result.v1\0");
    hash.update(input);
    for id in [record.id, record.operation] {
        hash.update(id.as_bytes());
    }
    hash.update(record.hub.as_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    reported_clock(&mut hash, record.recording_clock_assessment());
    Ok(hash.finalize().into())
}

pub fn actual_departure_result_digest(
    input: &[u8; 32],
    command: &ActualNewDepartureCommand,
    context: &ActualNewDepartureContext<'_>,
    births: &[ActualPhysicalBirth],
    findings: &[HistoricalHoldScopeAssessment],
    record: &ActualNewDeparturePlan,
) -> Result<[u8; 32], ActualReportError> {
    departure_digest(input, command, context, None, births, findings, record)
}

/// Qualified association data is bound separately; Some(empty) remains distinct
/// from the legacy absent extension and cannot replace a nonempty baseline.
pub fn actual_departure_result_digest_with_interpretations(
    input: &[u8; 32],
    command: &ActualNewDepartureCommand,
    context: &ActualNewDepartureContext<'_>,
    interpretations: &ActualKnownInterpretations,
    births: &[ActualPhysicalBirth],
    findings: &[HistoricalHoldScopeAssessment],
    record: &ActualNewDeparturePlan,
) -> Result<[u8; 32], ActualReportError> {
    departure_digest(
        input,
        command,
        context,
        Some(interpretations),
        births,
        findings,
        record,
    )
}

fn departure_digest(
    input: &[u8; 32],
    command: &ActualNewDepartureCommand,
    context: &ActualNewDepartureContext<'_>,
    interpretations: Option<&ActualKnownInterpretations>,
    births: &[ActualPhysicalBirth],
    findings: &[HistoricalHoldScopeAssessment],
    record: &ActualNewDeparturePlan,
) -> Result<[u8; 32], ActualReportError> {
    let replanned = match interpretations {
        Some(records) => command.plan_with_interpretations(
            context,
            records,
            record.operation,
            record.actor,
            record.recorded_at,
        ),
        None => command.plan(context, record.operation, record.actor, record.recorded_at),
    }?;
    if command.fingerprint(context.review.hub, record.actor, record.operation) != *input
        || replanned != *record
    {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let graph = context.review.lineage;
    if births.len() != graph.nodes().len() {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let mut ids = BTreeSet::new();
    for birth in births {
        birth.validate(graph, record.recorded_at)?;
        if !ids.insert(birth.group) {
            return Err(ActualReportError::InvalidNativeContext);
        }
    }
    let expected: Vec<_> = context
        .review
        .treatment
        .holds()
        .iter()
        .map(|hold| {
            HistoricalHoldScopeContext {
                lineage: graph,
                group: command.facts.group,
                occurred: command.facts.occurred,
                births,
                now: record.recorded_at,
            }
            .verify(hold)
        })
        .collect::<Result<_, _>>()?;
    if expected != findings {
        return Err(ActualReportError::InvalidHistory);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.departure.result.v1\0");
    hash.update(input);
    hash.update(record.operation.as_bytes());
    hash.update(context.review.hub.as_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(context.review.catalog.revision().value().to_be_bytes());
    let report_input = record.report.report.fingerprint(
        record.report.hub,
        record.report.actor,
        record.report.operation,
    );
    hash.update(actual_report_result_digest(&report_input, &record.report)?);
    // All original reference content, not only numeric revisions.
    length(&mut hash, context.review.catalog.records().count())?;
    for value in context.review.catalog.records() {
        custody_result::reference(
            &mut hash,
            value,
            context.review.hub,
            record.actor,
            context.review.catalog.revision(),
        );
    }
    lineage(&mut hash, graph)?;
    treatment(&mut hash, context.review.treatment)?;
    let source = record.historical_source.historical();
    hash_source(&mut hash, source)?;
    hash.update(record.historical_source.balancing_group().as_bytes());
    length(
        &mut hash,
        context.historical_source.subsequent_activity.len(),
    )?;
    for event in context.historical_source.subsequent_activity {
        hash.update(event.sequence.to_be_bytes());
        hash.update(event.operation.as_bytes());
        hash.update(event.actor.as_bytes());
        hash.update(event.stock_revision.value().to_be_bytes());
        hash.update(event.hub.as_bytes());
        hash.update(event.trace.as_bytes());
        hash.update(event.group.as_bytes());
        optional_id(&mut hash, event.location);
        hash.update([match event.kind {
            ActualPhysicalActivityKind::GroupBorn => 0,
            ActualPhysicalActivityKind::LocationChanged => 1,
            ActualPhysicalActivityKind::CountReconciled => 2,
            ActualPhysicalActivityKind::OtherPhysicalEffect => 3,
        }]);
        hash_occurrence(&mut hash, event.occurred);
        hash.update(event.recorded_at.as_seconds().to_be_bytes());
    }
    length(&mut hash, context.known_departures.len())?;
    for original in context.known_departures {
        known(&mut hash, original)?;
    }
    length(&mut hash, births.len())?;
    for original in births {
        birth(&mut hash, original);
    }
    length(&mut hash, findings.len())?;
    for finding in findings {
        match finding {
            HistoricalHoldScopeAssessment::Unresolved {
                original,
                conditional_timing,
                reason,
            } => {
                hash.update([0]);
                hold(&mut hash, original)?;
                timing(&mut hash, *conditional_timing);
                hash.update([match reason {
                    HistoricalHoldScopeUncertainty::ScopeNotEstablished => 0,
                    HistoricalHoldScopeUncertainty::MissingBirth => 1,
                    HistoricalHoldScopeUncertainty::UnknownBirthTime => 2,
                    HistoricalHoldScopeUncertainty::GroupNotPresentAtDeparture => 3,
                    HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime => 4,
                    HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry => 5,
                    HistoricalHoldScopeUncertainty::ExpandedPhysicalScope => 6,
                    HistoricalHoldScopeUncertainty::AnchorCohortUnverified => 7,
                }]);
            }
            HistoricalHoldScopeAssessment::Verified(verified) => {
                hash.update([1]);
                hold(&mut hash, verified.original())?;
                hash.update(verified.group().as_bytes());
                hash_occurrence(&mut hash, verified.occurred());
                hash.update(verified.lineage_revision().value().to_be_bytes());
                timing(&mut hash, verified.timing());
                length(&mut hash, verified.births().len())?;
                for original in verified.births() {
                    birth(&mut hash, original);
                }
            }
        }
    }
    // Derived physical/work results are sealed as well as every captured input.
    let reduction = &record.reduction;
    custody_result::group(
        &mut hash,
        &reduction.before,
        context.review.hub,
        record.actor,
        context.review.catalog.revision(),
    );
    hash.update(reduction.actual_departed.value().to_be_bytes());
    hash.update(reduction.on_hand_after.value().to_be_bytes());
    hash.update(reduction.after_revision.value().to_be_bytes());
    custody_result::clock(&mut hash, record.after_causal_floor);
    length(&mut hash, reduction.work_changes.len())?;
    for change in &reduction.work_changes {
        custody_result::work(
            &mut hash,
            &change.before,
            context.review.hub,
            record.actor,
            context.review.catalog.revision(),
        );
        custody_result::work(
            &mut hash,
            &change.after,
            context.review.hub,
            record.actor,
            context.review.catalog.revision(),
        );
        hash.update(change.retired.value().to_be_bytes());
    }
    length(&mut hash, reduction.removed_work_scope.len())?;
    for scope in &reduction.removed_work_scope {
        hash.update(scope.work.as_bytes());
        progress(&mut hash, scope.before_progress);
        hash.update(scope.before_scoped.value().to_be_bytes());
        hash.update(scope.on_hand_after.value().to_be_bytes());
        hash.update(scope.removed.value().to_be_bytes());
        progress(&mut hash, scope.removed_progress);
    }
    reported_clock(&mut hash, record.comparison.reported_clock);
    let comparison = &record.comparison.scope;
    for reference in [
        &comparison.product,
        &comparison.variant,
        &comparison.recipient,
        &comparison.current_location,
    ] {
        custody_result::reference(
            &mut hash,
            reference,
            context.review.hub,
            record.actor,
            context.review.catalog.revision(),
        );
    }
    custody_result::group(
        &mut hash,
        &comparison.before,
        context.review.hub,
        record.actor,
        context.review.catalog.revision(),
    );
    hash.update(comparison.compared_departure.value().to_be_bytes());
    hash.update(comparison.on_hand_after.value().to_be_bytes());
    hash.update([match record.comparison.source_comparison {
        ActualSourceComparison::UnknownClaim => 0,
        ActualSourceComparison::CurrentCandidateRequiresHistoricalVerification => 1,
        ActualSourceComparison::HistoricalAssociationUnresolved => 2,
        ActualSourceComparison::OutsideReportedCandidates => 3,
    }]);
    length(
        &mut hash,
        record.comparison.scope.pending_work_to_retire.len(),
    )?;
    for id in &record.comparison.scope.pending_work_to_retire {
        hash.update(id.as_bytes());
    }
    length(
        &mut hash,
        record.comparison.potential_historical_holds.len(),
    )?;
    for finding in &record.comparison.potential_historical_holds {
        hold(&mut hash, &finding.original)?;
        timing(&mut hash, finding.conditional_timing);
        hash.update([match finding.applicability {
            HistoricalHoldApplicability::HistoricalScopeUnverified => 0,
        }]);
    }
    hash_text(&mut hash, record.duplicate_review_basis.as_str());
    if let Some(set) = interpretations {
        hash.update(b"harvestcircle.hub.actual.known.interpretations.v1\0");
        hash.update(set.hub.as_bytes());
        hash.update(set.stock_revision.value().to_be_bytes());
        hash.update(set.catalog_revision.value().to_be_bytes());
        hash.update(set.latest_recorded_at.as_seconds().to_be_bytes());
        length(&mut hash, set.records.len())?;
        for record in &set.records {
            hash.update(record.digest()?);
        }
    }
    Ok(hash.finalize().into())
}

pub fn actual_duplicate_result_digest(
    input: &[u8; 32],
    command: &ActualDuplicateLink,
    record: &ActualDuplicateLinkPlan,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<[u8; 32], ActualReportError> {
    let replanned = match record.interpretation.as_deref() {
        Some(value) => {
            if value.stock_revision.value().checked_add(1) != Some(revision.value())
                || value.catalog_revision != catalog
            {
                return Err(ActualReportError::InvalidNativeContext);
            }
            if let Some(source) = record.count_source.as_deref() {
                command.plan_with_count_interpretation(
                    &record.original,
                    ActualCountDuplicateContext {
                        interpretation: value,
                        source,
                    },
                    None,
                    record.operation,
                    record.actor,
                    record.recorded_at,
                )?
            } else {
                command.plan_with_interpretation(
                    &record.original,
                    value,
                    None,
                    record.operation,
                    record.actor,
                    record.recorded_at,
                )?
            }
        }
        None if record.count_source.is_none() => command.plan(
            &record.original,
            &record.known,
            None,
            record.operation,
            record.actor,
            record.recorded_at,
        )?,
        None => return Err(ActualReportError::InvalidNativeContext),
    };
    if command.fingerprint(record.original.hub, record.actor, record.operation) != *input
        || replanned != *record
        || revision.value() <= record.original.stock_revision.value()
        || revision.value() <= record.known.stock_revision.value()
        || catalog.value() < record.original.catalog_revision.value()
        || catalog.value() < record.known.catalog_revision.value()
    {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.duplicate.result.v1\0");
    hash.update(input);
    hash.update(record.operation.as_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash.update(revision.value().to_be_bytes());
    hash.update(catalog.value().to_be_bytes());
    let report_input = record.original.report.fingerprint(
        record.original.hub,
        record.original.actor,
        record.original.operation,
    );
    hash.update(actual_report_result_digest(
        &report_input,
        &record.original,
    )?);
    known(&mut hash, &record.known)?;
    hash_text(&mut hash, record.verification.as_str());
    if let Some(value) = &record.interpretation {
        hash.update(b"harvestcircle.hub.actual.duplicate.interpretation.v1\0");
        hash.update(value.digest()?);
    }
    if let Some(source) = record.count_source.as_deref() {
        hash.update(b"harvestcircle.hub.actual.duplicate.count_source.v1\0");
        hash.update(source.digest()?);
    }
    Ok(hash.finalize().into())
}

fn length(hash: &mut Sha256, len: usize) -> Result<(), ActualReportError> {
    hash.update(
        u32::try_from(len)
            .map_err(|_| ActualReportError::Capacity)?
            .to_be_bytes(),
    );
    Ok(())
}
fn optional_id(hash: &mut Sha256, id: Option<HubRecordId>) {
    hash.update([u8::from(id.is_some())]);
    if let Some(id) = id {
        hash.update(id.as_bytes());
    }
}
fn lineage(hash: &mut Sha256, graph: &PhysicalLineage) -> Result<(), ActualReportError> {
    hash.update(graph.hub().as_bytes());
    hash.update(graph.trace().as_bytes());
    hash.update(graph.variant().as_bytes());
    hash.update(graph.revision().value().to_be_bytes());
    length(hash, graph.nodes().len())?;
    for node in graph.nodes() {
        hash.update(node.id.as_bytes());
        optional_id(hash, node.parent);
        hash.update(node.trace.as_bytes());
        hash.update(node.variant.as_bytes());
        hash.update(node.created_revision.value().to_be_bytes());
        hash.update(node.packs.value().to_be_bytes());
    }
    let origins = graph
        .nodes()
        .iter()
        .filter_map(|n| n.recovery.map(|o| (n.id, o)))
        .collect::<Vec<_>>();
    if !origins.is_empty() {
        hash.update(b"harvestcircle.hub.lineage.recovery-origin.capture.v1\0");
        length(hash, origins.len())?;
        for (group, origin) in origins {
            hash.update(group.as_bytes());
            hash.update(origin.debit_operation.as_bytes());
            hash.update(origin.debit_revision.value().to_be_bytes());
        }
    }
    Ok(())
}
fn stamp(hash: &mut Sha256, value: &TreatmentStamp) {
    hash.update(value.id.as_bytes());
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.actor.as_bytes());
    hash_text(hash, value.basis.id());
    hash.update(value.basis.version().to_be_bytes());
    hash_text(hash, value.basis.site_timezone());
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
    hash_occurrence(hash, value.occurred);
    hash_text(hash, value.observation.as_str());
}
fn scope(hash: &mut Sha256, value: &CapturedPhysicalScope) -> Result<(), ActualReportError> {
    hash.update(value.hub().as_bytes());
    hash.update(value.trace().as_bytes());
    hash.update(value.variant().as_bytes());
    hash.update(value.captured_revision().value().to_be_bytes());
    length(hash, value.anchors().len())?;
    for anchor in value.anchors() {
        hash.update(anchor.group.as_bytes());
        hash.update(anchor.created_revision.value().to_be_bytes());
        hash.update(anchor.captured_packs.value().to_be_bytes());
    }
    Ok(())
}
fn hold(hash: &mut Sha256, value: &crate::hub::treatment::Hold) -> Result<(), ActualReportError> {
    stamp(hash, &value.stamp);
    scope(hash, &value.scope)?;
    hash.update([u8::from(value.release.is_some())]);
    if let Some(release) = &value.release {
        stamp(hash, release);
    }
    Ok(())
}
fn treatment(hash: &mut Sha256, value: &TreatmentState) -> Result<(), ActualReportError> {
    length(hash, value.decisions().len())?;
    for decision in value.decisions() {
        stamp(hash, &decision.stamp);
        scope(hash, &decision.scope)?;
        hash.update([match decision.status {
            crate::hub::treatment::AcceptanceStatus::ReviewRequired => 0,
            crate::hub::treatment::AcceptanceStatus::Accepted => 1,
            crate::hub::treatment::AcceptanceStatus::Rejected => 2,
        }]);
    }
    length(hash, value.holds().len())?;
    for value in value.holds() {
        hold(hash, value)?;
    }
    if !value.initial_reviews().is_empty() {
        hash.update(b"harvestcircle.hub.treatment.initial-review.capture.v1\0");
        length(hash, value.initial_reviews().len())?;
        for review in value.initial_reviews() {
            stamp(hash, &review.stamp);
            scope(hash, &review.scope)?;
            hash.update(b"review_required\0");
        }
    }
    Ok(())
}
fn birth(hash: &mut Sha256, value: &ActualPhysicalBirth) {
    hash.update([match value.kind {
        ActualPhysicalBirthKind::Receipt => 0,
        ActualPhysicalBirthKind::CountedOpening => 1,
        ActualPhysicalBirthKind::Returned => 2,
        ActualPhysicalBirthKind::CountedSurplus => 3,
        ActualPhysicalBirthKind::ObservedPortion => 4,
        ActualPhysicalBirthKind::FoundRemainder { .. } => 5,
    }]);
    if let ActualPhysicalBirthKind::FoundRemainder { allocation } = value.kind {
        hash.update([allocation]);
    }
    hash.update(value.operation.as_bytes());
    hash.update(value.hub.as_bytes());
    hash.update(value.trace.as_bytes());
    hash.update(value.variant.as_bytes());
    hash.update(value.group.as_bytes());
    optional_id(hash, value.parent);
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.packs.value().to_be_bytes());
    hash.update(value.actor.as_bytes());
    hash_occurrence(hash, value.occurred);
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
}
pub(super) fn known_original_digest(
    value: &ActualKnownDeparture,
) -> Result<[u8; 32], ActualReportError> {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count.explanation.known_source_original.v1\0");
    known(&mut hash, value)?;
    Ok(hash.finalize().into())
}

fn known(hash: &mut Sha256, value: &ActualKnownDeparture) -> Result<(), ActualReportError> {
    value.validate()?;
    hash.update(value.operation.as_bytes());
    hash.update(value.actor.as_bytes());
    hash.update(value.hub.as_bytes());
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.catalog_revision.value().to_be_bytes());
    hash.update(value.trace.as_bytes());
    custody_result::reference(
        hash,
        &value.variant,
        value.hub,
        value.actor,
        value.catalog_revision,
    );
    custody_result::reference(
        hash,
        &value.recipient,
        value.hub,
        value.actor,
        value.catalog_revision,
    );
    hash_occurrence(hash, value.occurred);
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
    length(hash, value.lines.len())?;
    for line in &value.lines {
        hash.update(line.group.as_bytes());
        custody_result::reference(
            hash,
            &line.location,
            value.hub,
            value.actor,
            value.catalog_revision,
        );
        hash.update(line.packs.value().to_be_bytes());
    }
    Ok(())
}
fn timing(hash: &mut Sha256, value: HistoricalHoldTiming) {
    hash.update([match value {
        HistoricalHoldTiming::EstablishedBreach => 0,
        HistoricalHoldTiming::BeforeHold => 1,
        HistoricalHoldTiming::AfterRelease => 2,
        HistoricalHoldTiming::TimingUnknown => 3,
    }]);
}
fn progress(hash: &mut Sha256, value: crate::hub::movement::WorkProgress) {
    hash.update([match value {
        crate::hub::movement::WorkProgress::Pending => 0,
        crate::hub::movement::WorkProgress::Completed => 1,
        crate::hub::movement::WorkProgress::NoLongerRequired => 2,
    }]);
}

fn reported_clock(hash: &mut Sha256, value: ActualReportedClockAssessment) {
    hash.update([match value {
        ActualReportedClockAssessment::Unknown => 0,
        ActualReportedClockAssessment::NotAfterRecordedTime => 1,
        ActualReportedClockAssessment::RecordingClockConflict => 2,
    }]);
}

pub(super) fn hash_source(
    hash: &mut Sha256,
    source: &HistoricalActualSource,
) -> Result<(), ActualReportError> {
    source.validate()?;
    hash.update(source.operation.as_bytes());
    hash.update(source.actor.as_bytes());
    hash.update(source.hub.as_bytes());
    hash.update(source.stock_revision.value().to_be_bytes());
    hash.update(source.catalog_revision.value().to_be_bytes());
    for id in [
        source.group.id,
        source.group.trace,
        source.group.variant,
        source.group.location,
    ] {
        hash.update(id.as_bytes());
    }
    optional_id(hash, source.group.parent);
    hash.update(source.group.packs.value().to_be_bytes());
    hash.update(source.group.revision.value().to_be_bytes());
    custody_result::reference(
        hash,
        &source.location,
        source.hub,
        source.actor,
        source.catalog_revision,
    );
    hash_occurrence(hash, source.occurred);
    hash.update(source.recorded_at.as_seconds().to_be_bytes());
    Ok(())
}

// Manual bounded field hashing introduces no JSON codec or request decoder into
// domain rules. All original/current facts are covered, not only derived totals.
pub(super) fn interpretation_digest(
    value: &ActualKnownInterpretation,
) -> Result<[u8; 32], ActualReportError> {
    interpretation_digest_inner(value, true)
}
fn interpretation_digest_inner(
    value: &ActualKnownInterpretation,
    include_count_snapshot: bool,
) -> Result<[u8; 32], ActualReportError> {
    use crate::hub::corrections::{
        AmendedRecipient, CorrectedDepartureFamily, CorrectionImpact, DepartureQuantityDelta,
    };
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.known.interpretation.v1\0");
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.catalog_revision.value().to_be_bytes());
    hash.update(value.latest_recorded_at.as_seconds().to_be_bytes());
    known(&mut hash, &value.original)?;
    let original = &value.recipient_original;
    hash.update(original.hub.as_bytes());
    hash.update([match original.family {
        CorrectedDepartureFamily::Normal => 0,
        CorrectedDepartureFamily::ReviewedActual => 1,
        CorrectedDepartureFamily::ExplainedCountLoss => 2,
    }]);
    hash.update(original.operation.as_bytes());
    hash.update(original.stock_revision.value().to_be_bytes());
    hash.update(original.catalog_revision.value().to_be_bytes());
    hash.update(original.recorded_at.as_seconds().to_be_bytes());
    hash.update(original.actor.as_bytes());
    custody_result::reference(
        &mut hash,
        &original.recipient,
        original.hub,
        original.actor,
        original.catalog_revision,
    );
    length(&mut hash, original.populations.len())?;
    for id in &original.populations {
        hash.update(id.as_bytes());
    }
    length(&mut hash, original.issued_versions.len())?;
    for id in &original.issued_versions {
        hash.update(id.as_bytes());
    }
    length(&mut hash, value.recipient_history.len())?;
    for record in &value.recipient_history {
        let stamp = &record.stamp;
        hash.update(stamp.hub.as_bytes());
        hash.update(stamp.operation.as_bytes());
        hash.update(stamp.actor.as_bytes());
        hash.update(stamp.stock_revision.value().to_be_bytes());
        hash.update(stamp.catalog_revision.value().to_be_bytes());
        hash.update(stamp.recorded_at.as_seconds().to_be_bytes());
        crate::hub::stock::hash_optional_id(&mut hash, record.prior);
        for recipient in [&record.before, &record.after] {
            match recipient {
                AmendedRecipient::Unknown => hash.update([0]),
                AmendedRecipient::Known(reference) => {
                    hash.update([1]);
                    custody_result::reference(
                        &mut hash,
                        reference,
                        stamp.hub,
                        stamp.actor,
                        stamp.catalog_revision,
                    );
                }
            }
        }
        hash_text(&mut hash, record.stamp.basis.as_str());
        length(&mut hash, record.impact.rows().len())?;
        for target in record.impact.rows() {
            let (kind, id) = match *target {
                CorrectionImpact::Population(id) => (0, id),
                CorrectionImpact::Source(id) => (1, id),
                CorrectionImpact::Departure(id) => (2, id),
                CorrectionImpact::IssuedVersion(id) => (3, id),
                CorrectionImpact::Recipient(id) => (4, id),
                CorrectionImpact::UnresolvedSource(id) => (5, id),
                CorrectionImpact::UnresolvedRecipient(id) => (6, id),
            };
            hash.update([kind]);
            hash.update(id.as_bytes());
        }
    }
    length(&mut hash, value.quantities.len())?;
    for account in &value.quantities {
        let original = account.original();
        for id in [
            original.operation,
            original.group,
            original.trace,
            original.variant,
        ] {
            hash.update(id.as_bytes());
        }
        hash.update(original.stock_revision.value().to_be_bytes());
        hash.update(original.packs.value().to_be_bytes());
        length(&mut hash, account.amendments().len())?;
        for change in account.amendments() {
            hash.update(change.operation.as_bytes());
            hash.update(change.stock_revision.value().to_be_bytes());
            hash.update(change.before.value().to_be_bytes());
            hash.update(change.after.value().to_be_bytes());
            let (kind, packs) = match change.delta {
                DepartureQuantityDelta::AdditionalDeparture(n) => (0, n),
                DepartureQuantityDelta::IdentifiedFoundRemainder(n) => (1, n),
            };
            hash.update([kind]);
            hash.update(packs.value().to_be_bytes());
        }
        for packs in [
            account.effective(),
            account.additional_departure(),
            account.found_remainder(),
            account.established_returns(),
        ] {
            hash.update(packs.value().to_be_bytes());
        }
    }
    // No absent marker: every physical20 digest remains byte-for-byte frozen.
    if include_count_snapshot && let Some(value) = value.count_explanation.as_deref() {
        hash_count_interpretation(&mut hash, value)?;
    }
    Ok(hash.finalize().into())
}

fn hash_count_interpretation(
    hash: &mut Sha256,
    value: &ActualKnownCountExplanation,
) -> Result<(), ActualReportError> {
    hash.update(b"harvestcircle.hub.actual.known.count_interpretation.v1\0");
    hash.update(
        crate::hub::counts::count_post_result_digest(&[0; 32], &value.count)
            .map_err(|_| ActualReportError::InvalidHistory)?,
    );
    let account = &value.account;
    let original = account.original();
    for id in [
        original.reconciliation,
        original.operation,
        original.trace,
        original.variant,
    ] {
        hash.update(id.as_bytes());
    }
    hash.update(original.stock_revision.value().to_be_bytes());
    hash.update(original.packs.value().to_be_bytes());
    length(hash, account.explanations().len())?;
    for row in account.explanations() {
        hash.update(row.operation.as_bytes());
        hash.update(row.stock_revision.value().to_be_bytes());
        hash.update(row.packs.value().to_be_bytes());
    }
    length(hash, account.amendments().len())?;
    for row in account.amendments() {
        hash.update(row.operation.as_bytes());
        hash.update(row.stock_revision.value().to_be_bytes());
        hash.update(row.explanation.as_bytes());
        hash.update(row.before.value().to_be_bytes());
        hash.update(row.after.value().to_be_bytes());
    }
    length(hash, account.effective_explanations().len())?;
    for row in account.effective_explanations() {
        hash.update(row.original.operation.as_bytes());
        hash.update(row.packs.value().to_be_bytes());
        hash.update(row.established_returns.value().to_be_bytes());
    }
    hash.update(account.explained().value().to_be_bytes());
    hash.update(account.unexplained().value().to_be_bytes());
    Ok(())
}

pub(super) fn count_inventory_digest(
    value: &ActualKnownInterpretations,
) -> Result<[u8; 32], ActualReportError> {
    use std::collections::BTreeMap;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.known.count_inventory.v1\0");
    hash.update([ACTUAL_KNOWN_COUNT_INVENTORY_SCHEMA]);
    hash.update(value.hub.as_bytes());
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.catalog_revision.value().to_be_bytes());
    hash.update(value.latest_recorded_at.as_seconds().to_be_bytes());
    let mut accounts = BTreeMap::new();
    for record in &value.records {
        if let Some(account) = record.count_explanation.as_deref() {
            accounts.entry(account.count.operation).or_insert(account);
        }
    }
    length(&mut hash, accounts.len())?;
    for (operation, account) in accounts {
        hash.update(operation.as_bytes());
        let mut account_hash = Sha256::new();
        hash_count_interpretation(&mut account_hash, account)?;
        hash.update(account_hash.finalize());
    }
    length(&mut hash, value.records.len())?;
    for record in &value.records {
        hash.update(interpretation_digest_inner(record, false)?);
        optional_id(
            &mut hash,
            record
                .count_explanation
                .as_ref()
                .map(|account| account.count.operation),
        );
    }
    Ok(hash.finalize().into())
}

#[cfg(test)]
mod initial_review_compatibility_tests {
    use super::*;

    #[test]
    fn legacy_treatment_capture_encoding_stays_frozen() {
        let (_, group, graph, original) =
            crate::hub::departure::tests::disposition_policy_fixture();
        let decision = &original.decisions()[0];
        let scope = graph
            .restore_scope(
                HubRevision::from_value(9).unwrap(),
                graph.capture(&[group.id]).unwrap().anchors().to_vec(),
            )
            .unwrap();
        let hold = crate::hub::treatment::Hold {
            stamp: TreatmentStamp {
                id: HubRecordId::from_bytes([217; 16]).unwrap(),
                stock_revision: HubRevision::from_value(10).unwrap(),
                ..decision.stamp.clone()
            },
            scope,
            release: None,
        };
        let state =
            TreatmentState::restore(&graph, original.decisions().to_vec(), vec![hold]).unwrap();
        assert!(!state.decisions().is_empty());
        assert!(!state.holds().is_empty());
        let mut hash = Sha256::new();
        treatment(&mut hash, &state).unwrap();
        assert_eq!(
            format!("{:x}", hash.finalize()),
            "f2a0b6518933b5e9d20ab0d448f37e14c5ee0056d1a4559f974247021451b7f7"
        );
    }

    #[test]
    fn legacy_lineage_capture_encoding_stays_frozen() {
        let (_, _, graph, _) = crate::hub::departure::tests::disposition_policy_fixture();
        let mut hash = Sha256::new();
        lineage(&mut hash, &graph).unwrap();
        let actual = format!("{:x}", hash.finalize());
        assert_eq!(
            actual,
            "c4831fa6c8ded97d830865f55f07607a949e9d6b6901957e5141009cd9916b72"
        );
    }

    #[test]
    fn recovery_origin_capture_binds_debit_identity_and_recorded_cut() {
        let (_, group, graph, _) = crate::hub::departure::tests::disposition_policy_fixture();
        let child = HubRecordId::from_bytes([211; 16]).unwrap();
        let origin = crate::hub::portions::PhysicalRecoveryOrigin {
            debit_operation: HubRecordId::from_bytes([210; 16]).unwrap(),
            debit_revision: HubRevision::from_value(5).unwrap(),
        };
        let seal = |recovery| {
            let mut nodes = graph.nodes().to_vec();
            nodes.push(crate::hub::portions::PhysicalLineageNode {
                id: child,
                parent: Some(group.id),
                trace: graph.trace(),
                variant: graph.variant(),
                created_revision: HubRevision::from_value(11).unwrap(),
                packs: WholePacks::from_value(1).unwrap(),
                recovery,
            });
            let graph = PhysicalLineage::new(
                graph.hub(),
                graph.trace(),
                graph.variant(),
                HubRevision::from_value(11).unwrap(),
                nodes,
            )
            .unwrap();
            let mut hash = Sha256::new();
            lineage(&mut hash, &graph).unwrap();
            hash.finalize()
        };
        let base = seal(Some(origin));
        assert_ne!(base, seal(None));
        assert_ne!(
            base,
            seal(Some(crate::hub::portions::PhysicalRecoveryOrigin {
                debit_operation: child,
                ..origin
            }))
        );
        assert_ne!(
            base,
            seal(Some(crate::hub::portions::PhysicalRecoveryOrigin {
                debit_revision: HubRevision::from_value(6).unwrap(),
                ..origin
            }))
        );
    }
    #[test]
    fn nonempty_initial_review_capture_binds_its_stamp_and_scope_separately() {
        use crate::hub::portions::{PhysicalLineage, PhysicalLineageNode};
        use crate::hub::treatment::InitialGroupReview;
        let (_, group, graph, state) = crate::hub::departure::tests::disposition_policy_fixture();
        let child = HubRecordId::from_bytes([214; 16]).unwrap();
        let mut nodes = graph.nodes().to_vec();
        nodes.iter_mut().find(|n| n.id == group.id).unwrap().packs =
            WholePacks::from_value(5).unwrap();
        nodes.push(PhysicalLineageNode {
            recovery: None,
            id: child,
            parent: Some(group.id),
            trace: graph.trace(),
            variant: graph.variant(),
            created_revision: HubRevision::from_value(11).unwrap(),
            packs: WholePacks::from_value(1).unwrap(),
        });
        let graph = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            HubRevision::from_value(12).unwrap(),
            nodes,
        )
        .unwrap();
        let scope = graph
            .restore_scope(
                HubRevision::from_value(11).unwrap(),
                vec![crate::hub::portions::PhysicalScopeAnchor {
                    group: child,
                    created_revision: HubRevision::from_value(11).unwrap(),
                    captured_packs: WholePacks::from_value(1).unwrap(),
                }],
            )
            .unwrap();
        let review = InitialGroupReview {
            stamp: TreatmentStamp {
                id: HubRecordId::from_bytes([213; 16]).unwrap(),
                stock_revision: HubRevision::from_value(11).unwrap(),
                recorded_at: UnixTimestamp::from_seconds(200).unwrap(),
                occurred: Occurrence::Known(UnixTimestamp::from_seconds(200).unwrap()),
                ..state.decisions()[0].stamp.clone()
            },
            scope,
        };
        let seal = |review: Option<InitialGroupReview>| {
            let value = TreatmentState::restore_with_initial_reviews(
                &graph,
                state.decisions().to_vec(),
                state.holds().to_vec(),
                review.into_iter().collect(),
            )
            .unwrap();
            let mut hash = Sha256::new();
            treatment(&mut hash, &value).unwrap();
            format!("{:x}", hash.finalize())
        };
        let base = seal(Some(review.clone()));
        assert_ne!(base, seal(None));
        let mut changes = Vec::new();
        let mut changed = review.clone();
        changed.stamp.id = HubRecordId::from_bytes([212; 16]).unwrap();
        changes.push(changed);
        let mut changed = review.clone();
        changed.stamp.observation =
            SourceProvenance::new("Different recorded review evidence").unwrap();
        changes.push(changed);
        let mut changed = review.clone();
        changed.stamp.actor =
            PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
                .unwrap();
        changes.push(changed);
        let mut changed = review.clone();
        changed.stamp.recorded_at = UnixTimestamp::from_seconds(201).unwrap();
        changed.stamp.occurred = Occurrence::Known(changed.stamp.recorded_at);
        changes.push(changed);
        let mut anchors = review.scope.anchors().to_vec();
        anchors[0].captured_packs = WholePacks::from_value(2).unwrap();
        let mut changed = review;
        changed.scope = graph
            .restore_scope(HubRevision::from_value(11).unwrap(), anchors)
            .unwrap();
        changes.push(changed);
        for changed in changes {
            assert_ne!(base, seal(Some(changed)));
        }
    }
}
