use super::*;
use crate::hub::movement::{PhysicalStockGroup, WorkProgress};
use crate::hub::portions::PhysicalLineage;
use crate::hub::reference::{ReferenceCatalog, ReferenceKind};
use crate::hub::treatment::{Hold, TreatmentStamp, TreatmentState};

mod hold_scope;
mod interpretation;
mod reconcile;
mod result;
mod source;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn known(n: i64) -> Occurrence {
    Occurrence::Known(at(n))
}
fn interval(a: i64, b: i64) -> Occurrence {
    Occurrence::Interval {
        earliest: at(a),
        latest: at(b),
    }
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn report() -> ActualReportCommand {
    ActualReportCommand {
        variant: None,
        recipient: None,
        packs: Some(packs(2)),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note(
            "Two packs already left; original witness cannot yet identify source or time",
        ),
    }
}

struct Fixture {
    catalog: ReferenceCatalog,
    group: PhysicalStockGroup,
    graph: PhysicalLineage,
    state: TreatmentState,
    original: ActualReportRecord,
    facts: ActualReviewFacts,
}
impl Fixture {
    fn new(occurred: Occurrence) -> Self {
        let (catalog, group, graph, state) =
            crate::hub::departure::tests::disposition_policy_fixture();
        let mut command = report();
        command.occurred = occurred;
        let original = command
            .preserve(
                graph.hub(),
                graph.revision(),
                catalog.revision(),
                id(180),
                owner(),
                at(150),
            )
            .unwrap();
        let graph = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            original.stock_revision,
            graph.nodes().to_vec(),
        )
        .unwrap();
        let recipient = catalog
            .records()
            .find(|r| r.value.kind() == ReferenceKind::Party)
            .unwrap()
            .id;
        let facts = ActualReviewFacts {
            report: original.id,
            expected_stock_revision: graph.revision(),
            expected_catalog_revision: catalog.revision(),
            group: group.id,
            expected_group_revision: group.revision,
            packs: packs(2),
            recipient,
            occurred,
            source_evidence: note("Original witness verifies this identified original stock group"),
            quantity_evidence: note("Witness counted two whole packs leaving"),
            recipient_evidence: note("Named recipient confirmed against the manual original"),
            timing_evidence: note("Occurrence certainty retained independently of recording time"),
        };
        Self {
            catalog,
            group,
            graph,
            state,
            original,
            facts,
        }
    }
    fn context(&self) -> ActualReviewContext<'_> {
        ActualReviewContext {
            hub: self.graph.hub(),
            stock_revision: self.graph.revision(),
            catalog: &self.catalog,
            original: &self.original,
            group: &self.group,
            lineage: &self.graph,
            treatment: &self.state,
            uncertain_submissions: false,
        }
    }
    fn hold(&mut self, effective: Occurrence, release: Option<Occurrence>) {
        let scope = self.graph.capture(&[self.group.id]).unwrap();
        let revision = HubRevision::from_value(self.graph.revision().value() + 1).unwrap();
        let stamp = TreatmentStamp {
            id: id(190),
            stock_revision: revision,
            actor: owner(),
            basis: self.catalog.handling_basis(),
            recorded_at: at(155),
            occurred: effective,
            observation: note("Independent recorded restriction with original effective timing"),
        };
        let mut current = revision;
        let release = release.map(|occurred| {
            current = HubRevision::from_value(revision.value() + 1).unwrap();
            TreatmentStamp {
                id: id(191),
                stock_revision: current,
                recorded_at: at(160),
                occurred,
                ..stamp.clone()
            }
        });
        self.graph = PhysicalLineage::new(
            self.graph.hub(),
            self.graph.trace(),
            self.graph.variant(),
            current,
            self.graph.nodes().to_vec(),
        )
        .unwrap();
        self.state = TreatmentState::restore(
            &self.graph,
            self.state.decisions().to_vec(),
            vec![Hold {
                stamp,
                scope,
                release,
            }],
        )
        .unwrap();
        self.facts.expected_stock_revision = current;
    }
    fn review(&self) -> Result<ActualReviewComparison, ActualReportError> {
        self.facts.review(&self.context(), at(170))
    }
}

