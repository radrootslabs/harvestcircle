use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, now, operation, other, owner, setup,
};
use harvestcircle_domain::hub::portions::SplitCommand;
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::treatment::{
    AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection,
};

fn scope(trace: HubRecordId) -> HubTreatmentScope {
    HubTreatmentScope {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
    }
}
fn selection(groups: &[HubRecordId]) -> TreatmentSelection {
    TreatmentSelection::new(groups.to_vec()).unwrap()
}
async fn treatment(
    db: &crate::Database,
    trace: HubRecordId,
    action: TreatmentAction,
) -> TreatmentCommand {
    let snapshot = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    TreatmentCommand {
        expected_stock_revision: snapshot.revision,
        expected_catalog_revision: snapshot.catalog_revision,
        action,
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Explicit manually reviewed sample decision").unwrap(),
    }
}
async fn separate(
    db: &crate::Database,
    group: HubRecordId,
    packs: u32,
    label: &str,
    op: u32,
) -> HubRecordId {
    let current = db.hub_group(owner(), group).await.unwrap();
    let receipt = db
        .apply_hub_split(
            owner(),
            operation(op),
            SplitCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group,
                expected_group_revision: current.group.revision,
                packs: WholePacks::from_value(packs).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new(label).unwrap(),
                    observation: SourceProvenance::new(
                        "Observed tagged original crates in a separate container",
                    )
                    .unwrap(),
                },
                occurred: Occurrence::Unknown,
                observation: SourceProvenance::new(
                    "Observed separately identified portion at Receiving",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    let HubHandlingResult::Separated(value) = receipt.result else {
        panic!("separation")
    };
    value.portion_group
}
async fn relocate(db: &crate::Database, group: HubRecordId, to: HubRecordId, nonce: u32) {
    let selected = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(nonce),
        harvestcircle_domain::hub::movement::MovementCommand {
            expected_stock_revision: selected.revision,
            expected_catalog_revision: selected.catalog_revision,
            group,
            expected_group_revision: selected.group.revision,
            to,
            packs: selected.group.packs,
            separated_portion: None,
            occurred: Occurrence::Unknown,
            observation: SourceProvenance::new("Manually confirmed actual internal relocation")
                .unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
}

fn result(receipt: &HubHandlingReceipt) -> &HubTreatmentResult {
    let HubHandlingResult::Treatment(result) = &receipt.result else {
        panic!("treatment receipt")
    };
    result
}

#[tokio::test]
async fn native_fx05_hold_union_selected_release_originals_and_restart() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let root = intake.original.position.group;
    let initial = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(initial.projection.summary.review_required.value(), 20);
    assert_eq!(initial.projection.summary.departure_eligible.value(), 0);
    let accept = treatment(
        &db,
        trace,
        TreatmentAction::SetAcceptance {
            selection: selection(&[root]),
            status: AcceptanceStatus::Accepted,
        },
    )
    .await;
    let accepted = db
        .apply_hub_treatment(owner(), operation(2), accept.clone(), now())
        .await
        .unwrap();
    relocate(
        &db,
        root,
        intake.original.trace.planned_location.as_ref().unwrap().id,
        70,
    )
    .await;
    let eight = separate(&db, root, 8, "Blue eight", 3).await;
    let five = separate(&db, eight, 5, "Green five", 4).await;
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows;
    let a = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[five]),
        },
    )
    .await;
    let placed_a = db
        .apply_hub_treatment(owner(), operation(5), a.clone(), now())
        .await
        .unwrap();
    let TreatmentChange::HoldPlaced(held_a) = &result(&placed_a).change else {
        panic!("hold A")
    };
    let hold_a = held_a.stamp.id;
    let b = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[five, eight]),
        },
    )
    .await;
    let placed_b = db
        .apply_hub_treatment(owner(), operation(6), b, now())
        .await
        .unwrap();
    let TreatmentChange::HoldPlaced(held_b) = &result(&placed_b).change else {
        panic!("hold B")
    };
    let hold_b = held_b.stamp.id;
    let both = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(
        (
            both.projection.summary.on_hand.value(),
            both.projection.summary.restricted.value(),
            both.projection.summary.departure_eligible.value()
        ),
        (20, 8, 12)
    );
    assert_eq!(
        both.projection
            .groups
            .iter()
            .find(|group| group.group == five)
            .unwrap()
            .active_holds
            .len(),
        2
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows,
        work
    );
    let release = treatment(
        &db,
        trace,
        TreatmentAction::ReleaseHold {
            hold: hold_a,
            expected_revision: HubRevision::INITIAL,
        },
    )
    .await;
    let released_a = db
        .apply_hub_treatment(owner(), operation(7), release.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        (
            result(&released_a).summary.restricted.value(),
            result(&released_a).summary.departure_eligible.value()
        ),
        (8, 12)
    );
    let release = treatment(
        &db,
        trace,
        TreatmentAction::ReleaseHold {
            hold: hold_b,
            expected_revision: HubRevision::INITIAL,
        },
    )
    .await;
    let released_b = db
        .apply_hub_treatment(owner(), operation(8), release, now())
        .await
        .unwrap();
    assert_eq!(
        (
            result(&released_b).summary.restricted.value(),
            result(&released_b).summary.departure_eligible.value()
        ),
        (0, 20)
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows,
        work
    );
    assert_eq!(
        db.apply_hub_treatment(owner(), operation(2), accept, now())
            .await
            .unwrap(),
        accepted
    );
    assert_eq!(
        db.apply_hub_treatment(owner(), operation(5), a, now())
            .await
            .unwrap(),
        placed_a
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(7))
            .await
            .unwrap(),
        Some(released_a)
    );
    let final_state = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert!(
        final_state
            .holds
            .iter()
            .all(|held| held.hold.revision().value() == 2)
    );
    let history = db
        .hub_treatment_history(
            owner(),
            HubTreatmentQuery {
                trace,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(history.total, 5);
    assert_eq!(
        db.hub_treatment(other(), scope(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let attempt = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[root]),
        },
    )
    .await;
    assert_eq!(
        db.apply_hub_treatment(other(), operation(9), attempt, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_treatment(owner(), scope(trace)).await.unwrap(),
        final_state
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(5))
            .await
            .unwrap(),
        Some(placed_a)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(intake)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn original_treatment_recovery_ignores_later_positions_and_counters() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let request = treatment(
        &db,
        trace,
        TreatmentAction::SetAcceptance {
            selection: selection(&[intake.original.position.group]),
            status: AcceptanceStatus::Accepted,
        },
    )
    .await;
    let recorded = db
        .apply_hub_treatment(owner(), operation(2), request.clone(), now())
        .await
        .unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=19")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_stock_state SET event_count=0")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::Connection::close(raw).await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_treatment(owner(), scope(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    assert_eq!(
        db.apply_hub_treatment(owner(), operation(2), request, now())
            .await
            .unwrap(),
        recorded
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn treatment_rollback_and_competing_stale_commands_leave_one_atomic_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let request = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[intake.original.position.group]),
        },
    )
    .await;
    let before = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    let failed = request.clone();
    assert!(
        db.host()
            .transaction(|tx| Box::pin(async move {
                write::apply_on(tx, owner(), operation(2), failed, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            }))
            .await
            .is_err()
    );
    assert_eq!(
        db.hub_treatment(owner(), scope(trace)).await.unwrap(),
        before
    );
    assert!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap()
            .is_none()
    );
    let (a, b) = tokio::join!(
        db.apply_hub_treatment(owner(), operation(2), request.clone(), now()),
        db.apply_hub_treatment(owner(), operation(3), request, now())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let error = if let Err(error) = a {
        error
    } else {
        b.unwrap_err()
    };
    assert_eq!(error.code(), SafeErrorCode::HubConflict);
    let after = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(after.holds.len(), 1);
    assert_eq!(after.projection.summary.restricted.value(), 20);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_treatment_pages_pin_versions_and_detect_semantically_changed_originals() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let before = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    let request = treatment(
        &db,
        trace,
        TreatmentAction::SetAcceptance {
            selection: selection(&[intake.original.position.group]),
            status: AcceptanceStatus::Accepted,
        },
    )
    .await;
    db.apply_hub_treatment(owner(), operation(2), request, now())
        .await
        .unwrap();
    assert_eq!(
        db.hub_treatment(
            owner(),
            HubTreatmentScope {
                expected_revision: Some(before.revision),
                expected_catalog_revision: Some(before.catalog_revision),
                ..scope(trace)
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    let query = HubTreatmentQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 1,
    };
    assert_eq!(
        db.hub_treatment_history(owner(), query.clone())
            .await
            .unwrap()
            .total,
        1
    );
    for invalid in [
        HubTreatmentQuery {
            limit: 0,
            ..query.clone()
        },
        HubTreatmentQuery {
            limit: 65,
            ..query.clone()
        },
        HubTreatmentQuery {
            offset: 1,
            ..query.clone()
        },
        HubTreatmentQuery {
            offset: 513,
            ..query
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let guard = schema::MIGRATION_SQL
        .split(";\n")
        .find(|sql| sql.starts_with("CREATE TRIGGER hub_treatment_records_no_update "))
        .unwrap();
    sqlx::query("DROP TRIGGER hub_treatment_records_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    // Preserve valid SQL shape and accounting but corrupt the recorded result.
    sqlx::query("UPDATE hub_treatment_records SET accepted=0,review_required=20,departure_eligible=0 WHERE kind='acceptance'").execute(&mut raw).await.unwrap();
    sqlx::query(guard).execute(&mut raw).await.unwrap();
    sqlx::Connection::close(raw).await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_treatment(owner(), scope(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(intake)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_primary_limit_preserves_release_and_blocks_new_precautionary_gap() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let root = intake.original.position.group;
    let mut revision = HubRevision::from_value(2).unwrap();
    for n in 0..255 {
        let request = TreatmentCommand {
            expected_stock_revision: revision,
            expected_catalog_revision: catalog.revision(),
            action: TreatmentAction::SetAcceptance {
                selection: selection(&[root]),
                status: AcceptanceStatus::Accepted,
            },
            occurred: Occurrence::Unknown,
            observation: SourceProvenance::new(format!("Explicit sample review number {n}"))
                .unwrap(),
        };
        revision = db
            .apply_hub_treatment(owner(), operation(1000 + n), request, now())
            .await
            .unwrap()
            .revision;
    }
    let request = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[root]),
        },
    )
    .await;
    let placed = db
        .apply_hub_treatment(owner(), operation(2000), request, now())
        .await
        .unwrap();
    let TreatmentChange::HoldPlaced(held) = &result(&placed).change else {
        panic!("hold")
    };
    let request = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[root]),
        },
    )
    .await;
    assert_eq!(
        db.apply_hub_treatment(owner(), operation(2001), request, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    assert!(
        db.hub_handling_operation(owner(), operation(2001))
            .await
            .unwrap()
            .is_none()
    );
    let release = treatment(
        &db,
        trace,
        TreatmentAction::ReleaseHold {
            hold: held.stamp.id,
            expected_revision: HubRevision::INITIAL,
        },
    )
    .await;
    db.apply_hub_treatment(owner(), operation(2002), release, now())
        .await
        .unwrap();
    let current = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert!(current.projection.history_capacity_reached);
    assert_eq!(
        (
            current.projection.summary.restricted.value(),
            current.projection.summary.departure_eligible.value()
        ),
        (0, 20)
    );
    assert!(
        current
            .projection
            .require(
                &selection(&[root]),
                harvestcircle_domain::hub::treatment::PhysicalAction::NormalDeparture
            )
            .is_err()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_hold_inherits_observed_separation_and_allows_relocation_without_acceptance() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let trace = intake.original.trace.id;
    let request = treatment(
        &db,
        trace,
        TreatmentAction::PlaceHold {
            selection: selection(&[intake.original.position.group]),
        },
    )
    .await;
    let placed = db
        .apply_hub_treatment(owner(), operation(2), request, now())
        .await
        .unwrap();
    let TreatmentChange::HoldPlaced(held) = &result(&placed).change else {
        panic!("hold")
    };
    let eight = separate(&db, intake.original.position.group, 8, "Held blue eight", 3).await;
    let split = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(
        split
            .projection
            .groups
            .iter()
            .find(|row| row.group == eight)
            .unwrap()
            .active_holds,
        vec![held.stamp.id]
    );
    assert_eq!(split.projection.summary.restricted.value(), 20);
    relocate(
        &db,
        eight,
        intake.original.trace.planned_location.as_ref().unwrap().id,
        4,
    )
    .await;
    let moved = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(
        (
            moved.projection.summary.restricted.value(),
            moved.projection.summary.review_required.value(),
            moved.projection.summary.departure_eligible.value()
        ),
        (20, 20, 0)
    );
    let release = treatment(
        &db,
        trace,
        TreatmentAction::ReleaseHold {
            hold: held.stamp.id,
            expected_revision: HubRevision::INITIAL,
        },
    )
    .await;
    db.apply_hub_treatment(owner(), operation(5), release, now())
        .await
        .unwrap();
    let released = db.hub_treatment(owner(), scope(trace)).await.unwrap();
    assert_eq!(
        (
            released.projection.summary.restricted.value(),
            released.projection.summary.review_required.value(),
            released.projection.summary.departure_eligible.value()
        ),
        (0, 20, 0)
    );
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
    db.close().await.unwrap();
}
