use super::*;
use harvestcircle_domain::hub::actuals::ActualPhysicalActivityKind as Activity;
use harvestcircle_domain::hub::movement::MovementCommand;

fn source_query(
    trace: HubRecordId,
    operation: DurableRequestId,
    group: HubRecordId,
) -> HubActualSourceQuery {
    HubActualSourceQuery {
        context: query(trace),
        operation,
        group,
    }
}

async fn move_whole(db: &Database, group: HubRecordId, to: HubRecordId, n: u32, time: i64) {
    let view = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(n),
        MovementCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group,
            expected_group_revision: view.group.revision,
            to,
            packs: view.group.packs,
            separated_portion: None,
            occurred: Occurrence::Known(at(time)),
            observation: note("Whole observed group moved, preserving original physical source"),
        },
        at(time),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn explicit_original_and_later_arrival_keep_distinct_positions_and_complete_activity() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let group = intake.original.position.group;
    let trace = intake.original.trace.id;
    move_whole(&db, group, id(7), 2, 110).await;
    move_whole(&db, group, id(6), 3, 120).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let birth = db
        .hub_actual_source(owner(), source_query(trace, operation(1), group))
        .await
        .unwrap();
    assert_eq!(birth.source.location.id, id(6));
    assert_eq!(birth.source.group.packs, packs(20));
    assert_eq!(birth.source.occurred, Occurrence::Known(now()));
    assert_eq!(birth.subsequent_activity.len(), 2);
    assert!(
        birth
            .subsequent_activity
            .iter()
            .all(|a| a.kind == Activity::LocationChanged)
    );
    assert!(birth.known_departures.is_empty());
    let arrival = db
        .hub_actual_source(owner(), source_query(trace, operation(2), group))
        .await
        .unwrap();
    assert_eq!(arrival.source.location.id, id(7));
    assert_eq!(arrival.source.group.packs, packs(20));
    assert_eq!(arrival.source.group.revision.value(), 2);
    assert_eq!(arrival.source.occurred, Occurrence::Known(at(110)));
    assert_eq!(arrival.subsequent_activity.len(), 1);
    assert_eq!(
        arrival.subsequent_activity[0].occurred,
        Occurrence::Known(at(120))
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_source(owner(), source_query(trace, operation(2), group))
            .await
            .unwrap(),
        arrival
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn depleted_child_still_has_original_six_and_complete_original_removal() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let trace = intake.original.trace.id;
    let child = separated(&db, intake.original.position.group).await;
    let view = db.hub_group(owner(), child).await.unwrap();
    db.apply_hub_disposition(
        owner(),
        operation(3),
        DispositionCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group: child,
            expected_group_revision: view.group.revision,
            packs: packs(6),
            kind: DispositionKind::Destroyed,
            occurred: Occurrence::Known(at(120)),
            observation: note("All original six removed; retain historical physical snapshot"),
        },
        at(120),
    )
    .await
    .unwrap();
    let source = db
        .hub_actual_source(owner(), source_query(trace, operation(2), child))
        .await
        .unwrap();
    assert_eq!(source.source.group.packs, packs(6));
    assert_eq!(source.lineage.lineage.node(child).unwrap().packs, packs(0));
    assert_eq!(source.subsequent_activity.len(), 1);
    assert_eq!(
        source.subsequent_activity[0].kind,
        Activity::OtherPhysicalEffect
    );
    let root = db
        .hub_actual_source(
            owner(),
            source_query(trace, operation(1), intake.original.position.group),
        )
        .await
        .unwrap();
    assert_eq!(root.source.group.packs, packs(20));
    assert_eq!(root.subsequent_activity.len(), 2);
    assert_eq!(root.subsequent_activity[0].kind, Activity::GroupBorn);
    db.close().await.unwrap();
}