#[test]
fn unknown_and_over_balance_actual_reports_are_preserved_without_physical_effects() {
    let f = Fixture::new(Occurrence::Unknown);
    let mut input = report();
    input.packs = None;
    let saved = input
        .preserve(
            f.graph.hub(),
            f.graph.revision(),
            f.catalog.revision(),
            id(181),
            owner(),
            at(160),
        )
        .unwrap();
    assert_eq!(saved.report, input);
    input.packs = Some(packs(1_000));
    let large = input
        .preserve(
            f.graph.hub(),
            f.graph.revision(),
            f.catalog.revision(),
            id(181),
            owner(),
            at(160),
        )
        .unwrap();
    assert_eq!(large.report.packs, Some(packs(1_000)));
    assert_eq!(f.group.packs, packs(6));
    assert_eq!(f.group.work[0].progress, WorkProgress::Pending);
    assert_ne!(
        saved
            .report
            .fingerprint(saved.hub, owner(), saved.operation),
        large
            .report
            .fingerprint(large.hub, owner(), large.operation)
    );
}

#[test]
fn source_claims_keep_every_bounded_candidate_and_never_pick_one() {
    let first = ReportedActualSource {
        trace: id(1),
        group: None,
        location: None,
    };
    let second = ReportedActualSource {
        trace: id(2),
        group: Some(id(3)),
        location: Some(id(4)),
    };
    let mut input = report();
    input.sources = ActualReportSources::Possible(vec![first, second]);
    input.validate().unwrap();
    assert_eq!(input.sources.candidates(), &[first, second]);
    input.sources = ActualReportSources::Possible(vec![first, first]);
    assert_eq!(input.validate(), Err(ActualReportError::InvalidInput));
    input.sources = ActualReportSources::Possible(Vec::new());
    assert_eq!(input.validate(), Err(ActualReportError::InvalidInput));
    input.sources = ActualReportSources::Possible(
        (1..=65)
            .map(|n| ReportedActualSource {
                trace: id(n),
                group: None,
                location: None,
            })
            .collect(),
    );
    assert_eq!(input.validate(), Err(ActualReportError::Capacity));
}

#[test]
fn report_bounds_and_original_identity_fail_closed_without_losing_valid_unknowns() {
    let f = Fixture::new(Occurrence::Unknown);
    let mut input = report();
    input.packs = Some(packs(0));
    assert_eq!(input.validate(), Err(ActualReportError::InvalidInput));
    input.packs = None;
    input.occurred = known(171);
    input.validate().unwrap();
    let preserved = input
        .preserve(
            f.graph.hub(),
            f.graph.revision(),
            f.catalog.revision(),
            id(183),
            owner(),
            at(170),
        )
        .unwrap();
    assert_eq!(preserved.report.occurred, known(171));
    assert_eq!(
        preserved.recording_clock_assessment(),
        ActualReportedClockAssessment::RecordingClockConflict
    );
    input.occurred = interval(169, 168);
    assert_eq!(input.validate(), Err(ActualReportError::InvalidInput));
    let mut corrupt = f.original.clone();
    corrupt.id = id(182);
    assert_eq!(corrupt.validate(), Err(ActualReportError::InvalidHistory));
    assert_ne!(actual_report_id(id(180)).unwrap(), id(180));
    assert_eq!(actual_report_id(id(180)), actual_report_id(id(180)));
}

#[test]
fn hold_occurrence_evidence_distinguishes_breach_before_and_unknown_without_permission() {
    use HistoricalHoldTiming::*;
    for (departure, effective, released, expected) in [
        (known(105), known(100), None, EstablishedBreach),
        (known(95), known(100), None, BeforeHold),
        (interval(90, 110), known(100), None, TimingUnknown),
        (Occurrence::Unknown, known(100), None, TimingUnknown),
        (known(105), Occurrence::Unknown, None, TimingUnknown),
        (known(100), known(100), None, EstablishedBreach),
        (known(110), known(100), Some(known(110)), AfterRelease),
        (
            known(105),
            known(100),
            Some(Occurrence::Unknown),
            TimingUnknown,
        ),
        (
            known(105),
            interval(90, 100),
            Some(interval(110, 120)),
            EstablishedBreach,
        ),
        (
            interval(105, 115),
            known(100),
            Some(interval(110, 120)),
            TimingUnknown,
        ),
        (
            known(125),
            known(100),
            Some(interval(110, 120)),
            AfterRelease,
        ),
        (interval(110, 90), known(100), None, TimingUnknown),
    ] {
        assert_eq!(
            classify_hold_timing(departure, effective, released),
            expected
        );
    }
}

