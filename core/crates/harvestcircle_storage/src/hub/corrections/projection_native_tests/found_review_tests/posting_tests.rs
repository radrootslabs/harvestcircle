//! Legal product submission and immutable original recovery, never seeded success.
use super::*;
use harvestcircle_domain::hub::corrections::FoundCorrectionCommand;
mod composition_tests;
mod consumer_tests;
mod fault_tests;

pub(in crate::hub::corrections) async fn posted_fixture()
-> (Fixture, Database, Box<crate::HubFoundCorrectionReceipt>) {
    let world = world(2, PriorReturn::TwoEstablished).await;
    let receipt = post_world(&world).await;
    (world.fixture, world.db, receipt)
}

pub(in crate::hub) async fn unposted_fixture() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    FoundCorrectionCommand,
) {
    let world = world(2, PriorReturn::None).await;
    let review = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(200))
        .await
        .unwrap();
    assert!(matches!(review.outcome, Outcome::Candidate(_)));
    let command = FoundCorrectionCommand {
        observation_operation: review.observation.original.operation,
        expected_stock_revision: review.revision,
        expected_catalog_revision: review.catalog_revision,
    };
    (world.fixture, world.db, world.scope, command)
}

#[tokio::test]
async fn found_against_reviewed_actual_preserves_original_facts_and_work_after_full_recovery() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, packs) = super::super::actual_debit(&db).await;
    assert_eq!(packs, 2);
    let runtime = db.begin_hub_runtime(at(170)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(170),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let target_id = crate::hub::actuals::common::operation_id(&target).unwrap();
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: target_id,
            target_group: before.id,
            basis: note("Exact reviewed actual debit; original unknown occurrence remains"),
            allocations: vec![FoundStockInput {
                debit: PhysicalDebitKey {
                    operation: target_id,
                    group: before.id,
                },
                packs: WholePacks::from_value(2).unwrap(),
                location: before.location,
                identification: PhysicalIdentification {
                    label: HubLabel::new("Independently found actual two crates").unwrap(),
                    observation: note("Present physical identity independently observed"),
                },
                cohort_evidence: note("Exact original reviewed actual physical debit"),
            }],
        },
    };
    let crate::HubMutationResult::CorrectionObserved(observation) = db
        .submit_hub_mutation(
            scope,
            operation(1600),
            HubMutation::CorrectionObservation(Box::new(input)),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("raw fact")
    };
    let query = HubFoundCorrectionReviewQuery {
        observation_operation: operation(1600),
        proposed_operation: operation(1700),
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    let review = db
        .hub_found_correction_review(owner(), query, at(200))
        .await
        .unwrap();
    let Outcome::Candidate(expected) = review.outcome else {
        panic!("actual candidate")
    };
    let command = FoundCorrectionCommand {
        observation_operation: observation.original.operation,
        expected_stock_revision: review.revision,
        expected_catalog_revision: review.catalog_revision,
    };
    let original_snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: before.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let work = work_rows(crate::hub::custody::tests::material(&fixture).await);
    let crate::HubMutationResult::FoundCorrected(receipt) = db
        .submit_hub_mutation(
            scope,
            operation(1700),
            HubMutation::FoundCorrection(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("actual found posted")
    };
    assert_eq!(receipt.original, *expected);
    let known = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace: before.trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(known.departures.len(), 1);
    assert_eq!(known.departures[0].original.lines[0].packs.value(), 2);
    assert_eq!(
        known.departures[0].quantities[0].amendments()[0]
            .after
            .value(),
        0
    );
    assert_eq!(
        known.departures[0].funding[0].available().unwrap().value(),
        0
    );
    assert_eq!(receipt.observation, *observation);
    assert_eq!(
        (
            receipt.original.amendment.before.value(),
            receipt.original.amendment.after.value()
        ),
        (2, 0)
    );
    assert_eq!(
        receipt.original.family,
        harvestcircle_domain::hub::corrections::CorrectedDepartureFamily::ReviewedActual
    );
    assert!(receipt.original.allocations[0].group.work.is_empty());
    assert_eq!(
        work_rows(crate::hub::custody::tests::material(&fixture).await),
        work
    );
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: before.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        snapshot.actual_departures,
        original_snapshot.actual_departures
    );
    assert_eq!(
        (
            snapshot.balance().unwrap().on_hand.value(),
            snapshot.balance().unwrap().actual_departed.value()
        ),
        (20, 0)
    );
    assert_eq!(snapshot.quantity_corrections.len(), 1);
    assert_eq!(
        (
            snapshot.quantity_corrections[0].original().packs.value(),
            snapshot.quantity_corrections[0].effective().value()
        ),
        (2, 0)
    );
    let material = crate::hub::custody::tests::material(&fixture).await;
    let crate::HubMutationResult::FoundCorrected(replayed) = db
        .submit_hub_mutation(
            scope,
            operation(1700),
            HubMutation::FoundCorrection(Box::new(command)),
            at(201),
        )
        .await
        .unwrap()
    else {
        panic!("original replay")
    };
    assert_eq!(replayed, receipt);
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), operation(1700))
            .await
            .unwrap(),
        Some(*receipt)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    reopened.close().await.unwrap();
}

