use super::*;
use crate::hub::portions::PhysicalLineageNode;
use crate::hub::stock::ReceiptIdentities;

fn native_like_fixture() -> (Fixture, HistoricalActualSource, Vec<ActualPhysicalBirth>) {
    let mut f = Fixture::new(known(120));
    let operation = id(220);
    let ids = ReceiptIdentities::from_operation_bytes(operation).unwrap();
    f.group.id = ids.group;
    f.group.trace = ids.trace;
    f.group.parent = None;
    f.group.identification = None;
    f.group.packs = packs(20);
    f.group.revision = HubRevision::INITIAL;
    f.group.causal_floor = Some(at(100));
    f.group.work.clear();
    let graph = PhysicalLineage::new(
        f.graph.hub(),
        ids.trace,
        f.group.variant,
        HubRevision::from_value(4).unwrap(),
        vec![PhysicalLineageNode {
            recovery: None,
            id: ids.group,
            parent: None,
            trace: ids.trace,
            variant: f.group.variant,
            created_revision: HubRevision::from_value(2).unwrap(),
            packs: packs(20),
        }],
    )
    .unwrap();
    f.original = report()
        .preserve(
            graph.hub(),
            graph.revision(),
            f.catalog.revision(),
            id(180),
            owner(),
            at(150),
        )
        .unwrap();
    f.original.report.occurred = known(120);
    f.graph = PhysicalLineage::new(
        graph.hub(),
        graph.trace(),
        graph.variant(),
        f.original.stock_revision,
        graph.nodes().to_vec(),
    )
    .unwrap();
    f.state = TreatmentState::restore(&f.graph, vec![], vec![]).unwrap();
    f.facts.report = f.original.id;
    f.facts.group = f.group.id;
    f.facts.expected_group_revision = f.group.revision;
    f.facts.expected_stock_revision = f.graph.revision();
    f.hold(known(110), None);
    let source = HistoricalActualSource {
        actor: owner(),
        operation,
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(2).unwrap(),
        catalog_revision: f.catalog.revision(),
        group: (&f.group).into(),
        location: f.catalog.get(f.group.location).unwrap().clone(),
        occurred: known(100),
        recorded_at: at(100),
    };
    let birth = ActualPhysicalBirth {
        kind: ActualPhysicalBirthKind::Receipt,
        operation,
        hub: f.graph.hub(),
        trace: ids.trace,
        variant: f.group.variant,
        group: ids.group,
        parent: None,
        stock_revision: HubRevision::from_value(2).unwrap(),
        packs: packs(20),
        actor: owner(),
        occurred: known(100),
        recorded_at: at(100),
    };
    (f, source, vec![birth])
}
fn command(f: &Fixture, source: &HistoricalActualSource) -> ActualNewDepartureCommand {
    ActualNewDepartureCommand {
        facts: f.facts.clone(),
        source_operation: source.operation,
        source_group: source.group.id,
        duplicate_review_basis: note(
            "All native original departures reviewed, separately established physical occurrence",
        ),
    }
}
fn context<'a>(
    f: &'a Fixture,
    source: &'a HistoricalActualSource,
) -> ActualNewDepartureContext<'a> {
    ActualNewDepartureContext {
        review: f.context(),
        historical_source: ActualSourceContext {
            source,
            lineage: &f.graph,
            balancing_group: &f.group,
            subsequent_activity: &[],
        },
        known_departures: &[],
        prior_review: None,
    }
}
fn findings(f: &Fixture, births: &[ActualPhysicalBirth]) -> Vec<HistoricalHoldScopeAssessment> {
    f.state
        .holds()
        .iter()
        .map(|hold| {
            HistoricalHoldScopeContext {
                lineage: &f.graph,
                group: f.group.id,
                occurred: f.facts.occurred,
                births,
                now: at(170),
            }
            .verify(hold)
            .unwrap()
        })
        .collect()
}
#[test]
fn report_seal_binds_claims_and_original_native_metadata_without_requiring_source_resolution() {
    let f = Fixture::new(Occurrence::Unknown);
    let record = &f.original;
    let input = record
        .report
        .fingerprint(record.hub, record.actor, record.operation);
    let digest = actual_report_result_digest(&input, record).unwrap();
    assert_eq!(
        actual_report_result_digest(&input, &record.clone()).unwrap(),
        digest
    );
    let mut changed = record.clone();
    changed.recorded_at = at(151);
    assert_ne!(
        actual_report_result_digest(&input, &changed).unwrap(),
        digest
    );
    changed.report.packs = Some(packs(3));
    assert_eq!(
        actual_report_result_digest(&input, &changed),
        Err(ActualReportError::InvalidHistory)
    );
}
#[test]
fn physical_result_seal_checks_replanning_and_keeps_verified_birth_scope_proofs() {
    let (f, source, births) = native_like_fixture();
    let command = command(&f, &source);
    let context = context(&f, &source);
    let plan = command.plan(&context, id(230), owner(), at(170)).unwrap();
    let findings = findings(&f, &births);
    assert!(
        matches!(&findings[0],HistoricalHoldScopeAssessment::Verified(v) if v.timing()==HistoricalHoldTiming::EstablishedBreach)
    );
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let digest =
        actual_departure_result_digest(&input, &command, &context, &births, &findings, &plan)
            .unwrap();
    assert_eq!(
        actual_departure_result_digest(&input, &command, &context, &births, &findings, &plan)
            .unwrap(),
        digest
    );
    let mut changed = plan.clone();
    changed.after_causal_floor = None;
    assert_eq!(
        actual_departure_result_digest(&input, &command, &context, &births, &findings, &changed),
        Err(ActualReportError::InvalidNativeContext)
    );
    assert_eq!(
        actual_departure_result_digest(&input, &command, &context, &births, &[], &plan),
        Err(ActualReportError::InvalidHistory)
    );
    assert!(
        actual_departure_result_digest(&input, &command, &context, &[], &findings, &plan).is_err()
    );
}
#[test]
fn same_numeric_reference_revision_cannot_hide_different_captured_label_content() {
    let (mut f, source, births) = native_like_fixture();
    let command = command(&f, &source);
    let first = command
        .plan(&context(&f, &source), id(230), owner(), at(170))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), first.operation);
    let digest = actual_departure_result_digest(
        &input,
        &command,
        &context(&f, &source),
        &births,
        &findings(&f, &births),
        &first,
    )
    .unwrap();
    let location = f.group.location;
    let records = f
        .catalog
        .records()
        .cloned()
        .map(|mut record| {
            if record.id == location
                && let crate::hub::reference::ReferenceValue::Location { label, .. } =
                    &mut record.value
            {
                *label = crate::hub::reference::HubLabel::new(
                    "Altered content at the same claimed revision",
                )
                .unwrap();
            }
            record
        })
        .collect();
    f.catalog = ReferenceCatalog::restore(f.catalog.revision(), records).unwrap();
    let changed = command
        .plan(&context(&f, &source), id(230), owner(), at(170))
        .unwrap();
    assert_ne!(
        actual_departure_result_digest(
            &input,
            &command,
            &context(&f, &source),
            &births,
            &findings(&f, &births),
            &changed
        )
        .unwrap(),
        digest
    );
}
#[test]
fn native_birth_actor_and_proof_content_are_sealed_even_when_the_balance_plan_is_unchanged() {
    let (f, source, mut births) = native_like_fixture();
    let command = command(&f, &source);
    let context = context(&f, &source);
    let plan = command.plan(&context, id(230), owner(), at(170)).unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let first = actual_departure_result_digest(
        &input,
        &command,
        &context,
        &births,
        &findings(&f, &births),
        &plan,
    )
    .unwrap();
    births[0].actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_ne!(
        actual_departure_result_digest(
            &input,
            &command,
            &context,
            &births,
            &findings(&f, &births),
            &plan
        )
        .unwrap(),
        first
    );
}

