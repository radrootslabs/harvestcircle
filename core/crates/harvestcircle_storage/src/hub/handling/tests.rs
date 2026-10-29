use super::*;
use crate::hub::stock::tests::{Fixture, command, now, operation, other, owner, query, setup};
use harvestcircle_domain::hub::movement::{DiscrepancyAction, DiscrepancyCommand};
use harvestcircle_domain::hub::movement::{MovementCommand, PhysicalIdentification};
use harvestcircle_domain::hub::movement::{NewWorkCommand, ReplanCommand};
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::reference::{
    LocationPurpose, ReferenceCatalog, ReferenceCommand, ReferenceMutation, ReferenceValue,
};

fn movement(
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
    group: &PhysicalStockGroup,
    revision: HubRevision,
    to: HubRecordId,
    packs: u32,
) -> MovementCommand {
    MovementCommand {
        expected_stock_revision: revision,
        expected_catalog_revision: catalog.revision(),
        group: group.id,
        expected_group_revision: group.revision,
        to,
        packs: WholePacks::from_value(packs).unwrap(),
        separated_portion: (packs < group.packs.value()).then(|| PhysicalIdentification {
            label: HubLabel::new("Blue eight").unwrap(),
            observation: SourceProvenance::new("Eight separately tagged crates in the blue stack")
                .unwrap(),
        }),
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Manually moved the selected original crates").unwrap(),
    }
}

