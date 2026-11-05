use super::*;
use crate::hub::{
    actuals::{
        ActualCountDuplicateContext, ActualDuplicateLink, ActualKnownCountExplanation,
        ActualKnownInterpretation, ActualPhysicalActivity, ActualPhysicalActivityKind,
        CountExplanationSourceWitness, actual_duplicate_result_digest,
    },
    corrections::{
        AttributionCut, CorrectedDepartureFamily, CountExplanationAmendment,
        OriginalRecipientAttribution, RecipientAttributionCommand, RecipientAttributionContext,
    },
};
use std::sync::Arc;

fn ancestor() -> (Fixture, CountExplanationPlan) {
    let mut f = Fixture::new();
    f.count.before.id = id(110);
    f.count.before.parent = Some(id(10));
    f.count.before.identification = Some(crate::hub::movement::PhysicalIdentification {
        label: crate::hub::reference::HubLabel::new("Separately marked ancestor cohort").unwrap(),
        observation: note(),
    });
    f.count.before.location = id(4);
    f.count.before.causal_floor = Some(at(80));
    f.count.before.work.clear();
    // Build the observation and reconciliation through their policy commands,
    // so captured location, cutoff and later physical clock remain coherent.
    let group = f.count.before.clone();
    let mut current = crate::hub::counts::CountContext {
        hub: f.source.hub,
        stock_revision: rev(4),
        catalog: &f.catalog,
        group: &group,
        latest_physical_recorded_at: at(85),
        previous_reconciliation: None,
        latest_observation: None,
        unresolved_actual: false,
        uncertain_intent: false,
    };
    let observed = crate::hub::counts::CountObserveCommand {
        expected_stock_revision: rev(4),
        expected_catalog_revision: f.catalog.revision(),
        group: group.id,
        expected_group_revision: group.revision,
        counted_packs: packs(18),
        cutoff: Occurrence::Known(at(100)),
        observation: note(),
    }
    .plan(&current.observation_context(), id(20), owner(), at(100))
    .unwrap();
    current.stock_revision = observed.recorded_revision;
    current.latest_observation = Some(observed.id);
    f.count = crate::hub::counts::CountPostCommand {
        expected_stock_revision: current.stock_revision,
        expected_catalog_revision: f.catalog.revision(),
        count: observed.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note(),
    }
    .plan(&observed, &current, id(21), owner(), at(110))
    .unwrap();
    f.source.group.packs = packs(30);
    f.report.stock_revision = rev(7);
    f.account = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            reconciliation: f.count.id,
            operation: f.count.operation,
            stock_revision: f.count.stock_revision,
            trace: f.count.before.trace,
            variant: f.count.before.variant,
            packs: packs(2),
        },
        vec![],
    )
    .unwrap();
    f.graph = PhysicalLineage::new(
        f.source.hub,
        f.source.group.trace,
        f.source.group.variant,
        rev(4),
        vec![
            PhysicalLineageNode {
                id: id(10),
                parent: None,
                trace: f.source.group.trace,
                variant: f.source.group.variant,
                created_revision: rev(2),
                packs: packs(10),
                recovery: None,
            },
            PhysicalLineageNode {
                id: id(110),
                parent: Some(id(10)),
                trace: f.source.group.trace,
                variant: f.source.group.variant,
                created_revision: rev(3),
                packs: packs(20),
                recovery: None,
            },
        ],
    )
    .unwrap();
    let activity = [
        ActualPhysicalActivity {
            sequence: 1,
            operation: id(203),
            actor: owner(),
            stock_revision: rev(3),
            hub: f.source.hub,
            trace: f.source.group.trace,
            group: id(110),
            location: Some(id(3)),
            kind: ActualPhysicalActivityKind::GroupBorn,
            occurred: Occurrence::Known(at(60)),
            recorded_at: at(65),
        },
        ActualPhysicalActivity {
            sequence: 2,
            operation: id(204),
            actor: owner(),
            stock_revision: rev(4),
            hub: f.source.hub,
            trace: f.source.group.trace,
            group: id(110),
            location: Some(id(4)),
            kind: ActualPhysicalActivityKind::LocationChanged,
            occurred: Occurrence::Known(at(80)),
            recorded_at: at(85),
        },
    ];
    f.count.validate().expect("complete original count fixture");
    f.report.validate().expect("preserved raw report fixture");
    let plan = {
        let mut context = f.context();
        context.source.source.subsequent_activity = &activity;
        f.facts().plan(&context, id(202), owner(), at(150)).unwrap()
    };
    assert_eq!(plan.historical_source.historical().group.id, id(10));
    assert_eq!(plan.known_event.lines[0].group, id(110));
    (f, plan)
}
fn interpretation(f: &Fixture, plan: &CountExplanationPlan) -> ActualKnownInterpretation {
    let original = plan.known_event.clone();
    let account =
        CountLossQuantityAccount::restore(f.account.original(), vec![plan.explanation]).unwrap();
    ActualKnownInterpretation {
        stock_revision: rev(10),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        recipient_original: OriginalRecipientAttribution {
            hub: original.hub,
            family: CorrectedDepartureFamily::ExplainedCountLoss,
            operation: original.operation,
            stock_revision: original.stock_revision,
            catalog_revision: original.catalog_revision,
            recorded_at: original.recorded_at,
            actor: original.actor,
            recipient: original.recipient.clone(),
            populations: vec![original.trace],
            issued_versions: vec![],
        },
        original,
        recipient_history: vec![],
        quantities: vec![],
        count_explanation: Some(Arc::new(ActualKnownCountExplanation {
            count: plan.count.clone(),
            account,
        })),
    }
}
fn report(f: &Fixture) -> ActualReportRecord {
    let mut command = f.report.report.clone();
    command.packs = None;
    command
        .preserve(
            f.source.hub,
            rev(8),
            f.catalog.revision(),
            id(220),
            owner(),
            at(160),
        )
        .unwrap()
}
fn command(report: &ActualReportRecord, known: &ActualKnownDeparture) -> ActualDuplicateLink {
    ActualDuplicateLink {
        report: report.id,
        known_operation: known.operation,
        known_trace: known.trace,
        known_variant: known.variant.id,
        verification: note(),
    }
}
#[test]
fn count_link_uses_exact_original_ancestor_source_without_rewriting_anchor() {
    let (f, original) = ancestor();
    let record = interpretation(&f, &original);
    let report = report(&f);
    let source = CountExplanationSourceWitness::from_explanation(&original).unwrap();
    let command = command(&report, &record.original);
    assert_eq!(
        command.plan_with_interpretation(&report, &record, None, id(230), owner(), at(170)),
        Err(ActualReportError::InvalidNativeContext)
    );
    let plan = command
        .plan_with_count_interpretation(
            &report,
            ActualCountDuplicateContext {
                interpretation: &record,
                source: &source,
            },
            None,
            id(230),
            owner(),
            at(170),
        )
        .unwrap();
    assert_eq!(plan.known.lines[0].group, id(110));
    assert_eq!(source.historical().group.id, id(10));
    assert_eq!(plan.known, original.known_event);
    let hash = command.fingerprint(report.hub, owner(), id(230));
    assert_ne!(
        actual_duplicate_result_digest(&hash, &command, &plan, rev(11), record.catalog_revision)
            .unwrap(),
        [0; 32]
    );
}
#[test]
fn count_witness_rejects_unrelated_group_and_wrong_location_but_retains_unknown_and_anchor() {
    let (f, original) = ancestor();
    let record = interpretation(&f, &original);
    let source = CountExplanationSourceWitness::from_explanation(&original).unwrap();
    for case in 0..4 {
        let mut report = report(&f);
        report.report.sources = match case {
            0 => ActualReportSources::EstablishedClaim(ReportedActualSource {
                trace: record.original.trace,
                group: Some(id(111)),
                location: Some(id(3)),
            }),
            1 => ActualReportSources::EstablishedClaim(ReportedActualSource {
                trace: record.original.trace,
                group: Some(id(10)),
                location: Some(id(4)),
            }),
            2 => ActualReportSources::Unknown,
            _ => ActualReportSources::EstablishedClaim(ReportedActualSource {
                trace: record.original.trace,
                group: Some(id(110)),
                location: Some(id(3)),
            }),
        };
        let result = command(&report, &record.original).plan_with_count_interpretation(
            &report,
            ActualCountDuplicateContext {
                interpretation: &record,
                source: &source,
            },
            None,
            id(230),
            owner(),
            at(170),
        );
        if case < 2 {
            assert_eq!(result, Err(ActualReportError::HistoricalSourceUnresolved));
        } else {
            assert!(result.is_ok());
        }
    }
}
#[test]
fn count_source_stays_original_after_effective_zero_and_unknown_recipient() {
    let (f, original) = ancestor();
    let mut record = interpretation(&f, &original);
    let source = CountExplanationSourceWitness::from_explanation(&original).unwrap();
    let before = source.digest().unwrap();
    let snapshot = Arc::make_mut(record.count_explanation.as_mut().unwrap());
    snapshot.account = CountLossQuantityAccount::restore_corrected(
        snapshot.account.original(),
        snapshot.account.explanations().to_vec(),
        vec![CountExplanationAmendment {
            operation: id(240),
            stock_revision: rev(11),
            explanation: record.original.operation,
            before: packs(1),
            after: packs(0),
        }],
        vec![],
    )
    .unwrap();
    let change = RecipientAttributionCommand {
        departure_operation: record.original.operation,
        expected_stock_revision: rev(11),
        expected_catalog_revision: f.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: record.original.hub,
                stock_revision: rev(11),
                catalog: &f.catalog,
                latest_recorded_at: at(160),
                uncertain_submissions: false,
            },
            original: &record.recipient_original,
            history: &[],
            linked_issued_versions: &[],
        },
        id(241),
        owner(),
        at(165),
    )
    .unwrap();
    record.recipient_history.push(change);
    record.stock_revision = rev(12);
    record.latest_recorded_at = at(165);
    assert_eq!(record.view().unwrap().effective_packs, packs(0));
    assert!(record.view().unwrap().recipients.has_unknown());
    let report = report(&f);
    let command = command(&report, &record.original);
    let linked = command
        .plan_with_count_interpretation(
            &report,
            ActualCountDuplicateContext {
                interpretation: &record,
                source: &source,
            },
            None,
            id(230),
            owner(),
            at(170),
        )
        .unwrap();
    assert_eq!(linked.known, original.known_event);
    assert_eq!(source.digest().unwrap(), before);
}
#[test]
fn count_source_witness_cannot_bind_a_changed_original_or_count() {
    let (_, original) = ancestor();
    let source = CountExplanationSourceWitness::from_explanation(&original).unwrap();
    for case in 0..4 {
        let mut bad = original.clone();
        match case {
            0 => bad.known_event.lines[0].group = id(111),
            1 => bad.count.reason = SourceProvenance::new("Different original count").unwrap(),
            2 => bad.facts.packs = packs(2),
            _ => {
                bad.report.report.sources =
                    ActualReportSources::EstablishedClaim(ReportedActualSource {
                        trace: bad.known_event.trace,
                        group: Some(id(111)),
                        location: None,
                    })
            }
        }
        if case < 2 {
            assert!(source.validate_for(&bad.known_event, &bad.count).is_err());
        } else {
            assert!(CountExplanationSourceWitness::from_explanation(&bad).is_err());
        }
    }
}