fn work_rows(rows: Vec<(String, Vec<String>)>) -> Vec<(String, Vec<String>)> {
    rows.into_iter()
        .filter(|(table, _)| {
            matches!(
                table.as_str(),
                "hub_work_requirements"
                    | "hub_group_work"
                    | "hub_work_changes"
                    | "hub_work_actions"
                    | "hub_action_groups"
            )
        })
        .collect()
}

#[tokio::test]
async fn legal_found_post_creates_restricted_child_and_recovers_original_without_revival_or_work_credit()
 {
    let world = world(2, PriorReturn::None).await;
    let review = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(200))
        .await
        .unwrap();
    let Outcome::Candidate(expected) = review.outcome else {
        panic!("review candidate")
    };
    let command = FoundCorrectionCommand {
        observation_operation: review.observation.original.operation,
        expected_stock_revision: review.revision,
        expected_catalog_revision: review.catalog_revision,
    };
    let work = work_rows(crate::hub::custody::tests::material(&world.fixture).await);
    let issued = world.departure.record.paper.versions.last().unwrap().id;
    let issued_before = harvestcircle_application::hub_output::identification_output(
        &world.departure.record.paper,
        issued,
    )
    .unwrap();
    let crate::HubMutationResult::FoundCorrected(receipt) = world
        .db
        .submit_hub_mutation(
            world.scope,
            world.query.proposed_operation.clone(),
            HubMutation::FoundCorrection(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("posted found")
    };
    assert_eq!(receipt.original, *expected);
    assert_eq!(receipt.observation, world.observation);
    assert_eq!(
        world
            .db
            .hub_departure_operation(owner(), world.departure.operation.clone())
            .await
            .unwrap()
            .unwrap(),
        world.departure
    );
    assert_eq!(
        work_rows(crate::hub::custody::tests::material(&world.fixture).await),
        work
    );
    let page = world
        .db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        16
    );
    let child = &receipt.original.allocations[0].group;
    assert_eq!(
        page.rows
            .iter()
            .find(|r| r.position.group == child.id)
            .unwrap()
            .trace
            .original_packs
            .value(),
        20
    );
    assert_eq!(
        world.db.hub_group(owner(), child.id).await.unwrap().group,
        *child
    );
    assert_eq!(
        receipt
            .original
            .after_treatment
            .project(
                &receipt.original.after_lineage,
                SampleHandlingBasis::WHOLE_PACK_V1
            )
            .unwrap()
            .summary
            .review_required
            .value(),
        2
    );
    let snapshot = world
        .db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: child.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.departed.value()
        ),
        (20, 16, 4)
    );
    assert_eq!(snapshot.exits[0].packs.value(), 6);
    assert_eq!(snapshot.quantity_corrections.len(), 1);
    assert_eq!(
        (
            snapshot.quantity_corrections[0].original().packs.value(),
            snapshot.quantity_corrections[0].effective().value()
        ),
        (6, 4)
    );
    let output = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
    assert!(
        output
            .html
            .contains("Current effective whole packs:</strong> 4")
    );
    assert!(output.html.contains("Original whole packs:</strong> 6"));
    let retained = world
        .db
        .hub_departure_operation(owner(), world.departure.operation.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        harvestcircle_application::hub_output::identification_output(
            &retained.record.paper,
            issued
        )
        .unwrap()
        .html,
        issued_before.html
    );
    let history = world
        .db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace: child.trace,
                expected_revision: Some(snapshot.stock_revision),
                expected_catalog_revision: Some(snapshot.catalog_revision),
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        history
            .rows
            .iter()
            .filter(|row| row.kind == crate::HubTraceHistoryKind::CorrectionObserved)
            .count(),
        1
    );
    assert_eq!(
        history
            .rows
            .iter()
            .filter(|row| row.kind == crate::HubTraceHistoryKind::FoundCorrected)
            .count(),
        1
    );
    let observed = history
        .rows
        .iter()
        .find(|row| row.operation == world.observation.operation)
        .unwrap();
    assert_eq!(observed.occurred, None);
    let posted = history
        .rows
        .iter()
        .find(|row| row.operation == receipt.operation)
        .unwrap();
    assert_eq!(posted.occurred, Some(Occurrence::Known(at(200))));
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt.clone())
    );
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let crate::HubMutationResult::FoundCorrected(replayed) = world
        .db
        .submit_hub_mutation(
            world.scope,
            receipt.operation.clone(),
            HubMutation::FoundCorrection(Box::new(command)),
            at(201),
        )
        .await
        .unwrap()
    else {
        panic!("original replay")
    };
    assert_eq!(replayed, receipt);
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    assert!(receipt.impact.rows().iter().any(|target| matches!(
        target,
        harvestcircle_domain::hub::corrections::CorrectionImpact::IssuedVersion(_)
    )));
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    reopened.close().await.unwrap();
}

