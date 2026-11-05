use super::*;
use crate::hub::actuals::{
    ActualReportCommand, ActualReportSources, HistoricalActualSource, ReportedActualSource,
};
use crate::hub::portions::{PhysicalLineage, PhysicalLineageNode};
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
    SourceProvenance::new(
        "Verified original witness identifies the already counted missing quantity",
    )
    .unwrap()
}
struct Fixture {
    catalog: ReferenceCatalog,
    count: CountPostPlan,
    account: CountLossQuantityAccount,
    report: ActualReportRecord,
    source: HistoricalActualSource,
    graph: PhysicalLineage,
    treatment: TreatmentState,
}
impl Fixture {
    fn new() -> Self {
        let (catalog, count) = crate::hub::counts::shortage_policy_fixture();
        let scope = &count.count.scope;
        let account = CountLossQuantityAccount::restore(
            OriginalCountLossQuantity {
                reconciliation: count.id,
                operation: count.operation,
                stock_revision: count.stock_revision,
                trace: scope.trace,
                variant: scope.variant.id,
                packs: packs(2),
            },
            vec![],
        )
        .unwrap();
        let source = HistoricalActualSource {
            operation: id(200),
            actor: owner(),
            hub: scope.hub,
            stock_revision: rev(2),
            catalog_revision: catalog.revision(),
            group: (&count.before).into(),
            location: count.references.location.clone(),
            occurred: Occurrence::Known(at(40)),
            recorded_at: at(50),
        };
        let graph = PhysicalLineage::new(
            scope.hub,
            scope.trace,
            scope.variant.id,
            scope.stock_revision,
            vec![PhysicalLineageNode {
                id: count.before.id,
                parent: None,
                trace: scope.trace,
                variant: scope.variant.id,
                created_revision: rev(2),
                packs: packs(20),
                recovery: None,
            }],
        )
        .unwrap();
        let report = ActualReportCommand {
            variant: Some(scope.variant.id),
            recipient: Some(id(5)),
            packs: Some(packs(1)),
            sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                trace: scope.trace,
                group: Some(count.before.id),
                location: Some(count.before.location),
            }),
            occurred: Occurrence::Known(at(70)),
            observation: note(),
        }
        .preserve(
            scope.hub,
            count.stock_revision,
            catalog.revision(),
            id(201),
            owner(),
            at(140),
        )
        .unwrap();
        Self {
            catalog,
            count,
            account,
            report,
            source,
            graph,
            treatment: TreatmentState::default(),
        }
    }
    fn facts(&self) -> CountExplanationFacts {
        CountExplanationFacts {
            report: self.report.id,
            count_reconciliation: self.count.id,
            expected_stock_revision: self.report.stock_revision,
            expected_catalog_revision: self.catalog.revision(),
            expected_prior: self
                .account
                .explanations()
                .iter()
                .map(|e| (e.stock_revision, e.operation))
                .chain(
                    self.account
                        .amendments()
                        .iter()
                        .map(|a| (a.stock_revision, a.operation)),
                )
                .max_by_key(|(at, _)| at.value())
                .map(|(_, op)| op),
            packs: packs(1),
            recipient: id(5),
            occurred: Occurrence::Known(at(70)),
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        }
    }
    fn context(&self) -> CountExplanationContext<'_> {
        CountExplanationContext {
            hub: self.graph.hub(),
            stock_revision: self.report.stock_revision,
            latest_recorded_at: at(140),
            catalog: &self.catalog,
            account: &self.account,
            report: &self.report,
            prior_report_review: None,
            source: CountedShortageSourceContext {
                source: ActualSourceContext {
                    source: &self.source,
                    lineage: &self.graph,
                    balancing_group: &self.count.before,
                    subsequent_activity: &[],
                },
                count: &self.count,
            },
            treatment_at_count: &self.treatment,
            known_departures: vec![],
            uncertain_submissions: false,
        }
    }
    fn plan(&self) -> Result<CountExplanationPlan, QuantityEffectError> {
        self.facts()
            .plan(&self.context(), id(202), owner(), at(150))
    }
}
#[test]
fn shortage_explanation_classifies_one_without_a_second_stock_or_work_effect() {
    let f = Fixture::new();
    let p = f.plan().unwrap();
    assert_eq!(p.unexplained_after, packs(1));
    assert_eq!(p.known_event.lines[0].packs, packs(1));
    assert_eq!(p.count, f.count);
    assert_eq!(p.count.before.packs, packs(20));
    let CountEffect::Shortage {
        after_packs,
        work_changes,
        ..
    } = &p.count.effect
    else {
        panic!("shortage")
    };
    assert_eq!(*after_packs, packs(18));
    assert_eq!(
        work_changes[0].after.quantity.no_longer_required(),
        packs(2)
    );
    assert_eq!(p.report, f.report);
    assert_eq!(f.account.unexplained(), packs(2));
}
#[test]
fn only_exact_pre_count_cut_permits_the_counted_shortage_source_path() {
    let f = Fixture::new();
    assert!(f.plan().is_ok());
    assert_eq!(
        f.context()
            .source
            .source
            .resolve(&f.report, Occurrence::Known(at(70)), at(150)),
        Err(ActualReportError::InvalidNativeContext)
    );
    let wrong = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        rev(3),
        f.graph.nodes().to_vec(),
    )
    .unwrap();
    let mut c = f.context();
    c.source.source.lineage = &wrong;
    assert_eq!(
        f.facts().plan(&c, id(202), owner(), at(150)),
        Err(QuantityEffectError::Actual(
            ActualReportError::HistoricalSourceUnresolved
        ))
    );
}
#[test]
fn later_or_unknown_departure_time_cannot_explain_an_earlier_count() {
    let mut f = Fixture::new();
    f.report.report.occurred = Occurrence::Unknown;
    for occurred in [Occurrence::Known(at(81)), Occurrence::Unknown] {
        let mut facts = f.facts();
        facts.occurred = occurred;
        assert_eq!(
            facts.plan(&f.context(), id(202), owner(), at(150)),
            Err(QuantityEffectError::Actual(
                ActualReportError::HistoricalSourceUnresolved
            ))
        );
    }
}
#[test]
fn exhausted_allowance_and_mismatched_count_original_fail_closed() {
    let mut f = Fixture::new();
    f.account = CountLossQuantityAccount::restore(
        f.account.original(),
        vec![CountLossExplanation {
            operation: id(210),
            stock_revision: f.report.stock_revision,
            packs: packs(2),
        }],
    )
    .unwrap();
    assert_eq!(
        f.plan(),
        Err(QuantityEffectError::Accounting(
            CorrectionError::InvalidQuantity
        ))
    );
    f.account = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            reconciliation: id(211),
            ..f.account.original()
        },
        vec![],
    )
    .unwrap();
    assert_eq!(f.plan(), Err(QuantityEffectError::InvalidHistory));
}
#[test]
fn a_known_physical_event_cannot_be_classified_again_from_the_shortage() {
    let f = Fixture::new();
    let first = f.plan().unwrap();
    let mut event = first.known_event;
    event.operation = id(210);
    event.stock_revision = rev(3);
    event.recorded_at = at(100);
    let recipient = OriginalRecipientAttribution {
        hub: event.hub,
        family: CorrectedDepartureFamily::ReviewedActual,
        operation: event.operation,
        stock_revision: event.stock_revision,
        catalog_revision: event.catalog_revision,
        recorded_at: event.recorded_at,
        actor: event.actor,
        recipient: event.recipient.clone(),
        populations: vec![event.trace],
        issued_versions: vec![],
    };
    let lines = [DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: event.operation,
            group: event.lines[0].group,
            trace: event.trace,
            variant: event.variant.id,
            stock_revision: event.stock_revision,
            packs: event.lines[0].packs,
        },
        vec![],
        packs(0),
    )
    .unwrap()];
    let known = CorrectionKnownDeparture {
        original: &event,
        recipient_original: &recipient,
        recipient_history: &[],
        quantity: KnownCorrectionQuantity::Physical(&lines),
    };
    let mut c = f.context();
    c.known_departures = vec![known];
    assert_eq!(
        f.facts().plan(&c, id(203), owner(), at(150)),
        Err(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
    // The supplied original cannot come from a future native cut either.
    let future = ActualKnownDeparture {
        stock_revision: rev(99),
        ..event.clone()
    };
    let mut c = f.context();
    c.known_departures = vec![CorrectionKnownDeparture {
        original: &future,
        ..known
    }];
    assert_eq!(
        f.facts().plan(&c, id(203), owner(), at(150)),
        Err(QuantityEffectError::InvalidHistory)
    );
}

#[test]
fn zero_effective_count_explanation_with_unknown_recipient_cannot_escape_duplicate_review() {
    let mut f = Fixture::new();
    let original_explanation = CountLossExplanation {
        operation: id(210),
        stock_revision: rev(6),
        packs: packs(1),
    };
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        vec![original_explanation],
        vec![CountExplanationAmendment {
            operation: id(211),
            stock_revision: rev(7),
            explanation: id(210),
            before: packs(1),
            after: packs(0),
        }],
        vec![],
    )
    .unwrap();
    let event = ActualKnownDeparture {
        operation: id(210),
        actor: owner(),
        hub: f.graph.hub(),
        stock_revision: rev(6),
        catalog_revision: f.catalog.revision(),
        trace: f.graph.trace(),
        variant: f.count.references.variant.clone(),
        recipient: f.catalog.get(id(5)).unwrap().clone(),
        occurred: Occurrence::Known(at(70)),
        recorded_at: at(145),
        lines: vec![ActualKnownDepartureLine {
            group: f.count.before.id,
            location: f.source.location.clone(),
            packs: packs(1),
        }],
    };
    let original_recipient = OriginalRecipientAttribution {
        hub: event.hub,
        family: CorrectedDepartureFamily::ExplainedCountLoss,
        operation: event.operation,
        stock_revision: event.stock_revision,
        catalog_revision: event.catalog_revision,
        recorded_at: event.recorded_at,
        actor: event.actor,
        recipient: event.recipient.clone(),
        populations: vec![event.trace],
        issued_versions: vec![],
    };
    let change = RecipientAttributionCommand {
        departure_operation: event.operation,
        expected_stock_revision: rev(7),
        expected_catalog_revision: f.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            linked_issued_versions: &[],
            cut: AttributionCut {
                hub: event.hub,
                stock_revision: rev(7),
                catalog: &f.catalog,
                latest_recorded_at: at(147),
                uncertain_submissions: false,
            },
            original: &original_recipient,
            history: &[],
        },
        id(212),
        owner(),
        at(148),
    )
    .unwrap();
    let history = [change];
    let known = CorrectionKnownDeparture {
        original: &event,
        recipient_original: &original_recipient,
        recipient_history: &history,
        quantity: KnownCorrectionQuantity::CountExplanation {
            count: &f.count,
            account: &f.account,
        },
    };
    let mut context = f.context();
    context.stock_revision = rev(8);
    context.latest_recorded_at = at(150);
    context.known_departures = vec![known];
    let mut facts = f.facts();
    facts.expected_stock_revision = rev(8);
    assert_eq!(
        facts.plan(&context, id(203), owner(), at(160)),
        Err(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
    assert_eq!(f.account.explanations()[0].packs, packs(1));
    assert_eq!(f.account.effective_explanations()[0].packs, packs(0));
}

#[test]
fn explicit_count_source_command_rejects_substitution_and_binds_every_input() {
    let f = Fixture::new();
    let command = CountExplanationCommand {
        facts: f.facts(),
        source_operation: f.source.operation,
        source_group: f.source.group.id,
    };
    assert_eq!(
        command
            .plan(&f.context(), id(202), owner(), at(150))
            .unwrap(),
        f.plan().unwrap()
    );
    let original = command.fingerprint(f.graph.hub(), owner(), id(202));
    assert_ne!(
        original,
        command.facts.fingerprint(f.graph.hub(), owner(), id(202))
    );
    for field in 0..15 {
        let mut changed = command.clone();
        match field {
            0 => changed.source_operation = id(210),
            1 => changed.source_group = id(210),
            2 => changed.facts.report = id(210),
            3 => changed.facts.count_reconciliation = id(210),
            4 => changed.facts.expected_stock_revision = rev(100),
            5 => changed.facts.expected_catalog_revision = rev(100),
            6 => changed.facts.expected_prior = Some(id(210)),
            7 => changed.facts.packs = packs(2),
            8 => changed.facts.recipient = id(210),
            9 => changed.facts.occurred = Occurrence::Unknown,
            10 => {
                changed.facts.source_evidence =
                    SourceProvenance::new("Different source evidence").unwrap()
            }
            11 => {
                changed.facts.quantity_evidence =
                    SourceProvenance::new("Different quantity evidence").unwrap()
            }
            12 => {
                changed.facts.recipient_evidence =
                    SourceProvenance::new("Different recipient evidence").unwrap()
            }
            13 => {
                changed.facts.timing_evidence =
                    SourceProvenance::new("Different timing evidence").unwrap()
            }
            _ => {
                assert_ne!(
                    original,
                    command.fingerprint(f.graph.hub(), owner(), id(211))
                );
                continue;
            }
        }
        assert_ne!(
            original,
            changed.fingerprint(f.graph.hub(), owner(), id(202)),
            "field {field}"
        );
        if field < 2 {
            assert_eq!(
                changed.plan(&f.context(), id(202), owner(), at(150)),
                Err(QuantityEffectError::InvalidScope)
            );
        }
    }
}

mod classification;
mod source_witness;
