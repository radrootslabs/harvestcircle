use super::*;
use crate::hub::{
    actuals::{
        ActualReportCommand, ActualReportSources, ActualSourceContext,
        CountedShortageSourceContext, HistoricalActualSource, ReportedActualSource,
    },
    counts::{CountContext, CountObserveCommand, CountPostCommand},
    portions::{PhysicalLineage, PhysicalLineageNode},
    treatment::TreatmentState,
};

#[test]
fn an_intake_cannot_be_the_return_from_its_own_counted_shortage_even_at_equal_physical_times() {
    let f = Fixture::new();
    let hub = f.original.known_event.hub;
    let actor = f.original.actor;
    let catalog = &f.catalog;
    let returned_command = ReturnCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: f.original.known_event.variant.id,
        location: f.original.count.before.location,
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: packs(20),
        identification: PhysicalIdentification {
            label: HubLabel::new("Independent originally returned twenty").unwrap(),
            observation: note(),
        },
        planned_location: None,
        departure: None,
        occurred: Occurrence::Known(at(40)),
        observation: note(),
    };
    let returned_context = ReturnContext {
        hub,
        stock_revision: HubRevision::INITIAL,
        catalog,
        departure: None,
        established_return_totals: &[],
        current_allowances: None,
    };
    let returned = returned_command
        .plan(
            &returned_context,
            ReturnIdentities::from_operation_bytes(id(230)).unwrap(),
            actor,
            at(50),
        )
        .unwrap();
    let input = returned_command.fingerprint(hub, actor, returned.identities);
    let returned_digest = crate::hub::returns::return_result_digest(
        &input,
        &returned_command,
        &returned_context,
        &returned,
    )
    .unwrap();
    let mut current = CountContext {
        hub,
        stock_revision: returned.stock_revision,
        catalog,
        group: &returned.group,
        latest_physical_recorded_at: returned.intake.recorded_at,
        previous_reconciliation: None,
        latest_observation: None,
        unresolved_actual: false,
        uncertain_intent: false,
    };
    let observed = CountObserveCommand {
        expected_stock_revision: current.stock_revision,
        expected_catalog_revision: catalog.revision(),
        group: returned.group.id,
        expected_group_revision: returned.group.revision,
        counted_packs: packs(0),
        cutoff: Occurrence::Known(at(80)),
        observation: note(),
    }
    .plan(&current.observation_context(), id(20), actor, at(100))
    .unwrap();
    current.stock_revision = observed.recorded_revision;
    current.latest_observation = Some(observed.id);
    let count = CountPostCommand {
        expected_stock_revision: current.stock_revision,
        expected_catalog_revision: catalog.revision(),
        count: observed.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note(),
    }
    .plan(&observed, &current, id(21), actor, at(110))
    .unwrap();
    let report = ActualReportCommand {
        variant: Some(returned.intake.variant.id),
        recipient: Some(f.recipient.recipient.id),
        packs: Some(packs(20)),
        sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
            trace: returned.identities.population,
            group: Some(returned.group.id),
            location: Some(returned.group.location),
        }),
        occurred: Occurrence::Known(at(40)),
        observation: note(),
    }
    .preserve(
        hub,
        count.stock_revision.next().unwrap(),
        catalog.revision(),
        id(201),
        actor,
        at(140),
    )
    .unwrap();
    let graph = PhysicalLineage::new(
        hub,
        returned.identities.population,
        returned.intake.variant.id,
        returned.stock_revision,
        vec![PhysicalLineageNode {
            id: returned.group.id,
            parent: None,
            trace: returned.identities.population,
            variant: returned.intake.variant.id,
            created_revision: returned.stock_revision,
            packs: returned.intake.packs,
            recovery: None,
        }],
    )
    .unwrap();
    let source = HistoricalActualSource {
        operation: returned.identities.operation,
        actor,
        hub,
        stock_revision: returned.stock_revision,
        catalog_revision: returned.catalog_revision,
        group: (&returned.group).into(),
        location: returned.intake.location.clone(),
        occurred: returned.intake.occurred,
        recorded_at: returned.intake.recorded_at,
    };
    let account = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: count.before.trace,
            variant: count.before.variant,
            packs: packs(20),
        },
        Vec::new(),
    )
    .unwrap();
    let treatment = TreatmentState::default();
    let context = CountExplanationContext {
        hub,
        stock_revision: report.stock_revision,
        latest_recorded_at: report.recorded_at,
        catalog,
        account: &account,
        report: &report,
        prior_report_review: None,
        source: CountedShortageSourceContext {
            source: ActualSourceContext {
                source: &source,
                lineage: &graph,
                balancing_group: &count.before,
                subsequent_activity: &[],
            },
            count: &count,
        },
        treatment_at_count: &treatment,
        known_departures: Vec::new(),
        uncertain_submissions: false,
    };
    let original = CountExplanationFacts {
        report: report.id,
        count_reconciliation: count.id,
        expected_stock_revision: report.stock_revision,
        expected_catalog_revision: catalog.revision(),
        expected_prior: None,
        packs: packs(20),
        recipient: f.recipient.recipient.id,
        occurred: Occurrence::Known(at(40)),
        source_evidence: note(),
        quantity_evidence: note(),
        recipient_evidence: note(),
        timing_evidence: note(),
    }
    .plan(&context, id(202), actor, at(150))
    .unwrap();
    // Both complete pure originals are valid and timing permits equality. The
    // cycle is exclusively physical provenance, not a clock/order shortcut.
    CountExplanationSourceWitness::from_explanation(&original).unwrap();
    assert_eq!(
        crate::hub::returns::return_timing(original.known_event.occurred, returned.intake.occurred),
        ReturnTiming::AfterOrSame
    );
    let account =
        CountLossQuantityAccount::restore(account.original(), vec![original.explanation]).unwrap();
    // Full quantity is otherwise admissible: rejection cannot be masked by an
    // oversized Return relative to the effective explanation.
    CountLossQuantityAccount::restore_corrected(
        account.original(),
        account.explanations().to_vec(),
        Vec::new(),
        vec![CountExplanationReturns {
            explanation: original.operation,
            packs: packs(20),
        }],
    )
    .unwrap();
    let known = &original.known_event;
    let recipient = OriginalRecipientAttribution {
        hub,
        family: CorrectedDepartureFamily::ExplainedCountLoss,
        operation: known.operation,
        stock_revision: known.stock_revision,
        catalog_revision: known.catalog_revision,
        recorded_at: known.recorded_at,
        actor: known.actor,
        recipient: known.recipient.clone(),
        populations: vec![known.trace],
        issued_versions: Vec::new(),
    };
    let command = CountReturnAssociationCommand {
        return_operation: returned.identities.operation,
        explanation_operation: original.operation,
        expected_stock_revision: known.stock_revision,
        expected_catalog_revision: catalog.revision(),
        expected_account_prior: Some(original.operation),
        expected_returns_prior: None,
        source_operation: source.operation,
        source_group: source.group.id,
        source_evidence: note(),
        quantity_evidence: note(),
        timing_evidence: note(),
    };
    let context = CountReturnAssociationContext {
        hub,
        stock_revision: known.stock_revision,
        catalog,
        latest_recorded_at: at(150),
        returned: &returned,
        return_hub: hub,
        return_original_digest: returned_digest,
        explanation: &original,
        account: &account,
        recipient_original: &recipient,
        recipient_history: &[],
        associations: &[],
        normal_returns: &[],
        prior_association_for_return: None,
        uncertain_submissions: false,
    };
    assert_eq!(
        command.plan(&context, id(240), actor, at(200)),
        Err(CountReturnAssociationError::InvalidFacts)
    );
}