#[test]
fn historical_source_recorder_is_sealed_separately_from_the_current_reviewer() {
    let (f, mut source, births) = native_like_fixture();
    let command = command(&f, &source);
    let first = command
        .plan(&context(&f, &source), id(230), owner(), at(170))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), first.operation);
    let digest = actual_departure_result_digest(
        &input,
        &command,
        &context(&f, &source),
        &births,
        &findings(&f, &births),
        &first,
    )
    .unwrap();
    source.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    let changed = command
        .plan(&context(&f, &source), id(230), owner(), at(170))
        .unwrap();
    assert_eq!(first.actor, changed.actor);
    assert_eq!(first.reduction, changed.reduction);
    assert_ne!(
        actual_departure_result_digest(
            &input,
            &command,
            &context(&f, &source),
            &births,
            &findings(&f, &births),
            &changed
        )
        .unwrap(),
        digest
    );
}
#[test]
fn duplicate_seal_keeps_known_original_uncertainty_and_rejects_wrong_review_cut() {
    let (f, _, _) = native_like_fixture();
    let scope = f.facts.recipient;
    let known = ActualKnownDeparture {
        actor: owner(),
        operation: id(240),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(3).unwrap(),
        catalog_revision: f.catalog.revision(),
        trace: f.graph.trace(),
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(scope).unwrap().clone(),
        occurred: known(120),
        recorded_at: at(130),
        lines: vec![ActualKnownDepartureLine {
            group: id(245),
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
            "This preserved witness describes the same immutable existing departure",
        ),
    };
    let record = command
        .plan(&f.original, &known, None, id(231), owner(), at(170))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), record.operation);
    let revision = HubRevision::from_value(7).unwrap();
    let first =
        actual_duplicate_result_digest(&input, &command, &record, revision, f.catalog.revision())
            .unwrap();
    assert_eq!(
        actual_duplicate_result_digest(
            &input,
            &command,
            &record,
            known.stock_revision,
            f.catalog.revision()
        ),
        Err(ActualReportError::InvalidNativeContext)
    );
    let mut another_recorder = known.clone();
    another_recorder.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    let changed = command
        .plan(
            &f.original,
            &another_recorder,
            None,
            id(231),
            owner(),
            at(170),
        )
        .unwrap();
    assert_eq!(record.actor, changed.actor);
    assert_ne!(
        actual_duplicate_result_digest(&input, &command, &changed, revision, f.catalog.revision())
            .unwrap(),
        first
    );
    let mut uncertain = known;
    uncertain.occurred = Occurrence::Unknown;
    let changed = command
        .plan(&f.original, &uncertain, None, id(231), owner(), at(170))
        .unwrap();
    assert_ne!(
        actual_duplicate_result_digest(&input, &command, &changed, revision, f.catalog.revision())
            .unwrap(),
        first
    );
}