#[test]
fn count_link_rejects_source_operation_aliases_and_swapped_known_witness() {
    let (f, original) = ancestor();
    let record = interpretation(&f, &original);
    let source = CountExplanationSourceWitness::from_explanation(&original).unwrap();
    let report = report(&f);
    let command = command(&report, &record.original);
    assert!(
        command
            .plan_with_count_interpretation(
                &report,
                ActualCountDuplicateContext {
                    interpretation: &record,
                    source: &source
                },
                None,
                source.historical().operation,
                owner(),
                at(170)
            )
            .is_err()
    );
    let mut aliased = report.clone();
    aliased.operation = source.historical().operation;
    assert!(
        command
            .plan_with_count_interpretation(
                &aliased,
                ActualCountDuplicateContext {
                    interpretation: &record,
                    source: &source
                },
                None,
                id(230),
                owner(),
                at(170)
            )
            .is_err()
    );
    let mut other = original.clone();
    other.operation = id(231);
    other.explanation.operation = id(231);
    other.known_event.operation = id(231);
    let swapped = CountExplanationSourceWitness::from_explanation(&other).unwrap();
    assert!(
        command
            .plan_with_count_interpretation(
                &report,
                ActualCountDuplicateContext {
                    interpretation: &record,
                    source: &swapped
                },
                None,
                id(230),
                owner(),
                at(170)
            )
            .is_err()
    );
}