#[test]
fn review_preserves_the_exact_original_and_current_hold_for_all_three_timing_worlds() {
    for (occurred, expected) in [
        (known(105), HistoricalHoldTiming::EstablishedBreach),
        (known(95), HistoricalHoldTiming::BeforeHold),
        (interval(90, 110), HistoricalHoldTiming::TimingUnknown),
    ] {
        let mut f = Fixture::new(occurred);
        f.hold(known(100), None);
        let original = f.original.clone();
        let hold = f.state.holds()[0].clone();
        let review = f.review().unwrap();
        assert_eq!(review.original, original);
        assert_eq!(review.potential_historical_holds[0].original, hold);
        assert_eq!(
            review.potential_historical_holds[0].conditional_timing,
            expected
        );
        assert_eq!(review.scope.compared_departure, packs(2));
        assert_eq!(review.scope.on_hand_after, packs(4));
        assert_eq!(
            review
                .scope
                .compared_departure
                .checked_add(review.scope.on_hand_after)
                .unwrap(),
            f.group.packs
        );
        assert_eq!(f.state.holds()[0], hold);
        assert!(f.state.holds()[0].release.is_none());
        assert_eq!(f.group.packs, packs(6));
    }
}

#[test]
fn current_release_does_not_hide_an_established_earlier_breach() {
    let mut f = Fixture::new(known(105));
    f.hold(known(100), Some(known(120)));
    assert_eq!(
        f.review().unwrap().potential_historical_holds[0].conditional_timing,
        HistoricalHoldTiming::EstablishedBreach
    );
    f.facts.occurred = known(125);
    assert_eq!(f.review(), Err(ActualReportError::ConflictingEvidence));
}

#[test]
fn contradictory_restriction_history_never_produces_a_definitive_nonbreach() {
    for occurred in [known(125), known(140), interval(120, 145)] {
        for (effective, release) in [
            (known(130), known(110)),
            (interval(130, 135), interval(100, 125)),
        ] {
            assert_eq!(
                classify_hold_timing(occurred, effective, Some(release)),
                HistoricalHoldTiming::TimingUnknown,
            );
        }
    }
}

#[test]
fn resolving_source_and_time_keeps_original_unknowns_and_requires_supported_claim_scope() {
    let mut f = Fixture::new(Occurrence::Unknown);
    f.facts.occurred = known(105);
    let review = f.review().unwrap();
    assert_eq!(review.original.report.sources, ActualReportSources::Unknown);
    assert_eq!(review.original.report.occurred, Occurrence::Unknown);
    assert_eq!(review.facts.occurred, known(105));
    assert_eq!(
        review.source_comparison,
        ActualSourceComparison::UnknownClaim
    );
    let supported = ReportedActualSource {
        trace: f.group.trace,
        group: None,
        location: None,
    };
    f.original.report.sources = ActualReportSources::Possible(vec![
        supported,
        ReportedActualSource {
            trace: id(160),
            group: None,
            location: None,
        },
    ]);
    assert_eq!(
        f.review().unwrap().source_comparison,
        ActualSourceComparison::CurrentCandidateRequiresHistoricalVerification
    );
    f.original.report.sources = ActualReportSources::EstablishedClaim(ReportedActualSource {
        trace: id(160),
        group: None,
        location: None,
    });
    assert_eq!(
        f.review().unwrap().source_comparison,
        ActualSourceComparison::OutsideReportedCandidates
    );
}

#[test]
fn historical_location_and_ancestor_claims_remain_unresolved_separate_from_current_balance() {
    let mut f = Fixture::new(known(105));
    let old_location = f
        .catalog
        .records()
        .find(|r| r.value.kind() == ReferenceKind::Location && r.id != f.group.location)
        .unwrap()
        .id;
    for source in [
        ReportedActualSource {
            trace: f.group.trace,
            group: Some(f.group.id),
            location: Some(old_location),
        },
        ReportedActualSource {
            trace: f.group.trace,
            group: f.group.parent,
            location: Some(f.group.location),
        },
    ] {
        assert!(source.group != Some(f.group.id) || source.location != Some(f.group.location));
        f.original.report.sources = ActualReportSources::EstablishedClaim(source);
        let comparison = f.review().unwrap();
        assert_eq!(
            comparison.source_comparison,
            ActualSourceComparison::HistoricalAssociationUnresolved
        );
        assert_eq!(comparison.original.report.sources.candidates(), &[source]);
        assert_eq!(comparison.scope.current_location.id, f.group.location);
        assert_eq!(comparison.scope.before, f.group);
        assert_eq!(f.group.packs, packs(6));
    }
}