#[test]
fn scoped_physical_projection_keeps_historical_exit_separate_from_later_balancing_location() {
    let (mut f, source, births) = native_like_fixture();
    let later = f
        .catalog
        .records()
        .find(|r| r.value.kind() == ReferenceKind::Location && r.id != source.location.id)
        .unwrap()
        .clone();
    f.group.location = later.id;
    f.group.revision = HubRevision::from_value(2).unwrap();
    f.group.causal_floor = Some(at(130));
    f.facts.expected_group_revision = f.group.revision;
    let activity = [ActualPhysicalActivity {
        sequence: 4,
        operation: id(222),
        actor: owner(),
        stock_revision: HubRevision::from_value(4).unwrap(),
        hub: f.graph.hub(),
        trace: f.graph.trace(),
        group: f.group.id,
        location: Some(later.id),
        kind: ActualPhysicalActivityKind::LocationChanged,
        occurred: known(130),
        recorded_at: at(140),
    }];
    let command = command(&f, &source);
    let mut context = context(&f, &source);
    context.historical_source.subsequent_activity = &activity;
    let plan = command.plan(&context, id(230), owner(), at(170)).unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let full = actual_departure_result_digest(
        &input,
        &command,
        &context,
        &births,
        &findings(&f, &births),
        &plan,
    )
    .unwrap();
    let projection = plan.physical_projection().unwrap();
    let digest = actual_departure_projection_digest(
        &input,
        plan.report.id,
        plan.historical_source.historical(),
        &projection,
    )
    .unwrap();
    assert_ne!(digest, full);
    assert_eq!(projection.lines[0].location, source.location);
    assert_eq!(plan.reduction.before.location, later.id);
    assert_eq!(plan.after_causal_floor, Some(at(130)));
    assert_eq!(projection.actor, plan.actor);
    assert_eq!(projection.packs().unwrap(), packs(2));
    let mut changed = projection.clone();
    changed.lines[0].location = later;
    assert_eq!(
        actual_departure_projection_digest(
            &input,
            plan.report.id,
            plan.historical_source.historical(),
            &changed
        ),
        Err(ActualReportError::InvalidNativeContext)
    );
    changed = projection.clone();
    changed.lines[0].packs = packs(3);
    assert_ne!(
        actual_departure_projection_digest(
            &input,
            plan.report.id,
            plan.historical_source.historical(),
            &changed
        )
        .unwrap(),
        digest
    );
    changed = projection.clone();
    changed.lines[0].group = id(250);
    assert_ne!(
        actual_departure_projection_digest(
            &input,
            plan.report.id,
            plan.historical_source.historical(),
            &changed
        )
        .unwrap(),
        digest
    );
    changed = projection.clone();
    changed.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_ne!(
        actual_departure_projection_digest(
            &input,
            plan.report.id,
            plan.historical_source.historical(),
            &changed
        )
        .unwrap(),
        digest
    );
    changed = projection;
    changed.occurred = known(121);
    assert_ne!(
        actual_departure_projection_digest(
            &input,
            plan.report.id,
            plan.historical_source.historical(),
            &changed
        )
        .unwrap(),
        digest
    );
}

