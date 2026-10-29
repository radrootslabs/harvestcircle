use super::*;

#[tokio::test]
async fn another_nonreserving_issue_cannot_reuse_departed_stock_but_can_close_unused() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let group = intake.original.position.group;
    accept(&db, &[group], 2).await;
    let a = issue_to(&db, &[group], id(8), 20).await;
    let b = issue_to(&db, &[group], id(9), 40).await;
    let original = db
        .apply_hub_departure(owner(), operation(30), departing(&db, &a).await, now())
        .await
        .unwrap();
    let before = counts(&db).await;
    let blocked = db
        .hub_departure_review(owner(), request(&b), now())
        .await
        .unwrap();
    assert_eq!(blocked.block.unwrap().code(), SafeErrorCode::HubConflict);
    assert_eq!(blocked.view.state, crate::HubDispatchState::Issued);
    assert!(blocked.view.departure.is_none());
    assert_eq!(
        db.apply_hub_departure(owner(), operation(60), departing(&db, &b).await, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(before, counts(&db).await);
    let view = db.hub_dispatch_record(owner(), b.record.id).await.unwrap();
    db.apply_hub_dispatch(
        owner(),
        operation(61),
        DispatchCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            action: DispatchAction::MarkUnused {
                preparation: b.record.id,
                expected_revision: b.record.revision,
            },
            observation: note("Close the other unused issue after the stock departed under A"),
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_dispatch_record(owner(), b.record.id)
            .await
            .unwrap()
            .state,
        crate::HubDispatchState::Unused
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), b.operation.clone())
            .await
            .unwrap(),
        Some(b)
    );
    db.close().await.unwrap();
}

fn request(paper: &HubDispatchReceipt) -> HubDepartureReviewInput {
    HubDepartureReviewInput {
        preparation: paper.record.id,
        expected_preparation_revision: paper.record.revision,
        identification: paper.record.versions.last().unwrap().id,
    }
}
async fn counts(db: &Database) -> (i64, i64, i64, i64, i64) {
    db.host().transaction(|tx|Box::pin(async move {
        sqlx::query_as("SELECT (SELECT count(*) FROM hub_stock_operations),(SELECT count(*) FROM hub_stock_events),(SELECT count(*) FROM hub_departures),(SELECT count(*) FROM hub_departure_work_scope),(SELECT count(*) FROM durable_operations)")
            .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())
    })).await.unwrap()
}
#[tokio::test]
async fn current_review_proposes_work_without_writing_or_reserving_and_later_hold_blocks() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let five = split(&db, intake.original.position.group, 5, 2, "Reviewed five").await;
    accept(&db, &[five], 3).await;
    let paper = issue(&db, &[five]).await;
    let input = request(&paper);
    let before = counts(&db).await;
    let stock = db.hub_stock_page(owner(), query()).await.unwrap();
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    let document = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    let reviewed = db
        .hub_departure_review(owner(), input.clone(), now())
        .await
        .unwrap();
    assert!(reviewed.block.is_none());
    assert_eq!(reviewed.request, input);
    assert_eq!(reviewed.view, document);
    assert_eq!(reviewed.proposed_work_changes.len(), 1);
    let proposed = &reviewed.proposed_work_changes[0];
    assert_eq!(proposed.retired.value(), 5);
    assert_eq!(proposed.after.quantity.no_longer_required().value(), 5);
    assert_eq!(proposed.after.quantity.remaining().value(), 15);
    assert_eq!(
        reviewed,
        db.hub_departure_review(owner(), input.clone(), now())
            .await
            .unwrap()
    );
    assert_eq!(before, counts(&db).await);
    assert_eq!(stock, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        work,
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
    );
    assert_eq!(
        document,
        db.hub_dispatch_record(owner(), paper.record.id)
            .await
            .unwrap()
    );
    let current = db.hub_group(owner(), five).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(22),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![five]).unwrap(),
            },
            occurred: Occurrence::Unknown,
            observation: note("A real later hold invalidates prior readiness"),
        },
        now(),
    )
    .await
    .unwrap();
    let held = counts(&db).await;
    let blocked = db
        .hub_departure_review(owner(), input, now())
        .await
        .unwrap();
    assert_eq!(blocked.block.unwrap().code(), SafeErrorCode::HubConflict);
    assert!(blocked.proposed_work_changes.is_empty());
    assert_eq!(held, counts(&db).await);
    let stale = DepartureCommand {
        expected_stock_revision: reviewed.view.revision,
        expected_catalog_revision: reviewed.view.catalog_revision,
        preparation: paper.record.id,
        expected_preparation_revision: paper.record.revision,
        identification: request(&paper).identification,
        occurred: Occurrence::Unknown,
        observation: note("Prior readiness is never a future departure grant"),
    };
    assert_eq!(
        db.apply_hub_departure(owner(), operation(30), stale, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(held, counts(&db).await);
    db.close().await.unwrap();
}
#[tokio::test]
async fn changed_material_and_wrong_preparation_revision_remain_native_blocked_context() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let group = intake.original.position.group;
    accept(&db, &[group], 2).await;
    let paper = issue(&db, &[group]).await;
    let mut input = request(&paper);
    input.expected_preparation_revision =
        HubRevision::from_value(paper.record.revision.value() + 1).unwrap();
    let before = counts(&db).await;
    let blocked = db
        .hub_departure_review(owner(), input, now())
        .await
        .unwrap();
    assert_eq!(blocked.block.unwrap().code(), SafeErrorCode::HubConflict);
    assert_eq!(blocked.view.record, paper.record);
    assert_eq!(before, counts(&db).await);
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(22),
        MovementCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group,
            expected_group_revision: current.group.revision,
            to: id(7),
            packs: current.group.packs,
            separated_portion: None,
            occurred: Occurrence::Unknown,
            observation: note("Changed physical location after issue requires replacement"),
        },
        now(),
    )
    .await
    .unwrap();
    let before = counts(&db).await;
    let blocked = db
        .hub_departure_review(owner(), request(&paper), now())
        .await
        .unwrap();
    assert_eq!(blocked.block.unwrap().code(), SafeErrorCode::HubConflict);
    assert_eq!(blocked.view.record, paper.record);
    assert!(blocked.proposed_work_changes.is_empty());
    assert_eq!(before, counts(&db).await);
    db.close().await.unwrap();
}
#[tokio::test]
async fn used_identification_review_exposes_original_usage_without_another_effect_or_foreign_access()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let group = intake.original.position.group;
    accept(&db, &[group], 2).await;
    let paper = issue(&db, &[group]).await;
    let original = db
        .apply_hub_departure(owner(), operation(30), departing(&db, &paper).await, now())
        .await
        .unwrap();
    let before = counts(&db).await;
    let review = db
        .hub_departure_review(owner(), request(&paper), now())
        .await
        .unwrap();
    assert_eq!(review.block.unwrap().code(), SafeErrorCode::HubConflict);
    assert_eq!(review.view.state, crate::HubDispatchState::Departed);
    assert_eq!(review.view.departure.unwrap().id, original.record.id);
    assert_eq!(review.view.record, paper.record);
    assert!(review.proposed_work_changes.is_empty());
    assert_eq!(
        db.hub_departure_review(crate::hub::stock::tests::other(), request(&paper), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(before, counts(&db).await);
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(original)
    );
    db.close().await.unwrap();
}