#[test]
fn review_cannot_invent_balance_change_reported_facts_or_assert_future_occurrence() {
    let mut f = Fixture::new(known(105));
    f.facts.packs = packs(7);
    assert_eq!(f.review(), Err(ActualReportError::InsufficientBalance));
    f.facts.packs = packs(3);
    assert_eq!(f.review(), Err(ActualReportError::ConflictingEvidence));
    f.facts.packs = packs(2);
    f.original.report.recipient = Some(id(161));
    assert_eq!(f.review(), Err(ActualReportError::ConflictingEvidence));
    f.original.report.recipient = None;
    f.original.report.occurred = Occurrence::Unknown;
    f.facts.occurred = known(160);
    assert_eq!(f.review(), Err(ActualReportError::InvalidInput));
}

#[test]
fn review_rejects_stale_scope_mismatched_native_world_and_unresolved_submission() {
    let mut f = Fixture::new(known(105));
    f.facts.expected_stock_revision = HubRevision::INITIAL;
    assert_eq!(f.review(), Err(ActualReportError::StaleRevision));
    f.facts.expected_stock_revision = f.graph.revision();
    let mut context = f.context();
    context.uncertain_submissions = true;
    assert_eq!(
        f.facts.review(&context, at(170)),
        Err(ActualReportError::UncertainSubmission)
    );
    let mut context = f.context();
    context.hub = HubId::from_bytes([200; 16]).unwrap();
    assert_eq!(
        f.facts.review(&context, at(170)),
        Err(ActualReportError::InvalidNativeContext)
    );
    assert_eq!(
        f.facts.review(&f.context(), at(149)),
        Err(ActualReportError::InvalidNativeContext)
    );
}

#[test]
fn future_claim_is_preserved_as_recording_clock_conflict_without_physical_review_permission() {
    let mut f = Fixture::new(known(160));
    assert_eq!(f.original.recorded_at, at(150));
    assert_eq!(f.original.report.occurred, known(160));
    assert_eq!(
        f.original.recording_clock_assessment(),
        ActualReportedClockAssessment::RecordingClockConflict
    );
    f.original.validate().unwrap();
    assert_eq!(f.review(), Err(ActualReportError::InvalidInput));
    f.facts.occurred = known(140);
    assert_eq!(f.review(), Err(ActualReportError::ConflictingEvidence));
    assert_eq!(f.original.report.occurred, known(160));
    f.original.report.occurred = interval(160, 180);
    assert_eq!(f.review(), Err(ActualReportError::ConflictingEvidence));
    f.facts.occurred = known(165);
    assert_eq!(f.review(), Err(ActualReportError::InvalidInput));
}

#[test]
fn crossing_claim_interval_can_refine_to_verified_past_time_without_rewriting_original_warning() {
    let mut f = Fixture::new(known(145));
    f.original.report.occurred = interval(140, 160);
    f.original.validate().unwrap();
    let comparison = f.review().unwrap();
    assert_eq!(comparison.original.report.occurred, interval(140, 160));
    assert_eq!(comparison.original.recorded_at, at(150));
    assert_eq!(comparison.facts.occurred, known(145));
    assert_eq!(
        comparison.reported_clock,
        ActualReportedClockAssessment::RecordingClockConflict
    );
    f.facts.occurred = known(155);
    assert_eq!(f.review(), Err(ActualReportError::InvalidInput));
}

#[test]
fn exact_existing_departure_can_link_a_clock_conflicting_report_without_repeating_stock() {
    let f = Fixture::new(known(160));
    let known = ActualKnownDeparture {
        operation: id(240),
        actor: owner(),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(f.original.stock_revision.value() - 1).unwrap(),
        catalog_revision: f.catalog.revision(),
        trace: f.graph.trace(),
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(f.facts.recipient).unwrap().clone(),
        occurred: super::Occurrence::Known(at(160)),
        recorded_at: at(165),
        lines: vec![ActualKnownDepartureLine {
            group: f.group.id,
            location: f.catalog.get(f.group.location).unwrap().clone(),
            packs: packs(2),
        }],
    };
    let command = ActualDuplicateLink {
        report: f.original.id,
        known_operation: known.operation,
        known_trace: known.trace,
        known_variant: known.variant.id,
        verification: note(
            "Independent witness identifies this exact already recorded physical occurrence despite the recorder clock conflict",
        ),
    };
    let plan = command
        .plan(&f.original, &known, None, id(241), owner(), at(170))
        .unwrap();
    assert_eq!(plan.original.report.occurred, Occurrence::Known(at(160)));
    assert_eq!(
        plan.original.recording_clock_assessment(),
        ActualReportedClockAssessment::RecordingClockConflict
    );
    assert_eq!(plan.known, known);
    assert_eq!(plan.original, f.original);
}