#[tokio::test]
async fn all_known_departure_lines_survive_depletion_and_unknown_physical_time() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let source = db
        .hub_actual_source(
            owner(),
            source_query(
                intake.original.trace.id,
                intake.operation.clone(),
                intake.original.position.group,
            ),
        )
        .await
        .unwrap();
    assert_eq!(source.source.group.packs, packs(20));
    assert_eq!(source.source.occurred, Occurrence::Unknown);
    assert_eq!(source.known_departures.len(), 1);
    let known = &source.known_departures[0];
    assert_eq!(known.operation, departure.record.operation);
    assert_eq!(known.packs().unwrap(), packs(6));
    assert_eq!(known.recipient.id, id(8));
    assert_eq!(known.occurred, Occurrence::Unknown);
    assert_eq!(known.lines[0].group, departure.record.consumed[0].before.id);
    assert_eq!(
        source.subsequent_activity.last().unwrap().kind,
        Activity::OtherPhysicalEffect
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_source(
            owner(),
            source_query(
                intake.original.trace.id,
                intake.operation,
                intake.original.position.group
            )
        )
        .await
        .unwrap(),
        source
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn source_reads_reject_foreign_stale_or_unrelated_originals_without_writes() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let trace = intake.original.trace.id;
    let group = intake.original.position.group;
    let exact = source_query(trace, operation(1), group);
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_source(other(), exact.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = exact.clone();
    stale.context.expected_revision = Some(HubRevision::INITIAL);
    assert_eq!(
        db.hub_actual_source(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let mut stale = exact.clone();
    stale.context.expected_catalog_revision = Some(HubRevision::INITIAL);
    assert_eq!(
        db.hub_actual_source(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_actual_source(owner(), source_query(trace, operation(999), group))
            .await
            .is_err()
    );
    assert!(
        db.hub_actual_source(owner(), source_query(trace, operation(1), id(99)))
            .await
            .is_err()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn corrupted_physical_original_or_occurrence_never_yields_clear_activity() {
    for (trigger, mutation) in [
        (
            "hub_movement_details_no_update",
            "UPDATE hub_movement_details SET source_before_packs=source_before_packs+1",
        ),
        (
            "hub_stock_events_no_update",
            "UPDATE hub_stock_events SET occurrence_start_unix_s=109,occurrence_end_unix_s=109 WHERE kind='move'",
        ),
    ] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let intake = received(&db).await;
        let group = intake.original.position.group;
        move_whole(&db, group, id(7), 2, 110).await;
        let mut raw = fixture.raw().await;
        assert!(sqlx::query(mutation).execute(&mut raw).await.is_err());
        let ddl: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?")
                .bind(trigger)
                .fetch_one(&mut raw)
                .await
                .unwrap();
        assert!(
            trigger
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TRIGGER {trigger}")))
            .execute(&mut raw)
            .await
            .unwrap();
        sqlx::query(mutation).execute(&mut raw).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(ddl.clone()))
            .execute(&mut raw)
            .await
            .unwrap();
        let restored: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?")
                .bind(trigger)
                .fetch_one(&mut raw)
                .await
                .unwrap();
        assert_eq!(restored, ddl);
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_actual_source(
                owner(),
                source_query(intake.original.trace.id, operation(1), group)
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn frozen_native_cut_ignores_later_arrivals_births_and_depletion_without_current_balances() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let trace = intake.original.trace.id;
    let root = intake.original.position.group;
    let child = separated(&db, root).await;
    let saved = db
        .hub_actual_source(owner(), source_query(trace, operation(1), root))
        .await
        .unwrap();
    move_whole(&db, child, id(7), 3, 120).await;
    let view = db.hub_group(owner(), root).await.unwrap();
    db.apply_hub_split(
        owner(),
        operation(4),
        SplitCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group: root,
            expected_group_revision: view.group.revision,
            packs: packs(4),
            identification: PhysicalIdentification {
                label: HubLabel::new("Later four outside old cut").unwrap(),
                observation: note("Physically separate later cohort"),
            },
            occurred: Occurrence::Known(at(130)),
            observation: note("Later native birth must not enter original review context"),
        },
        at(130),
    )
    .await
    .unwrap();
    let view = db.hub_group(owner(), child).await.unwrap();
    db.apply_hub_disposition(
        owner(),
        operation(5),
        DispositionCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group: child,
            expected_group_revision: view.group.revision,
            packs: packs(6),
            kind: DispositionKind::Destroyed,
            occurred: Occurrence::Known(at(140)),
            observation: note("All original six removed after the frozen native cut"),
        },
        at(140),
    )
    .await
    .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let retained = saved.clone();
    let (births, source, activity) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = crate::hub::reference::authorize_on(tx, owner()).await?;
                let graph = &retained.lineage.lineage;
                let births =
                    crate::hub::handling::lineage::historical_births_on(tx, hub.id(), graph)
                        .await?;
                let source = crate::hub::handling::actuals::source_on(
                    tx,
                    hub.id(),
                    graph,
                    &retained.lineage.population,
                    &operation(1),
                    root,
                )
                .await?;
                let activity =
                    crate::hub::handling::actuals::activity_on(tx, hub.id(), graph, &source)
                        .await?;
                let floor = crate::hub::handling::read::causal_floor_through_on(
                    tx,
                    child,
                    graph.revision(),
                )
                .await?;
                assert_eq!(floor, Some(at(110)));
                Ok::<_, SafeError>((births, source, activity))
            })
        })
        .await
        .unwrap();
    assert_eq!(births, saved.lineage.births);
    assert_eq!(source, saved.source);
    assert_eq!(activity, saved.subsequent_activity);
    assert_eq!(saved.lineage.lineage.node(child).unwrap().packs, packs(6));
    let current = db.hub_actual_lineage(owner(), query(trace)).await.unwrap();
    assert_eq!(current.lineage.node(child).unwrap().packs, packs(0));
    assert_eq!(current.births.len(), 3);
    assert_eq!(births.len(), 2);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn known_departure_discovery_stays_at_the_original_native_cut() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let trace = intake.original.trace.id;
    let revision = departure.record.stock_revision;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let (earlier, current) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = crate::hub::reference::authorize_on(tx, owner()).await?;
                let earlier = crate::hub::handling::actuals::known_departures_on(
                    tx,
                    hub.id(),
                    trace,
                    HubRevision::from_value(revision.value() - 1).unwrap(),
                )
                .await?;
                let current = crate::hub::handling::actuals::known_departures_on(
                    tx,
                    hub.id(),
                    trace,
                    revision,
                )
                .await?;
                Ok::<_, SafeError>((earlier, current))
            })
        })
        .await
        .unwrap();
    assert!(earlier.is_empty());
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].packs().unwrap(), packs(6));
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn frozen_treatment_cut_keeps_original_hold_without_later_release_or_new_hold() {
    use harvestcircle_domain::hub::treatment::{
        TreatmentAction, TreatmentCommand, TreatmentSelection,
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let trace = intake.original.trace.id;
    let group = intake.original.position.group;
    let view = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(2),
        TreatmentCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred: Occurrence::Known(at(110)),
            observation: note("Original physical precautionary restriction"),
        },
        at(110),
    )
    .await
    .unwrap();
    let saved = db.hub_actual_lineage(owner(), query(trace)).await.unwrap();
    let graph = saved.lineage.clone();
    let original = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = crate::hub::reference::authorize_on(tx, owner()).await?;
                crate::hub::handling::treatment::read::historical_state_on(tx, hub.id(), &graph)
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(original.holds().len(), 1);
    let held = &original.holds()[0];
    let view = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(3),
        TreatmentCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            action: TreatmentAction::ReleaseHold {
                hold: held.stamp.id,
                expected_revision: held.revision(),
            },
            occurred: Occurrence::Known(at(120)),
            observation: note("Later separately authorized release"),
        },
        at(120),
    )
    .await
    .unwrap();
    let view = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(4),
        TreatmentCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred: Occurrence::Known(at(130)),
            observation: note("Different later physical precautionary restriction"),
        },
        at(130),
    )
    .await
    .unwrap();
    let current = db.hub_actual_lineage(owner(), query(trace)).await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let (past, current) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = crate::hub::reference::authorize_on(tx, owner()).await?;
                let past = crate::hub::handling::treatment::read::historical_state_on(
                    tx,
                    hub.id(),
                    &saved.lineage,
                )
                .await?;
                let current = crate::hub::handling::treatment::read::historical_state_on(
                    tx,
                    hub.id(),
                    &current.lineage,
                )
                .await?;
                Ok::<_, SafeError>((past, current))
            })
        })
        .await
        .unwrap();
    assert_eq!(past, original);
    assert!(past.holds()[0].release.is_none());
    assert_eq!(current.holds().len(), 2);
    assert!(
        current
            .holds()
            .iter()
            .any(|h| h.stamp.id == past.holds()[0].stamp.id && h.release.is_some())
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn frozen_catalog_cut_retains_old_location_while_current_reference_advances() {
    use harvestcircle_domain::hub::reference::{
        ReferenceCommand, ReferenceMutation, ReferenceValue,
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let original = db.hub_references(owner()).await.unwrap();
    let location = original.get(id(6)).unwrap();
    let mut value = location.value.clone();
    let ReferenceValue::Location { label, .. } = &mut value else {
        panic!("location")
    };
    *label = HubLabel::new("Receiving renamed after original cut").unwrap();
    db.apply_hub_reference(
        owner(),
        operation(2000),
        ReferenceCommand {
            expected_catalog_revision: original.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(6),
                expected_record_revision: location.revision,
                value,
            },
        },
        at(120),
    )
    .await
    .unwrap();
    let current = db.hub_references(owner()).await.unwrap();
    assert_ne!(current.get(id(6)), original.get(id(6)));
    let before = crate::hub::custody::tests::material(&fixture).await;
    let cut = original.revision();
    let past = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = crate::hub::reference::authorize_on(tx, owner()).await?;
                crate::hub::reference::load_catalog_through_on(tx, hub.id(), cut).await
            })
        })
        .await
        .unwrap();
    assert_eq!(past, original);
    let evidence = db
        .hub_actual_source(
            owner(),
            source_query(
                intake.original.trace.id,
                intake.operation,
                intake.original.position.group,
            ),
        )
        .await
        .unwrap();
    assert_eq!(evidence.source.location, *original.get(id(6)).unwrap());
    assert_eq!(evidence.lineage.catalog_revision, current.revision());
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn balanced_sibling_shrink_and_growth_without_a_canonical_outflow_is_not_native_birth_evidence()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let root = intake.original.position.group;
    let child = separated(&db, root).await;
    let mut raw = fixture.raw().await;
    assert!(
        sqlx::query("UPDATE hub_group_work SET scoped_packs=scoped_packs+2 WHERE group_id=?")
            .bind(root.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let ddl:String=sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='hub_group_work_identity_immutable'")
        .fetch_one(&mut raw).await.unwrap();
    sqlx::query("DROP TRIGGER hub_group_work_identity_immutable")
        .execute(&mut raw)
        .await
        .unwrap();
    for (group, delta) in [(root, 2_i64), (child, -2_i64)] {
        sqlx::query("UPDATE hub_stock_positions SET packs=packs+? WHERE group_id=?")
            .bind(delta)
            .bind(group.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .unwrap();
        sqlx::query("UPDATE hub_group_work SET scoped_packs=scoped_packs+? WHERE group_id=?")
            .bind(delta)
            .bind(group.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .unwrap();
    }
    sqlx::query(sqlx::AssertSqlSafe(ddl.clone()))
        .execute(&mut raw)
        .await
        .unwrap();
    let restored:String=sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='hub_group_work_identity_immutable'")
        .fetch_one(&mut raw).await.unwrap();
    assert_eq!(restored, ddl);
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_lineage(owner(), query(intake.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
