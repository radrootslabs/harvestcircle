use super::*;
use crate::hub::stock::tests::{Fixture, command, now, operation, other, owner, query, setup};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::SafeErrorCode;

async fn active(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}

#[tokio::test]
async fn native_count_review_reads_pending_admission_and_confirmed_resolution_without_stock_effects()
 {
    use harvestcircle_domain::hub::{
        counts::{CountBlockReason, CountObserveCommand},
        reference::{SourceProvenance, WholePacks},
        stock::Occurrence,
    };
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let count = db
        .observe_hub_count(
            owner(),
            operation(2),
            CountObserveCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: catalog.revision(),
                group: page.rows[0].position.group,
                expected_group_revision: page.rows[0].position.revision,
                counted_packs: WholePacks::from_value(18).unwrap(),
                cutoff: Occurrence::Known(now()),
                observation: SourceProvenance::new(
                    "Fictional count before a later interrupted submission",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    assert!(
        db.hub_count_review(owner(), count.original.id)
            .await
            .unwrap()
            .assessment
            .ready()
    );
    let scope = active(&db).await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut input = command(&catalog);
    input.expected_stock_revision = before.revision;
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(3),
                    HubMutation::Receive(Box::new(input)),
                    now(),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(decision, admission::Decision::Fresh(_)));
    drop(decision);
    let blocked = db
        .hub_count_review(owner(), count.original.id)
        .await
        .unwrap();
    assert_eq!(
        blocked.assessment.blockers,
        vec![CountBlockReason::UncertainPriorOutcome]
    );
    assert_eq!(blocked.observation, count.original);
    let posting = harvestcircle_domain::hub::counts::CountPostCommand {
        expected_stock_revision: before.revision,
        expected_catalog_revision: catalog.revision(),
        count: count.original.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: SourceProvenance::new("Review cannot override a pending native submission")
            .unwrap(),
    };
    assert_eq!(
        db.apply_hub_count_post(owner(), operation(4), posting.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(4),
            HubMutation::CountPosting(Box::new(posting.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_submission(owner(), operation(4))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    // The test deliberately discarded the fresh grant before any effect, then
    // records that confirmed no-effect rejection through the production writer.
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::reject_on(tx, &operation(3), stock::conflict(), now()).await
            })
        })
        .await
        .unwrap();
    assert!(
        db.hub_count_review(owner(), count.original.id)
            .await
            .unwrap()
            .assessment
            .ready()
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    let result = db
        .submit_hub_mutation(
            scope,
            operation(4),
            HubMutation::CountPosting(Box::new(posting)),
            now(),
        )
        .await
        .unwrap();
    assert!(matches!(result, HubMutationResult::CountPosted(_)));
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        18
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn new_admission_records_one_original_and_replay_never_writes_another_effect() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let scope = active(&db).await;
    let input = command(&catalog);
    let result = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input.clone())),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::Received(first) = result else {
        panic!("receipt")
    };
    let record = db
        .hub_submission(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.status, HubSubmissionStatus::Recorded);
    assert_eq!(record.scope, scope);
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let replay = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input.clone())),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::Received(replay) = replay else {
        panic!("receipt")
    };
    assert_eq!(first, replay);
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    let mut changed = input;
    changed.packs = harvestcircle_domain::hub::reference::WholePacks::from_value(8).unwrap();
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(changed)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_admission_is_durable_unknown_and_cannot_produce_a_second_grant_after_restart() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let scope = active(&db).await;
    let input = command(&catalog);
    let owned = input.clone();
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1),
                    HubMutation::Receive(Box::new(owned)),
                    now(),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(decision, admission::Decision::Fresh(_)));
    drop(decision);
    assert_eq!(
        db.hub_submission(owner(), operation(1))
            .await
            .unwrap()
            .unwrap()
            .status,
        HubSubmissionStatus::Pending
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::Receive(Box::new(input.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
    let db = f.open().await;
    let fresh = active(&db).await;
    assert_ne!(scope.runtime, fresh.runtime);
    assert_eq!(
        db.submit_hub_mutation(
            fresh,
            operation(1),
            HubMutation::Receive(Box::new(input)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn stale_and_foreign_scope_cannot_admit_or_apply_and_legacy_originals_remain_read_only_replay()
 {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    let original = db
        .apply_hub_receipt(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    let scope = active(&db).await;
    let result = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input.clone())),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::Received(replayed) = result else {
        panic!("receipt")
    };
    assert_eq!(*replayed, original);
    assert!(
        db.hub_submission(owner(), operation(1))
            .await
            .unwrap()
            .is_none()
    );
    let newer = db
        .advance_hub_session(scope.runtime, scope.generation, Some(other()), now())
        .await
        .unwrap();
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::Receive(Box::new(input.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let foreign = HubSubmissionScope {
        runtime: scope.runtime,
        generation: newer,
        actor: other(),
    };
    assert_eq!(
        db.submit_hub_mutation(
            foreign,
            operation(2),
            HubMutation::Receive(Box::new(input)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        20
    );
    db.close().await.unwrap();
}

async fn prepare(
    db: &Database,
    scope: HubSubmissionScope,
    op: u32,
    mutation: HubMutation,
) -> admission::FreshAdmission {
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(tx, scope, operation(op), mutation, now()).await
            })
        })
        .await
        .unwrap();
    match decision {
        admission::Decision::Fresh(grant) => grant,
        _ => panic!("new admission"),
    }
}

#[tokio::test]
async fn confirmed_rejection_retains_exact_submitted_semantics_without_holding_a_later_corrected_intent()
 {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let scope = active(&db).await;
    let mut input = command(&catalog);
    input.packs = harvestcircle_domain::hub::reference::WholePacks::from_value(0).unwrap();
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    let rejected = db
        .hub_submission(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rejected.status,
        HubSubmissionStatus::Rejected(SafeErrorCode::HubInvalidInput)
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_submission(owner(), operation(1))
            .await
            .unwrap()
            .unwrap(),
        rejected
    );
    db.submit_hub_mutation(
        scope,
        operation(2),
        HubMutation::Receive(Box::new(command(&catalog))),
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        20
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn an_old_fresh_grant_cannot_commit_after_the_session_changes() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let scope = active(&db).await;
    let grant = prepare(
        &db,
        scope,
        1,
        HubMutation::Receive(Box::new(command(&catalog))),
    )
    .await;
    db.advance_hub_session(scope.runtime, scope.generation, None, now())
        .await
        .unwrap();
    assert_eq!(
        execute::execute_fresh(&db, grant, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let record = db
        .hub_submission(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.status,
        HubSubmissionStatus::Rejected(SafeErrorCode::StorageUnavailable)
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn a_newly_pending_precautionary_hold_blocks_an_already_admitted_move_before_any_effect() {
    use harvestcircle_domain::hub::{
        movement::{HandlingCommand, MovementCommand},
        reference::SourceProvenance,
        stock::Occurrence,
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt_input = command(&catalog);
    let target = receipt_input.planned_location.unwrap();
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receipt_input, now())
        .await
        .unwrap();
    let scope = active(&db).await;
    let group = receipt.original.position.group;
    let stock = receipt.original.stock_revision;
    let note =
        SourceProvenance::new("Fictional native interleaving without a physical move").unwrap();
    let movement = HandlingCommand::Move(MovementCommand {
        expected_stock_revision: stock,
        expected_catalog_revision: catalog.revision(),
        group,
        expected_group_revision: HubRevision::INITIAL,
        to: target,
        packs: receipt.original.position.packs,
        separated_portion: None,
        occurred: Occurrence::Known(now()),
        observation: note.clone(),
    });
    let move_grant = prepare(&db, scope, 2, HubMutation::Handling(Box::new(movement))).await;
    let hold = HandlingCommand::Treatment(TreatmentCommand {
        expected_stock_revision: stock,
        expected_catalog_revision: catalog.revision(),
        action: TreatmentAction::PlaceHold {
            selection: TreatmentSelection::new(vec![group]).unwrap(),
        },
        occurred: Occurrence::Known(now()),
        observation: note,
    });
    let hold_grant = prepare(&db, scope, 3, HubMutation::Handling(Box::new(hold))).await;
    assert_eq!(
        execute::execute_fresh(&db, move_grant, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    execute::execute_fresh(&db, hold_grant, now())
        .await
        .unwrap();
    assert_eq!(
        db.hub_submission(owner(), operation(2))
            .await
            .unwrap()
            .unwrap()
            .status,
        HubSubmissionStatus::Rejected(SafeErrorCode::StorageUnavailable)
    );
    assert_eq!(
        db.hub_submission(owner(), operation(3))
            .await
            .unwrap()
            .unwrap()
            .status,
        HubSubmissionStatus::Recorded
    );
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        after.rows[0].position.location,
        receipt.original.position.location
    );
    assert_eq!(after.rows[0].position.packs.value(), 20);
    db.close().await.unwrap();
}