#[tokio::test]
async fn current_group_and_work_read_real_receiving_allocations() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let group = db
        .hub_group(owner(), receipt.original.position.group)
        .await
        .unwrap();
    assert_eq!(group.group.packs.value(), 20);
    assert_eq!(
        group.group.location,
        receipt.original.trace.initial_location.id
    );
    assert_eq!(group.group.work.len(), 1);
    assert_eq!(
        group.group.work[0].requirement.quantity.remaining().value(),
        20
    );
    let page = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].origin, HubWorkOrigin::Intake);
    assert_eq!(page.rows[0].pending_groups, 1);
    assert!(page.rows[0].source_needs_review);
    assert_eq!(page.rows[0].work.id, receipt.original.trace.id);
    assert_eq!(page.rows[0].observation, receipt.original.trace.observation);
    assert_eq!(page.rows[0].created_operation, operation(1));
    assert_eq!(page.rows[0].created_by, owner());
    assert_eq!(
        db.hub_group(other(), receipt.original.position.group)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_work_page(other(), HubHandlingQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_group(owner(), receipt.original.position.group)
            .await
            .unwrap(),
        group
    );
    assert_eq!(
        reopened
            .hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap(),
        page
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn work_counter_without_matching_scope_is_rejected_on_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query(
        "UPDATE hub_work_requirements SET completed=1,remaining=19,revision=2 WHERE work_id=?",
    )
    .bind(receipt.original.trace.id.as_bytes().as_slice())
    .execute(&mut raw)
    .await
    .unwrap();
    sqlx::Connection::close(raw).await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_group(owner(), receipt.original.position.group)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        reopened
            .hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        reopened
            .hub_stock_page(owner(), query())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn actual_movement_conserves_stock_and_original_result_across_return_and_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = command(&catalog);
    let receiving = intake.receiving;
    let target = intake.planned_location.unwrap();
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let command = movement(&catalog, &source.group, source.revision, target, 8);
    let first = db
        .apply_hub_movement(owner(), operation(2), command.clone(), now())
        .await
        .unwrap();
    let HubHandlingResult::Moved(result) = &first.result else {
        panic!("movement result")
    };
    assert_eq!(result.remaining_packs.value(), 12);
    assert_eq!(result.packs.value(), 8);
    assert_eq!(result.work_changes[0].after.quantity.completed().value(), 8);
    assert_eq!(
        result.work_changes[0].after.quantity.remaining().value(),
        12
    );
    assert_eq!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .iter()
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        20
    );
    let moved = db.hub_group(owner(), result.moved_group).await.unwrap();
    let back = movement(&catalog, &moved.group, moved.revision, receiving, 8);
    db.apply_hub_movement(owner(), operation(3), back, now())
        .await
        .unwrap();
    let returned = db.hub_group(owner(), result.moved_group).await.unwrap();
    let again = movement(&catalog, &returned.group, returned.revision, target, 8);
    let again = db
        .apply_hub_movement(owner(), operation(4), again, now())
        .await
        .unwrap();
    let HubHandlingResult::Moved(again) = again.result else {
        panic!("movement result")
    };
    assert!(again.work_changes.is_empty());
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows[0]
            .work
            .quantity
            .completed()
            .value(),
        8
    );
    assert_eq!(
        db.apply_hub_movement(owner(), operation(2), command.clone(), now())
            .await
            .unwrap(),
        first
    );
    let mut changed = command.clone();
    changed.packs = WholePacks::from_value(9).unwrap();
    assert_eq!(
        db.apply_hub_movement(owner(), operation(2), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(first.clone())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        db.apply_hub_movement(owner(), operation(2), command, now())
            .await
            .unwrap(),
        first
    );
    let stock = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        stock
            .rows
            .iter()
            .filter(|row| row.position.location == receiving)
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        12
    );
    assert_eq!(
        stock
            .rows
            .iter()
            .filter(|row| row.position.location == target)
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        8
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_movement_after_all_production_writes_rolls_back_scope_and_progress() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = command(&catalog);
    let target = intake.planned_location.unwrap();
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let command = movement(&catalog, &source.group, source.revision, target, 8);
    let failure = db
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                write::apply_movement_on(transaction, owner(), operation(2), command, now())
                    .await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(
        db.hub_group(owner(), source.group.id).await.unwrap(),
        source
    );
    assert!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap().total, 1);
    db.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_stale_movements_leave_exactly_one_complete_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = command(&catalog);
    let target = intake.planned_location.unwrap();
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let command = movement(&catalog, &source.group, source.revision, target, 8);
    assert_eq!(
        db.apply_hub_movement(other(), operation(2), command.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let (one, two) = tokio::join!(
        db.apply_hub_movement(owner(), operation(2), command.clone(), now()),
        db.apply_hub_movement(owner(), operation(3), command, now())
    );
    assert_ne!(one.is_ok(), two.is_ok());
    assert_eq!(
        one.err().or_else(|| two.err()).unwrap().code(),
        SafeErrorCode::HubConflict
    );
    let page = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    assert_eq!(page.rows[0].work.quantity.completed().value(), 8);
    assert_eq!(page.rows[0].work.quantity.remaining().value(), 12);
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap().total, 2);
    db.close().await.unwrap();
}

