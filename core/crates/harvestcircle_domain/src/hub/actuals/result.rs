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
    if command.fingerprint(context.review.hub, record.actor, record.operation) != *input
        || command.plan(context, record.operation, record.actor, record.recorded_at)? != *record
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
    Ok(hash.finalize().into())
}

pub fn actual_duplicate_result_digest(
    input: &[u8; 32],
    command: &ActualDuplicateLink,
    record: &ActualDuplicateLinkPlan,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<[u8; 32], ActualReportError> {
    if command.fingerprint(record.original.hub, record.actor, record.operation) != *input
        || command.plan(
            &record.original,
            &record.known,
            None,
            record.operation,
            record.actor,
            record.recorded_at,
        )? != *record
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
    Ok(())
}
fn birth(hash: &mut Sha256, value: &ActualPhysicalBirth) {
    hash.update([match value.kind {
        ActualPhysicalBirthKind::Receipt => 0,
        ActualPhysicalBirthKind::CountedOpening => 1,
        ActualPhysicalBirthKind::Returned => 2,
        ActualPhysicalBirthKind::CountedSurplus => 3,
        ActualPhysicalBirthKind::ObservedPortion => 4,
    }]);
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

fn hash_source(
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