// Constants measured from a fresh pure-domain build of published604cf4d0.
// No retained schema world or old retained executable was opened/run.
#[test]
fn legacy_actual_result_seals_match_published_604cf4d0_goldens() {
    let (f, source, births) = native_like_fixture();
    let command = command(&f, &source);
    let context = context(&f, &source);
    let plan = command.plan(&context, id(230), owner(), at(170)).unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let digest = actual_departure_result_digest(
        &input,
        &command,
        &context,
        &births,
        &findings(&f, &births),
        &plan,
    )
    .unwrap();
    assert_eq!(
        digest
            .iter()
            .map(|n| format!("{n:02x}"))
            .collect::<String>(),
        "f2aade84463f6d871cb8f22d28d718e09bc256defb4aa30271759a05d399406f"
    );
    let known = ActualKnownDeparture {
        actor: owner(),
        operation: id(240),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(3).unwrap(),
        catalog_revision: f.catalog.revision(),
        trace: f.graph.trace(),
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(f.facts.recipient).unwrap().clone(),
        occurred: known(120),
        recorded_at: at(130),
        lines: vec![ActualKnownDepartureLine {
            group: id(245),
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
            "This preserved witness describes the same immutable existing departure",
        ),
    };
    let plan = command
        .plan(&f.original, &known, None, id(231), owner(), at(170))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let digest = actual_duplicate_result_digest(
        &input,
        &command,
        &plan,
        HubRevision::from_value(7).unwrap(),
        f.catalog.revision(),
    )
    .unwrap();
    assert_eq!(
        digest
            .iter()
            .map(|n| format!("{n:02x}"))
            .collect::<String>(),
        "54ead350607ce9155645ffd761286b668a0d79b664d514d0ec425dc671d39009"
    );
}

#[test]
fn qualified_empty_known_history_has_a_distinct_seal_from_legacy_absence() {
    let (f, source, births) = native_like_fixture();
    let command = command(&f, &source);
    let context = context(&f, &source);
    let set = ActualKnownInterpretations {
        hub: f.graph.hub(),
        stock_revision: f.graph.revision(),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        records: vec![],
    };
    let legacy = command.plan(&context, id(230), owner(), at(170)).unwrap();
    let qualified = command
        .plan_with_interpretations(&context, &set, id(230), owner(), at(170))
        .unwrap();
    assert_eq!(qualified, legacy);
    let input = command.fingerprint(f.graph.hub(), owner(), legacy.operation);
    let findings = findings(&f, &births);
    let old =
        actual_departure_result_digest(&input, &command, &context, &births, &findings, &legacy)
            .unwrap();
    let new = actual_departure_result_digest_with_interpretations(
        &input, &command, &context, &set, &births, &findings, &qualified,
    )
    .unwrap();
    assert_ne!(old, new);
    let mut different = set.clone();
    different.latest_recorded_at = at(161);
    assert_ne!(
        actual_departure_result_digest_with_interpretations(
            &input, &command, &context, &different, &births, &findings, &qualified
        )
        .unwrap(),
        new
    );
    different.latest_recorded_at = at(149);
    assert!(
        actual_departure_result_digest_with_interpretations(
            &input, &command, &context, &different, &births, &findings, &qualified
        )
        .is_err()
    );
    let mut changed = qualified;
    changed.duplicate_review_basis = note("Changed review evidence is not the original command");
    assert!(
        actual_departure_result_digest_with_interpretations(
            &input, &command, &context, &set, &births, &findings, &changed
        )
        .is_err()
    );
}

#[test]
fn qualified_departure_seal_binds_nonmatching_current_known_facts_without_rewriting_the_plan() {
    use crate::hub::corrections::{
        CorrectedDepartureFamily, DepartureQuantityAccount, DepartureQuantityAmendment,
        DepartureQuantityDelta, OriginalDepartureQuantity, OriginalRecipientAttribution,
    };
    let (f, source, births) = native_like_fixture();
    let original = ActualKnownDeparture {
        actor: owner(),
        operation: id(240),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(3).unwrap(),
        catalog_revision: f.catalog.revision(),
        trace: f.graph.trace(),
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(f.facts.recipient).unwrap().clone(),
        occurred: known(100),
        recorded_at: at(130),
        lines: vec![ActualKnownDepartureLine {
            group: id(245),
            location: f.catalog.get(f.group.location).unwrap().clone(),
            packs: packs(2),
        }],
    };
    let quantity = OriginalDepartureQuantity {
        operation: original.operation,
        group: id(245),
        trace: original.trace,
        variant: original.variant.id,
        stock_revision: original.stock_revision,
        packs: packs(2),
    };
    let interpretation = ActualKnownInterpretation {
        stock_revision: f.graph.revision(),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        recipient_original: OriginalRecipientAttribution {
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
        },
        recipient_history: vec![],
        quantities: vec![DepartureQuantityAccount::restore(quantity, vec![], packs(0)).unwrap()],
        original: original.clone(),
    };
    let originals = [original];
    let mut context = context(&f, &source);
    context.known_departures = &originals;
    let set = ActualKnownInterpretations {
        hub: f.graph.hub(),
        stock_revision: f.graph.revision(),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        records: vec![interpretation],
    };
    let command = command(&f, &source);
    let plan = command
        .plan_with_interpretations(&context, &set, id(230), owner(), at(170))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
    let findings = findings(&f, &births);
    let digest = actual_departure_result_digest_with_interpretations(
        &input, &command, &context, &set, &births, &findings, &plan,
    )
    .unwrap();
    let mut changed = set.clone();
    changed.records[0].quantities[0] = DepartureQuantityAccount::restore(
        quantity,
        vec![DepartureQuantityAmendment {
            operation: id(79),
            stock_revision: f.graph.revision(),
            before: packs(2),
            after: packs(1),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(1)),
        }],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        command
            .plan_with_interpretations(&context, &changed, id(230), owner(), at(170))
            .unwrap(),
        plan
    );
    assert_ne!(
        actual_departure_result_digest_with_interpretations(
            &input, &command, &context, &changed, &births, &findings, &plan
        )
        .unwrap(),
        digest
    );
    changed.records.clear();
    assert!(
        actual_departure_result_digest_with_interpretations(
            &input, &command, &context, &changed, &births, &findings, &plan
        )
        .is_err()
    );
}