#[tokio::test]
async fn movement_time_follows_its_portion_not_later_actions_on_the_remaining_parent() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut intake = command(&catalog);
    let receiving = intake.receiving;
    let target = intake.planned_location.unwrap();
    intake.occurred = Occurrence::Known(UnixTimestamp::from_seconds(80).unwrap());
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let mut forward = movement(&catalog, &source.group, source.revision, target, 8);
    forward.occurred = Occurrence::Known(UnixTimestamp::from_seconds(90).unwrap());
    let done = db
        .apply_hub_movement(owner(), operation(2), forward, now())
        .await
        .unwrap();
    let HubHandlingResult::Moved(done) = done.result else {
        panic!("movement result")
    };
    let parent = db.hub_group(owner(), source.group.id).await.unwrap();
    let mut parent_move = movement(&catalog, &parent.group, parent.revision, target, 12);
    parent_move.occurred = Occurrence::Known(UnixTimestamp::from_seconds(99).unwrap());
    db.apply_hub_movement(owner(), operation(3), parent_move, now())
        .await
        .unwrap();
    let child = db.hub_group(owner(), done.moved_group).await.unwrap();
    assert_eq!(
        child.group.causal_floor,
        Some(UnixTimestamp::from_seconds(90).unwrap())
    );
    let mut backward = movement(&catalog, &child.group, child.revision, receiving, 8);
    backward.occurred = Occurrence::Known(UnixTimestamp::from_seconds(89).unwrap());
    assert_eq!(
        db.apply_hub_movement(owner(), operation(4), backward.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    backward.occurred = Occurrence::Known(UnixTimestamp::from_seconds(95).unwrap());
    db.apply_hub_movement(owner(), operation(5), backward, now())
        .await
        .unwrap();
    let back = db.hub_group(owner(), done.moved_group).await.unwrap();
    let unknown = movement(&catalog, &back.group, back.revision, target, 8);
    db.apply_hub_movement(owner(), operation(6), unknown, now())
        .await
        .unwrap();
    assert_eq!(
        db.hub_group(owner(), done.moved_group)
            .await
            .unwrap()
            .group
            .causal_floor,
        Some(UnixTimestamp::from_seconds(95).unwrap())
    );
    db.close().await.unwrap();
}

async fn with_b3(db: &crate::Database, catalog: &ReferenceCatalog) -> ReferenceCatalog {
    db.apply_hub_reference(
        owner(),
        operation(5000),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Create {
                id: crate::hub::stock::tests::id(8),
                value: ReferenceValue::Location {
                    label: HubLabel::new("B3").unwrap(),
                    purpose: LocationPurpose::Storage,
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    db.hub_references(owner()).await.unwrap()
}

#[tokio::test]
async fn replanning_preserves_stock_and_completion_and_requires_actual_replacement_work() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = with_b3(&db, &setup(&db).await).await;
    let intake = command(&catalog);
    let target = intake.planned_location.unwrap();
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        movement(&catalog, &source.group, source.revision, target, 8),
        now(),
    )
    .await
    .unwrap();
    let before = db.hub_group(owner(), source.group.id).await.unwrap();
    let work = before.group.work[0].requirement.clone();
    let replan = ReplanCommand {
        expected_stock_revision: before.revision,
        expected_catalog_revision: catalog.revision(),
        requirement: work.id,
        expected_work_revision: work.revision,
        target: crate::hub::stock::tests::id(8),
        observation: SourceProvenance::new("Remaining crates require B3 instead of A3").unwrap(),
    };
    let recorded = db
        .apply_hub_replan(owner(), operation(3), replan.clone(), now())
        .await
        .unwrap();
    let after = db.hub_group(owner(), source.group.id).await.unwrap();
    assert_eq!(
        (after.group.packs, after.group.location),
        (before.group.packs, before.group.location)
    );
    let HubHandlingResult::WorkChanged(result) = &recorded.result else {
        panic!("work result")
    };
    let old = &result.previous.as_ref().unwrap().after.quantity;
    assert_eq!(
        (
            old.completed().value(),
            old.no_longer_required().value(),
            old.remaining().value()
        ),
        (8, 12, 0)
    );
    assert_eq!(result.replacement.quantity.remaining().value(), 12);
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows[0]
            .work
            .id,
        result.replacement.id
    );
    db.apply_hub_movement(
        owner(),
        operation(4),
        movement(
            &catalog,
            &after.group,
            after.revision,
            crate::hub::stock::tests::id(8),
            12,
        ),
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        db.apply_hub_replan(owner(), operation(3), replan.clone(), now())
            .await
            .unwrap(),
        recorded
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    let mut changed = replan.clone();
    changed.observation = SourceProvenance::new("Changed original reason").unwrap();
    assert_eq!(
        db.apply_hub_replan(owner(), operation(3), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(recorded)
    );
    assert_eq!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .iter()
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        20
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn new_handling_is_explicit_and_never_resets_completed_original_work() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = with_b3(&db, &setup(&db).await).await;
    let intake = command(&catalog);
    let target = intake.planned_location.unwrap();
    let received = db
        .apply_hub_receipt(owner(), operation(1), intake, now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let new = NewWorkCommand {
        expected_stock_revision: source.revision,
        expected_catalog_revision: catalog.revision(),
        groups: vec![source.group.id],
        target: crate::hub::stock::tests::id(8),
        observation: SourceProvenance::new("Explicit new handling to B3").unwrap(),
    };
    assert_eq!(
        db.apply_hub_new_work(owner(), operation(2), new.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    db.apply_hub_movement(
        owner(),
        operation(3),
        movement(&catalog, &source.group, source.revision, target, 20),
        now(),
    )
    .await
    .unwrap();
    let done = db.hub_group(owner(), source.group.id).await.unwrap();
    let new = NewWorkCommand {
        expected_stock_revision: done.revision,
        ..new
    };
    let recorded = db
        .apply_hub_new_work(owner(), operation(4), new.clone(), now())
        .await
        .unwrap();
    let group = db.hub_group(owner(), source.group.id).await.unwrap();
    assert_eq!(group.group.location, target);
    assert_eq!(group.group.packs.value(), 20);
    assert_eq!(
        group
            .group
            .work
            .iter()
            .find(|work| work.requirement.id == source.group.work[0].requirement.id)
            .unwrap()
            .requirement
            .quantity
            .completed()
            .value(),
        20
    );
    let HubHandlingResult::WorkChanged(result) = &recorded.result else {
        panic!("work result")
    };
    assert_eq!(result.replacement.quantity.completed().value(), 0);
    assert_eq!(result.replacement.quantity.remaining().value(), 20);
    assert!(result.previous.is_none());
    db.apply_hub_movement(
        owner(),
        operation(5),
        movement(
            &catalog,
            &group.group,
            group.revision,
            crate::hub::stock::tests::id(8),
            20,
        ),
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.apply_hub_new_work(owner(), operation(4), new, now())
            .await
            .unwrap(),
        recorded
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(4))
            .await
            .unwrap(),
        Some(recorded)
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .total,
        0
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_replan_rolls_back_retirement_replacement_and_group_scope() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = with_b3(&db, &setup(&db).await).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let work = &source.group.work[0].requirement;
    let replan = ReplanCommand {
        expected_stock_revision: source.revision,
        expected_catalog_revision: catalog.revision(),
        requirement: work.id,
        expected_work_revision: work.revision,
        target: crate::hub::stock::tests::id(8),
        observation: SourceProvenance::new("Replan all pending crates to B3").unwrap(),
    };
    let failure = db
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                work_write::apply_replan_on(transaction, owner(), operation(2), replan, now())
                    .await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(
        db.hub_group(owner(), source.group.id).await.unwrap(),
        source
    );
    assert!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .total,
        1
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn discrepancy_resolution_changes_no_physical_or_work_state_and_retains_original_results() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let before = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    let open = DiscrepancyCommand {
        expected_stock_revision: before.revision,
        expected_catalog_revision: catalog.revision(),
        action: DiscrepancyAction::Open {
            trace: before.group.trace,
            group: Some(before.group.id),
            observation: SourceProvenance::new(
                "Manually reported discrepancy; review original observation",
            )
            .unwrap(),
        },
    };
    let opened = db
        .apply_hub_discrepancy(owner(), operation(2), open.clone(), now())
        .await
        .unwrap();
    let HubHandlingResult::DiscrepancyChanged(issue) = &opened.result else {
        panic!("discrepancy")
    };
    let page = db
        .hub_discrepancy_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].value.id, issue.id);
    assert_eq!(page.rows[0].created_by, owner());
    let resolve = DiscrepancyCommand {
        expected_stock_revision: page.revision,
        expected_catalog_revision: catalog.revision(),
        action: DiscrepancyAction::Resolve {
            discrepancy: issue.id,
            expected_revision: issue.revision,
            resolution: SourceProvenance::new("Observation reviewed; no physical action asserted")
                .unwrap(),
        },
    };
    let resolved = db
        .apply_hub_discrepancy(owner(), operation(3), resolve.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        db.hub_discrepancy_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .total,
        0
    );
    let all = db
        .hub_discrepancy_page(
            owner(),
            HubHandlingQuery {
                open_only: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(all.total, 1);
    assert_eq!(all.rows[0].resolved_operation, Some(operation(3)));
    assert!(all.rows[0].value.resolution.is_some());
    assert_eq!(
        db.hub_group(owner(), before.group.id).await.unwrap().group,
        before.group
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows,
        work.rows
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(opened.clone())
    );
    assert_eq!(
        db.apply_hub_discrepancy(owner(), operation(2), open, now())
            .await
            .unwrap(),
        opened.clone()
    );
    assert_eq!(
        db.apply_hub_discrepancy(owner(), operation(3), resolve, now())
            .await
            .unwrap(),
        resolved.clone()
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(resolved.clone())
    );
    assert_eq!(
        db.hub_discrepancy_page(other(), HubHandlingQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(resolved)
    );
    assert_eq!(
        db.hub_group(owner(), before.group.id).await.unwrap().group,
        before.group
    );
    db.close().await.unwrap();
    // A host fault may retain valid SQL shape while changing semantic text.
    // Restore the exact schema inventory so opening reaches the product decoder.
    let mut raw = fixture.raw().await;
    sqlx::query("DROP TRIGGER hub_discrepancy_identity_immutable")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE hub_discrepancies SET resolution='A different resolution' WHERE discrepancy_id=?",
    )
    .bind(issue.id.as_bytes().as_slice())
    .execute(&mut raw)
    .await
    .unwrap();
    let trigger = schema::MIGRATION_SQL
        .split(";\n")
        .find(|statement| {
            statement.starts_with("CREATE TRIGGER hub_discrepancy_identity_immutable ")
        })
        .unwrap();
    sqlx::raw_sql(trigger).execute(&mut raw).await.unwrap();
    sqlx::Connection::close(raw).await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_discrepancy_page(
            owner(),
            HubHandlingQuery {
                open_only: false,
                ..Default::default()
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    // Later damaged resolution does not rewrite the immutable opening receipt.
    let original = db
        .hub_handling_operation(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    let HubHandlingResult::DiscrepancyChanged(original) = original.result else {
        panic!("original discrepancy");
    };
    assert!(original.resolution.is_none());
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_discrepancy_resolution_preserves_the_open_issue_and_stock() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let before = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let open = DiscrepancyCommand {
        expected_stock_revision: before.revision,
        expected_catalog_revision: catalog.revision(),
        action: DiscrepancyAction::Open {
            trace: before.group.trace,
            group: None,
            observation: SourceProvenance::new("Manually reported discrepancy").unwrap(),
        },
    };
    let opened = db
        .apply_hub_discrepancy(owner(), operation(2), open, now())
        .await
        .unwrap();
    let HubHandlingResult::DiscrepancyChanged(issue) = opened.result else {
        panic!("discrepancy")
    };
    let pending = db
        .hub_discrepancy_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    let resolve = DiscrepancyCommand {
        expected_stock_revision: pending.revision,
        expected_catalog_revision: catalog.revision(),
        action: DiscrepancyAction::Resolve {
            discrepancy: issue.id,
            expected_revision: issue.revision,
            resolution: SourceProvenance::new("Reviewed observation").unwrap(),
        },
    };
    let failure = db
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                discrepancy::apply_on(transaction, owner(), operation(3), resolve, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(
        db.hub_discrepancy_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap(),
        pending
    );
    assert!(
        db.hub_handling_operation(owner(), operation(3))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.hub_group(owner(), before.group.id).await.unwrap().group,
        before.group
    );
    db.close().await.unwrap();
}
