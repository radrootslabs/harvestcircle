use super::*;
use crate::hub::stock::tests::{Fixture, command, now, operation, other, owner, query, setup};
use harvestcircle_domain::hub::portions::{CapturedPhysicalScope, SplitCommand};
use harvestcircle_domain::hub::reference::SourceProvenance;

/// Populate the actual schema-7 host through its compatible handling writers.
pub(crate) async fn seed_schema7_handling(
    host: &radroots_service_sqlite::ServiceSqliteHost,
    hub: harvestcircle_domain::hub::HubOwnership,
    receipt: &crate::HubStockReceipt,
) -> Vec<HubHandlingReceipt> {
    let trace = receipt.original.trace.id;
    let group = receipt.original.position.group;
    host.transaction(|tx| {
        Box::pin(async move {
            let catalog = super::super::reference::load_catalog_on(tx, hub.id()).await?;
            let source = read::group_on(tx, hub.id(), group, &catalog).await?;
            let state = super::super::stock::read::state_on(tx, hub.id()).await?;
            let movement = harvestcircle_domain::hub::movement::MovementCommand {
                expected_stock_revision: state.revision,
                expected_catalog_revision: catalog.revision(),
                group,
                expected_group_revision: source.revision,
                to: receipt_target(&catalog)?,
                packs: WholePacks::from_value(8).unwrap(),
                separated_portion: Some(PhysicalIdentification {
                    label: HubLabel::new("Historical blue eight").unwrap(),
                    observation: SourceProvenance::new("Eight original tagged crates").unwrap(),
                }),
                occurred: Occurrence::Unknown,
                observation: SourceProvenance::new("Actual historical move to A3").unwrap(),
            };
            let moved =
                write::apply_movement_on(tx, hub.owner(), operation(300), movement, now()).await?;
            let issue = discrepancy::apply_on(
                tx,
                hub.owner(),
                operation(301),
                harvestcircle_domain::hub::movement::DiscrepancyCommand {
                    expected_stock_revision: moved.revision,
                    expected_catalog_revision: catalog.revision(),
                    action: harvestcircle_domain::hub::movement::DiscrepancyAction::Open {
                        trace,
                        group: Some(group),
                        observation: SourceProvenance::new(
                            "Historical label follow-up remains open",
                        )
                        .unwrap(),
                    },
                },
                now(),
            )
            .await?;
            Ok::<_, SafeError>(vec![moved, issue])
        })
    })
    .await
    .unwrap()
}

fn receipt_target(
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
) -> Result<HubRecordId, SafeError> {
    let target = crate::hub::stock::tests::id(14);
    catalog
        .require_kind(
            target,
            harvestcircle_domain::hub::reference::ReferenceKind::Location,
        )
        .map_err(|_| crate::db::corrupt_storage())?;
    Ok(target)
}

fn split(group: &HubGroupView, packs: u32, label: &str) -> SplitCommand {
    SplitCommand {
        expected_stock_revision: group.revision,
        expected_catalog_revision: group.catalog_revision,
        group: group.group.id,
        expected_group_revision: group.group.revision,
        packs: WholePacks::from_value(packs).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new(label).unwrap(),
            observation: SourceProvenance::new("Observed and separately tagged original crates")
                .unwrap(),
        },
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Separated the counted portion at the same location")
            .unwrap(),
    }
}

fn lineage_query(trace: HubRecordId) -> HubLineageQuery {
    HubLineageQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
    }
}

#[tokio::test]
async fn birth_guard_rejects_fabricated_ancestry_without_growing_history() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    for parent in [None, Some(received.original.position.group)] {
        let trace = received.original.trace.id;
        let result = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let fabricated = crate::hub::stock::tests::id(70);
                    sqlx::query("INSERT INTO hub_stock_groups VALUES (?,?,1)")
                        .bind(fabricated.as_bytes().as_slice())
                        .bind(trace.as_bytes().as_slice())
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| storage_unavailable())?;
                    let failure =
                        sqlx::query("INSERT INTO hub_group_identification VALUES (?,?,?,?,?)")
                            .bind(fabricated.as_bytes().as_slice())
                            .bind(parent.as_ref().map(|id| id.as_bytes().as_slice()))
                            .bind(parent.map(|_| "Unobserved extra portion"))
                            .bind(parent.map(|_| "Invalid claimed birth"))
                            .bind(operation(1).as_str())
                            .execute(&mut *tx)
                            .await
                            .unwrap_err();
                    assert!(
                        failure
                            .to_string()
                            .contains("physical group birth must retain its original population")
                    );
                    Err::<(), SafeError>(storage_unavailable())
                })
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            db.hub_lineage(owner(), lineage_query(trace))
                .await
                .unwrap()
                .total,
            1
        );
    }
    db.close().await.unwrap();
}

