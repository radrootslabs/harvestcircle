use super::*;
use crate::hub::actuals::{
    ActualKnownDepartureLine, ActualReportCommand, ActualReportSources, HistoricalActualSource,
    ReportedActualSource,
};
use crate::hub::movement::PhysicalStockGroup;
use crate::hub::portions::PhysicalLineage;
use crate::hub::stock::QuantityWork;
fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent original witness and complete native records checked")
        .unwrap()
}
#[derive(Clone)]
struct OwnedKnown {
    original: ActualKnownDeparture,
    recipient: OriginalRecipientAttribution,
    history: Vec<RecipientAttributionCorrection>,
    lines: Vec<DepartureQuantityAccount>,
}
impl OwnedKnown {
    fn new(original: ActualKnownDeparture) -> Self {
        let recipient = OriginalRecipientAttribution {
            hub: original.hub,
            family: CorrectedDepartureFamily::ReviewedActual,
            operation: original.operation,
            stock_revision: original.stock_revision,
            catalog_revision: original.catalog_revision,
            recorded_at: original.recorded_at,
            actor: original.actor,
            recipient: original.recipient.clone(),
            populations: vec![original.trace],
            issued_versions: vec![],
        };
        let lines = original
            .lines
            .iter()
            .map(|l| {
                DepartureQuantityAccount::restore(
                    OriginalDepartureQuantity {
                        operation: original.operation,
                        group: l.group,
                        trace: original.trace,
                        variant: original.variant.id,
                        stock_revision: original.stock_revision,
                        packs: l.packs,
                    },
                    vec![],
                    packs(0),
                )
                .unwrap()
            })
            .collect();
        Self {
            original,
            recipient,
            history: vec![],
            lines,
        }
    }
    fn evidence(&self) -> CorrectionKnownDeparture<'_> {
        CorrectionKnownDeparture {
            original: &self.original,
            recipient_original: &self.recipient,
            recipient_history: &self.history,
            quantity: KnownCorrectionQuantity::Physical(&self.lines),
        }
    }
}
struct Fixture {
    catalog: ReferenceCatalog,
    group: PhysicalStockGroup,
    graph: PhysicalLineage,
    treatment: TreatmentState,
    original: ActualKnownDeparture,
    target: DepartureQuantityAccount,
    recipient: OriginalRecipientAttribution,
    source: HistoricalActualSource,
    report: ActualReportRecord,
    known: Vec<OwnedKnown>,
    history: Vec<RecipientAttributionCorrection>,
    latest: UnixTimestamp,
}
impl Fixture {
    fn new() -> Self {
        let (catalog, mut group, graph, _) =
            crate::hub::departure::tests::disposition_policy_fixture();
        group.revision = rev(2);
        group.work[0].requirement.quantity =
            QuantityWork::restore(packs(20), packs(0), packs(6), packs(14)).unwrap();
        let mut source_group = group.clone();
        source_group.revision = rev(1);
        source_group.packs = packs(12);
        let source = HistoricalActualSource {
            operation: id(230),
            actor: owner(),
            hub: graph.hub(),
            stock_revision: rev(3),
            catalog_revision: catalog.revision(),
            group: (&source_group).into(),
            location: catalog.get(group.location).unwrap().clone(),
            occurred: Occurrence::Known(at(100)),
            recorded_at: at(100),
        };
        let original = ActualKnownDeparture {
            operation: id(220),
            actor: owner(),
            hub: graph.hub(),
            stock_revision: rev(6),
            catalog_revision: catalog.revision(),
            trace: group.trace,
            variant: catalog.get(group.variant).unwrap().clone(),
            recipient: catalog.get(id(4)).unwrap().clone(),
            occurred: Occurrence::Known(at(110)),
            recorded_at: at(120),
            lines: vec![ActualKnownDepartureLine {
                group: group.id,
                location: source.location.clone(),
                packs: packs(6),
            }],
        };
        let target = DepartureQuantityAccount::restore(
            OriginalDepartureQuantity {
                operation: original.operation,
                group: group.id,
                trace: group.trace,
                variant: group.variant,
                stock_revision: original.stock_revision,
                packs: packs(6),
            },
            vec![],
            packs(0),
        )
        .unwrap();
        let recipient = OriginalRecipientAttribution {
            hub: graph.hub(),
            family: CorrectedDepartureFamily::ReviewedActual,
            operation: original.operation,
            stock_revision: original.stock_revision,
            catalog_revision: catalog.revision(),
            recorded_at: original.recorded_at,
            actor: owner(),
            recipient: original.recipient.clone(),
            populations: vec![group.trace],
            issued_versions: vec![],
        };
        let report = ActualReportCommand {
            variant: Some(group.variant),
            recipient: Some(id(4)),
            packs: Some(packs(8)),
            sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                trace: group.trace,
                group: Some(group.id),
                location: Some(group.location),
            }),
            occurred: Occurrence::Known(at(110)),
            observation: note(),
        }
        .preserve(
            graph.hub(),
            graph.revision(),
            catalog.revision(),
            id(240),
            owner(),
            at(150),
        )
        .unwrap();
        let mut nodes = graph.nodes().to_vec();
        nodes.iter_mut().find(|n| n.parent.is_none()).unwrap().packs = packs(8);
        let graph = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            report.stock_revision,
            nodes,
        )
        .unwrap();
        Self {
            catalog,
            group,
            graph,
            treatment: TreatmentState::default(),
            known: vec![OwnedKnown::new(original.clone())],
            history: vec![],
            latest: at(150),
            original,
            target,
            recipient,
            source,
            report,
        }
    }
    fn command(&self) -> AdditionalQuantityCommand {
        AdditionalQuantityCommand {
            facts: CorrectedLineFacts {
                report: self.report.id,
                target_operation: self.original.operation,
                target_group: self.group.id,
                expected_stock_revision: self.graph.revision(),
                expected_catalog_revision: self.catalog.revision(),
                expected_prior: self.target.amendments().last().map(|a| a.operation),
                corrected_line_packs: packs(8),
                recipient: match current_recipient_attribution(&self.recipient, &self.history)
                    .unwrap()
                {
                    AmendedRecipient::Known(r) => Some(r.id),
                    AmendedRecipient::Unknown => None,
                },
                occurred: Occurrence::Known(at(110)),
                source_evidence: note(),
                quantity_evidence: note(),
                recipient_evidence: note(),
                timing_evidence: note(),
            },
            source_operation: self.source.operation,
            source_group: self.source.group.id,
            expected_balancing_revision: self.group.revision,
            duplicate_review_basis: note(),
        }
    }
    fn context(&self) -> AdditionalQuantityContext<'_> {
        AdditionalQuantityContext {
            hub: self.graph.hub(),
            stock_revision: self.graph.revision(),
            latest_recorded_at: self.latest,
            catalog: &self.catalog,
            family: self.recipient.family,
            original: &self.original,
            target: &self.target,
            recipient_original: &self.recipient,
            recipient_history: &self.history,
            report: &self.report,
            prior_report_review: None,
            historical_source: ActualSourceContext {
                source: &self.source,
                lineage: &self.graph,
                balancing_group: &self.group,
                subsequent_activity: &[],
            },
            treatment: &self.treatment,
            known_departures: self.known.iter().map(OwnedKnown::evidence).collect(),
            uncertain_submissions: false,
        }
    }
    fn plan(&self) -> Result<AdditionalQuantityPlan, QuantityEffectError> {
        self.command()
            .plan(&self.context(), id(250), owner(), at(170))
    }
}
#[test]
fn corrected_eight_preserves_raw_total_and_debits_only_two() {
    let f = Fixture::new();
    let p = f.plan().unwrap();
    assert_eq!(p.report.report.packs, Some(packs(8)));
    assert_eq!(p.original.lines[0].packs, packs(6));
    assert_eq!(p.amendment.before, packs(6));
    assert_eq!(p.amendment.after, packs(8));
    assert_eq!(p.reduction.actual_departed, packs(2));
    assert_eq!(p.reduction.on_hand_after, packs(4));
    assert_eq!(
        p.reduction.work_changes[0]
            .after
            .quantity
            .no_longer_required(),
        packs(8)
    );
    assert_eq!(
        p.reduction.work_changes[0].after.quantity.remaining(),
        packs(12)
    );
    assert_eq!(f.group.packs, packs(6));
    assert_eq!(f.report.report.packs, Some(packs(8)));
}
#[test]
fn raw_delta_report_does_not_masquerade_as_the_corrected_line_total() {
    let mut f = Fixture::new();
    f.report.report.packs = Some(packs(2));
    assert_eq!(f.plan(), Err(QuantityEffectError::InvalidFacts));
}
#[test]
fn stale_heads_raw_claims_and_wrong_original_roles_fail_without_effect() {
    let f = Fixture::new();
    for field in 0..8 {
        let mut c = f.command();
        match field {
            0 => c.facts.expected_stock_revision = rev(1),
            1 => c.facts.expected_catalog_revision = rev(1),
            2 => c.facts.expected_prior = Some(id(252)),
            3 => c.facts.target_operation = id(253),
            4 => c.facts.target_group = id(254),
            5 => c.facts.recipient = Some(id(5)),
            6 => c.source_group = id(255),
            _ => c.expected_balancing_revision = rev(99),
        }
        assert!(c.plan(&f.context(), id(250), owner(), at(170)).is_err());
    }
    let mut context = f.context();
    context.uncertain_submissions = true;
    assert_eq!(
        f.command().plan(&context, id(250), owner(), at(170)),
        Err(QuantityEffectError::UncertainSubmission)
    );
    context.uncertain_submissions = false;
    context.prior_report_review = Some(id(201));
    assert_eq!(
        f.command().plan(&context, id(250), owner(), at(170)),
        Err(QuantityEffectError::AlreadyReviewed)
    );
}
#[test]
fn corrected_total_and_evidence_cannot_invent_stock_or_a_different_event() {
    let mut f = Fixture::new();
    f.report.report.packs = Some(packs(13));
    let mut c = f.command();
    c.facts.corrected_line_packs = packs(13);
    assert_eq!(
        c.plan(&f.context(), id(250), owner(), at(170)),
        Err(QuantityEffectError::Actual(
            ActualReportError::InsufficientBalance
        ))
    );
    f.report.report.packs = Some(packs(8));
    c = f.command();
    c.facts.occurred = Occurrence::Known(at(111));
    assert_eq!(
        c.plan(&f.context(), id(250), owner(), at(170)),
        Err(QuantityEffectError::InvalidFacts)
    );
}
#[test]
fn only_exact_target_event_is_exempt_from_complete_duplicate_matching() {
    let mut f = Fixture::new();
    assert!(f.plan().is_ok());
    let mut duplicate = f.original.clone();
    duplicate.operation = id(221);
    f.known.push(OwnedKnown::new(duplicate));
    assert_eq!(
        f.plan(),
        Err(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
    f.known.clear();
    assert_eq!(f.plan(), Err(QuantityEffectError::InvalidHistory));
}
#[test]
fn explained_count_event_is_never_an_additional_physical_debit() {
    let f = Fixture::new();
    let mut context = f.context();
    context.family = CorrectedDepartureFamily::ExplainedCountLoss;
    assert_eq!(
        f.command().plan(&context, id(250), owner(), at(170)),
        Err(QuantityEffectError::UnsupportedFamily)
    );
}

#[test]
fn additional_fact_keeps_completed_work_and_later_native_floor() {
    let mut f = Fixture::new();
    f.group.work[0].progress = crate::hub::movement::WorkProgress::Completed;
    f.group.work[0].requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    f.group.causal_floor = Some(at(140));
    let p = f.plan().unwrap();
    assert!(p.reduction.work_changes.is_empty());
    assert_eq!(
        p.reduction.removed_work_scope[0].removed_progress,
        crate::hub::movement::WorkProgress::Completed
    );
    assert_eq!(p.after_causal_floor, Some(at(140)));
    assert_eq!(f.group.work[0].requirement.quantity.completed(), packs(20));
}
#[test]
fn current_restriction_is_retained_without_claiming_historical_clearance() {
    use crate::hub::portions::PhysicalScopeAnchor;
    use crate::hub::treatment::{Hold, TreatmentStamp};
    let mut f = Fixture::new();
    let hold = Hold {
        stamp: TreatmentStamp {
            id: id(232),
            stock_revision: rev(4),
            actor: owner(),
            basis: f.catalog.handling_basis(),
            recorded_at: at(105),
            occurred: Occurrence::Known(at(105)),
            observation: note(),
        },
        scope: f
            .graph
            .restore_scope(
                rev(3),
                vec![PhysicalScopeAnchor {
                    group: f.group.id,
                    created_revision: rev(3),
                    captured_packs: packs(12),
                }],
            )
            .unwrap(),
        release: None,
    };
    f.treatment = TreatmentState::restore(&f.graph, vec![], vec![hold.clone()]).unwrap();
    let p = f.plan().unwrap();
    assert_eq!(p.potential_historical_holds.len(), 1);
    assert_eq!(p.potential_historical_holds[0].original, hold);
    assert_eq!(
        p.potential_historical_holds[0].conditional_timing,
        crate::hub::actuals::HistoricalHoldTiming::EstablishedBreach
    );
    assert_eq!(
        p.potential_historical_holds[0].applicability,
        HistoricalHoldApplicability::HistoricalScopeUnverified
    );
    assert!(f.treatment.holds()[0].release.is_none());
}
#[test]
fn later_count_and_unverified_source_time_cannot_be_ignored_for_another_debit() {
    use crate::hub::actuals::{ActualPhysicalActivity, ActualPhysicalActivityKind};
    let mut f = Fixture::new();
    let activity = [ActualPhysicalActivity {
        sequence: 1,
        operation: id(200),
        actor: owner(),
        stock_revision: rev(9),
        hub: f.graph.hub(),
        trace: f.group.trace,
        group: f.group.id,
        location: None,
        kind: ActualPhysicalActivityKind::CountReconciled,
        occurred: Occurrence::Known(at(125)),
        recorded_at: at(130),
    }];
    let mut context = f.context();
    context.historical_source.subsequent_activity = &activity;
    assert_eq!(
        f.command().plan(&context, id(250), owner(), at(170)),
        Err(QuantityEffectError::Actual(
            ActualReportError::HistoricalSourceUnresolved
        ))
    );
    f.source.occurred = Occurrence::Unknown;
    assert_eq!(
        f.plan(),
        Err(QuantityEffectError::Actual(
            ActualReportError::HistoricalSourceUnresolved
        ))
    );
}
#[test]
fn native_cut_clocks_cannot_be_reconstructed_from_future_source_or_hold_records() {
    let mut f = Fixture::new();
    f.source.recorded_at = at(160);
    assert_eq!(f.plan(), Err(QuantityEffectError::InvalidScope));
}

#[test]
fn prior_whole_event_recipient_amendment_does_not_replace_the_immutable_target() {
    for recipient in [None, Some(id(5))] {
        let mut f = Fixture::new();
        let change = RecipientAttributionCommand {
            departure_operation: f.original.operation,
            expected_stock_revision: f.graph.revision(),
            expected_catalog_revision: f.catalog.revision(),
            expected_prior: None,
            recipient,
            basis: note(),
        }
        .plan(
            &RecipientAttributionContext {
                cut: AttributionCut {
                    hub: f.graph.hub(),
                    stock_revision: f.graph.revision(),
                    catalog: &f.catalog,
                    latest_recorded_at: f.latest,
                    uncertain_submissions: false,
                },
                original: &f.recipient,
                history: &[],
            },
            id(241),
            owner(),
            at(160),
        )
        .unwrap();
        f.graph = PhysicalLineage::new(
            f.graph.hub(),
            f.graph.trace(),
            f.graph.variant(),
            change.stamp.stock_revision,
            f.graph.nodes().to_vec(),
        )
        .unwrap();
        f.history.push(change);
        f.known[0].history = f.history.clone();
        f.report = ActualReportCommand {
            recipient,
            ..f.report.report.clone()
        }
        .preserve(
            f.graph.hub(),
            f.graph.revision(),
            f.catalog.revision(),
            id(242),
            owner(),
            at(165),
        )
        .unwrap();
        f.graph = PhysicalLineage::new(
            f.graph.hub(),
            f.graph.trace(),
            f.graph.variant(),
            f.report.stock_revision,
            f.graph.nodes().to_vec(),
        )
        .unwrap();
        f.latest = at(165);
        let p = f.plan().unwrap();
        assert_eq!(p.original, f.original);
        assert_eq!(p.facts.recipient, recipient);
        assert_eq!(p.reduction.actual_departed, packs(2));
    }
}
#[test]
fn prior_quantity_head_matches_immutable_original_and_effective_target_separately() {
    let mut f = Fixture::new();
    let amendment = DepartureQuantityAmendment {
        operation: id(221),
        stock_revision: rev(10),
        before: packs(6),
        after: packs(8),
        delta: DepartureQuantityDelta::AdditionalDeparture(packs(2)),
    };
    f.target =
        DepartureQuantityAccount::restore(f.target.original(), vec![amendment], packs(0)).unwrap();
    f.known[0].lines[0] = f.target.clone();
    f.group.packs = packs(4);
    f.group.revision = rev(3);
    f.group.work[0].requirement.quantity =
        QuantityWork::restore(packs(20), packs(0), packs(8), packs(12)).unwrap();
    let mut nodes = f.graph.nodes().to_vec();
    nodes.iter_mut().find(|n| n.id == f.group.id).unwrap().packs = packs(4);
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        f.graph.revision(),
        nodes,
    )
    .unwrap();
    f.report.report.packs = Some(packs(10));
    let mut command = f.command();
    command.facts.corrected_line_packs = packs(10);
    let p = command
        .plan(&f.context(), id(250), owner(), at(170))
        .unwrap();
    assert_eq!(p.original.lines[0].packs, packs(6));
    assert_eq!(p.amendment.before, packs(8));
    assert_eq!(p.reduction.actual_departed, packs(2));
    f.known[0].lines[0] =
        DepartureQuantityAccount::restore(f.target.original(), vec![], packs(0)).unwrap();
    assert_eq!(
        command.plan(&f.context(), id(250), owner(), at(170)),
        Err(QuantityEffectError::InvalidHistory)
    );
}
#[test]
fn another_zero_effective_event_with_unknown_recipient_remains_a_duplicate_candidate() {
    let mut f = Fixture::new();
    let mut original = f.original.clone();
    original.operation = id(221);
    original.stock_revision = rev(7);
    original.recorded_at = at(125);
    original.recipient = f.catalog.get(id(5)).unwrap().clone();
    let mut other = OwnedKnown::new(original);
    other.lines[0] = DepartureQuantityAccount::restore(
        other.lines[0].original(),
        vec![DepartureQuantityAmendment {
            operation: id(222),
            stock_revision: rev(8),
            before: packs(6),
            after: packs(0),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(6)),
        }],
        packs(0),
    )
    .unwrap();
    let record = RecipientAttributionCommand {
        departure_operation: other.original.operation,
        expected_stock_revision: rev(8),
        expected_catalog_revision: f.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: f.graph.hub(),
                stock_revision: rev(8),
                catalog: &f.catalog,
                latest_recorded_at: at(130),
                uncertain_submissions: false,
            },
            original: &other.recipient,
            history: &[],
        },
        id(223),
        owner(),
        at(140),
    )
    .unwrap();
    other.history.push(record);
    f.known.push(other);
    assert_eq!(
        f.plan(),
        Err(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
}
