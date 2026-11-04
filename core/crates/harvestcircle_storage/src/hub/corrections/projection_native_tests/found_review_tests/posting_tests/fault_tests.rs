//! Every corruption begins with a legally posted native original. Restore the
//! exact immutable trigger definitions before asking native readers to verify.
use super::*;
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn persisted_found_root_admission_birth_review_quantity_and_event_corruption_never_repairs() {
    for (table, update) in [
        (
            "hub_corrections",
            "UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_captures",
            "UPDATE hub_correction_captures SET scope_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_submissions",
            "UPDATE hub_submissions SET capture_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_found_allocations",
            "UPDATE hub_correction_found_allocations SET projection_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_initial_reviews",
            "UPDATE hub_correction_initial_reviews SET result_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_quantity_changes",
            "UPDATE hub_correction_quantity_changes SET projection_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_stock_events",
            "UPDATE hub_stock_events SET packs=packs+1 WHERE operation_id=?",
        ),
        (
            "hub_correction_initial_review_anchors",
            "UPDATE hub_correction_initial_review_anchors SET packs=packs+1 WHERE operation_id=?",
        ),
    ] {
        let world = world(2, PriorReturn::None).await;
        let receipt = post_world(&world).await;
        let mut raw = world.fixture.raw().await;
        assert!(
            sqlx::query(update)
                .bind(receipt.operation.as_str())
                .execute(&mut raw)
                .await
                .is_err(),
            "immutable {table}"
        );
        let triggers: Vec<(String, String)> = sqlx::query_as(
            "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY name",
        )
        .bind(table)
        .fetch_all(&mut raw)
        .await
        .unwrap();
        assert!(!triggers.is_empty());
        for (name, _) in &triggers {
            sqlx::query(AssertSqlSafe(format!(
                "DROP TRIGGER \"{}\"",
                name.replace('"', "\"\"")
            )))
            .execute(&mut raw)
            .await
            .unwrap();
        }
        assert_eq!(
            sqlx::query(update)
                .bind(receipt.operation.as_str())
                .execute(&mut raw)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        for (_, ddl) in triggers {
            sqlx::query(AssertSqlSafe(ddl))
                .execute(&mut raw)
                .await
                .unwrap();
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
            SafeErrorCode::StorageCorrupt,
            "original {table}"
        );
        assert_eq!(
            world
                .db
                .hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "current {table}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            material,
            "no repair {table}"
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn bounded_bulk_group_read_keeps_every_later_group_identity_and_work_check() {
    let world = two_found_world().await;
    let receipt = post_world(&world).await;
    let ids = receipt
        .original
        .allocations
        .iter()
        .map(|a| a.group.id)
        .collect::<Vec<_>>();
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let catalog = world.db.hub_references(owner()).await.unwrap();
    let selected = ids.clone();
    let checked = catalog.clone();
    let groups = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move { handling::read::groups_on(tx, hub, &selected, &checked).await })
        })
        .await
        .unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].trace, groups[1].trace);
    assert_eq!(groups[1], receipt.original.allocations[1].group);
    let mut raw = world.fixture.raw().await;
    // Old work assigned to the later found child must still fail the bulk read.
    sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) SELECT ?,work_id,'pending',1 FROM hub_work_requirements WHERE trace_id=? LIMIT 1")
        .bind(ids[1].as_bytes().as_slice()).bind(receipt.original.original_target.trace.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let material = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        world
            .db
            .host()
            .transaction(|tx| Box::pin(async move {
                handling::read::groups_on(tx, hub, &ids, &catalog).await
            }))
            .await
            .map_err(map_transaction_error)
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