async fn post_world(world: &ReviewWorld) -> Box<crate::HubFoundCorrectionReceipt> {
    post_world_at(world, 200).await
}

async fn post_world_at(world: &ReviewWorld, time: i64) -> Box<crate::HubFoundCorrectionReceipt> {
    let review = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(time))
        .await
        .unwrap();
    assert!(matches!(review.outcome, Outcome::Candidate(_)));
    let command = FoundCorrectionCommand {
        observation_operation: review.observation.original.operation,
        expected_stock_revision: review.revision,
        expected_catalog_revision: review.catalog_revision,
    };
    let crate::HubMutationResult::FoundCorrected(receipt) = world
        .db
        .submit_hub_mutation(
            world.scope,
            world.query.proposed_operation.clone(),
            HubMutation::FoundCorrection(Box::new(command)),
            at(time),
        )
        .await
        .unwrap()
    else {
        panic!("found posted")
    };
    receipt
}

#[tokio::test]
async fn later_acceptance_relocation_and_complete_disposition_do_not_rewrite_found_original() {
    use harvestcircle_domain::hub::{
        disposition::{DispositionCommand, DispositionKind},
        movement::{HandlingCommand, MovementCommand},
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let world = world(2, PriorReturn::Established).await;
    let receipt = post_world(&world).await;
    let child = receipt.original.allocations[0].group.id;
    let current = world.db.hub_group(owner(), child).await.unwrap();
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1800),
            HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: TreatmentAction::SetAcceptance {
                    selection: TreatmentSelection::new(vec![child]).unwrap(),
                    status: AcceptanceStatus::Accepted,
                },
                occurred: Occurrence::Known(at(210)),
                observation: note(
                    "Later explicit current acceptance does not amend the initial negative review",
                ),
            }))),
            at(210),
        )
        .await
        .unwrap();
    let current = world.db.hub_group(owner(), child).await.unwrap();
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1801),
            HubMutation::Handling(Box::new(HandlingCommand::Move(MovementCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: child,
                expected_group_revision: current.group.revision,
                to: if current.group.location == id(7) {
                    id(6)
                } else {
                    id(7)
                },
                packs: current.group.packs,
                separated_portion: None,
                occurred: Occurrence::Known(at(220)),
                observation: note("Later actual complete found-child relocation"),
            }))),
            at(220),
        )
        .await
        .unwrap();
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt.clone())
    );
    let current = world.db.hub_group(owner(), child).await.unwrap();
    assert!(current.group.work.is_empty());
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1802),
            HubMutation::Disposition(Box::new(DispositionCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: child,
                expected_group_revision: current.group.revision,
                packs: current.group.packs,
                kind: DispositionKind::Destroyed,
                occurred: Occurrence::Known(at(230)),
                observation: note(
                    "Separately observed later complete disposition, never a departure correction",
                ),
            })),
            at(230),
        )
        .await
        .unwrap();
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt.clone())
    );
    assert_eq!(
        world.db.hub_group(owner(), child).await.unwrap_err().code(),
        SafeErrorCode::HubConflict
    );
    let snapshot = world
        .db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: receipt.original.original_target.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    // The established return is a separate population. This original trace
    // retains fourteen on hand, four net departed and two later disposed.
    assert_eq!(
        (
            snapshot.balance().unwrap().on_hand.value(),
            snapshot.balance().unwrap().departed.value(),
            snapshot.balance().unwrap().disposed.value()
        ),
        (14, 4, 2)
    );
    let material = crate::hub::custody::tests::material(&world.fixture).await;
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        material
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn found_original_rejects_extra_work_and_old_requirement_revival_without_repair() {
    use sqlx::Connection;
    for fault in 0..3 {
        let world = world(2, PriorReturn::None).await;
        let receipt = post_world(&world).await;
        let child = receipt.original.allocations[0].group.id;
        let mut raw = world.fixture.raw().await;
        match fault {
            0 => {
                sqlx::query("INSERT INTO hub_work_actions (operation_id,previous_work_id,expected_work_revision,replacement_work_id,observation) SELECT ?,NULL,NULL,work_id,'Unexpected found work action' FROM hub_work_requirements LIMIT 1").bind(receipt.operation.as_str()).execute(&mut raw).await.unwrap();
            }
            1 => {
                sqlx::query("INSERT INTO hub_action_groups (operation_id,group_id,before_revision,scoped_packs,location_id,location_revision) VALUES (?,?,1,2,?,1)").bind(receipt.operation.as_str()).bind(child.as_bytes().as_slice()).bind(receipt.original.allocations[0].location.id.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
            }
            2 => {
                sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) SELECT ?,work_id,'pending',2 FROM hub_work_requirements LIMIT 1").bind(child.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
            }
            _ => unreachable!(),
        }
        raw.close().await.unwrap();
        let material = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            world
                .db
                .hub_found_correction_original(owner(), receipt.operation.clone())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            world
                .db
                .hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            material
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn exact_return_reservation_limits_found_credit_without_double_deducting_the_original() {
    let world = world(4, PriorReturn::Established).await;
    let receipt = post_world(&world).await;
    assert_eq!(
        (
            receipt.original.amendment.before.value(),
            receipt.original.amendment.after.value()
        ),
        (6, 2)
    );
    let funding = world
        .db
        .hub_correction_target_funding(
            owner(),
            crate::HubCorrectionFundingQuery {
                target_operation: world.departure.operation.clone(),
                original_group: world.departure.record.consumed[0].before.id,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(funding.quantity.established_returns().value(), 2);
    assert_eq!(funding.debits[0].available().unwrap().value(), 0);
    let snapshot = world
        .db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: receipt.original.original_target.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (
            snapshot.balance().unwrap().on_hand.value(),
            snapshot.balance().unwrap().departed.value()
        ),
        (18, 2)
    );
    assert_eq!(
        world
            .db
            .hub_return_operation(owner(), world.returned.as_ref().unwrap().operation.clone())
            .await
            .unwrap()
            .unwrap(),
        *world.returned.as_ref().unwrap()
    );
    world.db.close().await.unwrap();
}

async fn two_found_world() -> ReviewWorld {
    let mut world = world(2, PriorReturn::None).await;
    let mut next = world.observation.original.observation.clone();
    next.previous = Some(world.observation.original.operation);
    let CorrectionObservedFacts::Found { allocations, .. } = &mut next.facts else {
        unreachable!()
    };
    allocations[0].packs = WholePacks::from_value(1).unwrap();
    let mut second = allocations[0].clone();
    second.identification.label = HubLabel::new("Second independently found crate").unwrap();
    allocations.push(second);
    let crate::HubMutationResult::CorrectionObserved(observed) = world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1601),
            HubMutation::CorrectionObservation(Box::new(next)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("new factual head")
    };
    world.observation = *observed;
    world.query.observation_operation = operation(1601);
    world
}

#[tokio::test]
async fn two_found_allocations_form_distinct_empty_work_children_under_one_negative_review() {
    let world = two_found_world().await;
    let receipt = post_world(&world).await;
    assert_eq!(receipt.original.allocations.len(), 2);
    assert_ne!(
        receipt.original.allocations[0].group.id,
        receipt.original.allocations[1].group.id
    );
    assert!(
        receipt
            .original
            .allocations
            .iter()
            .all(|a| a.group.work.is_empty() && a.group.revision == HubRevision::INITIAL)
    );
    assert_eq!(receipt.original.review_required.scope.anchors().len(), 2);
    assert_eq!(
        receipt
            .original
            .after_treatment
            .project(
                &receipt.original.after_lineage,
                SampleHandlingBasis::WHOLE_PACK_V1
            )
            .unwrap()
            .summary
            .review_required
            .value(),
        2
    );
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt)
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn maximum_sixty_four_found_allocations_post_atomically_without_restoring_old_work() {
    let mut world = world_with_quantities(64, PriorReturn::None, 128, 64).await;
    let mut next = world.observation.original.observation.clone();
    next.previous = Some(world.observation.original.operation);
    let CorrectionObservedFacts::Found { allocations, .. } = &mut next.facts else {
        unreachable!()
    };
    let template = allocations[0].clone();
    *allocations = (0..64)
        .map(|n| {
            let mut a = template.clone();
            a.packs = WholePacks::from_value(1).unwrap();
            a.identification.label =
                HubLabel::new(format!("Independently observed found crate {n:02}")).unwrap();
            a
        })
        .collect();
    let crate::HubMutationResult::CorrectionObserved(observed) = world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1601),
            HubMutation::CorrectionObservation(Box::new(next)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("bounded new head")
    };
    world.observation = *observed;
    world.query.observation_operation = operation(1601);
    let work = work_rows(crate::hub::custody::tests::material(&world.fixture).await);
    let receipt = post_world(&world).await;
    assert_eq!(receipt.original.allocations.len(), 64);
    assert_eq!(
        receipt
            .original
            .allocations
            .iter()
            .map(|a| a.group.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        64
    );
    assert!(
        receipt
            .original
            .allocations
            .iter()
            .all(|a| a.group.work.is_empty()
                && a.group.packs.value() == 1
                && a.group.revision == HubRevision::INITIAL)
    );
    assert_eq!(receipt.original.review_required.scope.anchors().len(), 64);
    assert_eq!(
        (
            receipt.original.amendment.before.value(),
            receipt.original.amendment.after.value()
        ),
        (64, 0)
    );
    assert_eq!(
        work_rows(crate::hub::custody::tests::material(&world.fixture).await),
        work
    );
    let page = world
        .db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    // Stock is paged; verify the complete trace for all sixty-four children.
    assert!(page.total > 0);
    let snapshot = world
        .db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: receipt.original.original_target.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(snapshot.on_hand.len(), 65);
    assert_eq!(
        (
            snapshot.balance().unwrap().on_hand.value(),
            snapshot.balance().unwrap().departed.value()
        ),
        (128, 0)
    );
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt.clone())
    );
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), receipt.operation.clone())
            .await
            .unwrap(),
        Some(*receipt)
    );
    reopened.close().await.unwrap();
}