async fn capture(
    db: &crate::Database,
    trace: HubRecordId,
    group: HubRecordId,
) -> CapturedPhysicalScope {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = super::super::reference::authorize_on(tx, owner()).await?;
                let catalog = super::super::reference::load_catalog_on(tx, hub.id()).await?;
                let state = super::super::stock::read::state_on(tx, hub.id()).await?;
                Ok::<_, SafeError>(
                    lineage::load_on(tx, hub.id(), trace, &catalog, state.revision)
                        .await?
                        .graph
                        .capture(&[group])
                        .unwrap(),
                )
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn nested_same_location_portions_retain_birth_scope_work_and_originals() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let root = received.original.position.group;
    let trace = received.original.trace.id;
    let original_work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows;
    let source = db.hub_group(owner(), root).await.unwrap();
    let input = split(&source, 8, "Blue eight");
    let first = db
        .apply_hub_split(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    let HubHandlingResult::Separated(effect) = &first.result else {
        panic!("separation receipt")
    };
    assert_eq!(
        (
            effect.source_before_packs.value(),
            effect.packs.value(),
            effect.remaining_packs.value()
        ),
        (20, 8, 12)
    );
    assert_eq!(effect.location.id, source.group.location);
    let eight = effect.portion_group;
    let root_scope = capture(&db, trace, root).await;
    let eight_scope = capture(&db, trace, eight).await;
    let source = db.hub_group(owner(), eight).await.unwrap();
    let second = db
        .apply_hub_split(
            owner(),
            operation(3),
            split(&source, 5, "Green five"),
            now(),
        )
        .await
        .unwrap();
    let HubHandlingResult::Separated(effect) = second.result else {
        panic!("separation receipt")
    };
    let five = effect.portion_group;
    let five_scope = capture(&db, trace, five).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = super::super::reference::authorize_on(tx, owner()).await?;
                let catalog = super::super::reference::load_catalog_on(tx, hub.id()).await?;
                let state = super::super::stock::read::state_on(tx, hub.id()).await?;
                let graph = lineage::load_on(tx, hub.id(), trace, &catalog, state.revision)
                    .await?
                    .graph;
                assert_eq!(graph.resolve(&root_scope).unwrap().packs().value(), 12);
                assert_eq!(graph.resolve(&root_scope).unwrap().groups().len(), 1);
                assert_eq!(graph.resolve(&eight_scope).unwrap().packs().value(), 8);
                assert_eq!(
                    graph
                        .resolve_union(&[eight_scope, five_scope])
                        .unwrap()
                        .packs()
                        .value(),
                    8
                );
                Ok::<(), SafeError>(())
            })
        })
        .await
        .unwrap();
    let page = db.hub_lineage(owner(), lineage_query(trace)).await.unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.rows.iter().map(|r| r.packs.value()).sum::<u32>(), 20);
    assert!(
        page.rows
            .iter()
            .all(|r| r.location.as_ref().unwrap().id == received.original.position.location)
    );
    assert_eq!(
        page.rows
            .iter()
            .find(|r| r.id == eight)
            .unwrap()
            .created_revision,
        first.revision
    );
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows;
    assert_eq!(work[0].work, original_work[0].work);
    assert_eq!(work[0].pending_groups, 3);
    assert_eq!(
        db.apply_hub_split(owner(), operation(2), input.clone(), now())
            .await
            .unwrap(),
        first
    );
    let mut changed = input;
    changed.packs = WholePacks::from_value(7).unwrap();
    assert_eq!(
        db.apply_hub_split(owner(), operation(2), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_lineage(owner(), lineage_query(trace)).await.unwrap(),
        page
    );
    assert_eq!(
        db.hub_handling_operation(owner(), operation(2))
            .await
            .unwrap()
            .unwrap(),
        first
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .unwrap(),
        received
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn portion_rollback_stale_competition_and_authority_leave_no_partial_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let input = split(&source, 8, "Blue eight");
    let failed = input.clone();
    assert!(
        db.host()
            .transaction(|tx| Box::pin(async move {
                portions::apply_on(tx, owner(), operation(2), failed, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            }))
            .await
            .is_err()
    );
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
        db.hub_lineage(other(), lineage_query(received.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.apply_hub_split(other(), operation(2), input.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let (one, two) = tokio::join!(
        db.apply_hub_split(owner(), operation(2), input.clone(), now()),
        db.apply_hub_split(owner(), operation(3), input, now())
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let error = if let Err(e) = one {
        e
    } else {
        two.unwrap_err()
    };
    assert_eq!(error.code(), SafeErrorCode::HubConflict);
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap().total, 2);
    db.close().await.unwrap();
}

#[tokio::test]
async fn lineage_continuations_pin_both_versions_and_reject_damaged_birth_identity() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let source = db
        .hub_group(owner(), received.original.position.group)
        .await
        .unwrap();
    let first = db
        .hub_lineage(
            owner(),
            HubLineageQuery {
                limit: 1,
                ..lineage_query(received.original.trace.id)
            },
        )
        .await
        .unwrap();
    db.apply_hub_split(
        owner(),
        operation(2),
        split(&source, 8, "Blue eight"),
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_lineage(
            owner(),
            HubLineageQuery {
                offset: 1,
                expected_revision: Some(first.revision),
                expected_catalog_revision: Some(first.catalog_revision),
                ..lineage_query(received.original.trace.id)
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    for invalid in [
        HubLineageQuery {
            limit: 65,
            ..lineage_query(received.original.trace.id)
        },
        HubLineageQuery {
            offset: 1,
            ..lineage_query(received.original.trace.id)
        },
        HubLineageQuery {
            offset: 257,
            ..lineage_query(received.original.trace.id)
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("DROP TRIGGER hub_group_identification_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_group_identification SET physical_label='Altered tag' WHERE parent_group_id IS NOT NULL").execute(&mut raw).await.unwrap();
    sqlx::query("CREATE TRIGGER hub_group_identification_no_update BEFORE UPDATE ON hub_group_identification BEGIN SELECT RAISE(ABORT, 'physical identification is immutable'); END").execute(&mut raw).await.unwrap();
    sqlx::Connection::close(raw).await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_lineage(owner(), lineage_query(received.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}
