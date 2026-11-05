//! Candidate SQL checks on fresh owned fixtures only. These neither open a
//! retained predecessor nor qualify a native21 producer or historical upgrade.
use crate::hub::stock::tests::{Fixture, command, now, operation, owner, setup};
use sqlx::Connection;
const SQL21: &str = include_str!("../schema-v21.sql");

#[test]
fn candidate_count_schema_metadata_binds_complete_native_object_inventory() {
    use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};
    let schema = crate::hub::corrections::count_schema::schema_objects().unwrap();
    assert_eq!(schema.len(), 11);
    let mut objects = crate::contract::retained_schema20_objects();
    objects.retain(|object| {
        !crate::hub::corrections::count_schema::REPLACED_OBJECTS.contains(&object.name())
    });
    objects.extend(schema);
    let checksum = MigrationChecksum::for_sql(SQL21);
    let digest = SchemaVersionCatalog::computed_digest(21, objects).unwrap();
    println!("COUNT21_CHECKSUM={:?}", checksum.as_bytes());
    println!("COUNT21_DIGEST={:?}", digest.as_bytes());
    assert_eq!(
        checksum,
        crate::hub::corrections::count_schema::MIGRATION_CHECKSUM
    );
    assert_eq!(
        digest.as_bytes(),
        &crate::hub::corrections::count_schema::SCHEMA_DIGEST
    );
}

#[tokio::test]
async fn candidate_count_schema_keeps_every_old_row_and_binds_exact_activation_cut() {
    let (fixture, db, query) = crate::hub::corrections::count_review::tests::world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let old_guard: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='hub_actual_review_guard'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    let mut tx = raw.begin().await.unwrap();
    sqlx::raw_sql(SQL21).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let cut: (i64, i64) = sqlx::query_as(
        "SELECT legacy_stock_revision,legacy_catalog_revision FROM hub_count_interpretation_epochs",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    assert_eq!(
        cut,
        (
            i64::try_from(query.facts.expected_stock_revision.value()).unwrap(),
            i64::try_from(query.facts.expected_catalog_revision.value()).unwrap()
        )
    );
    let new_guard: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='hub_actual_review_guard'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    assert_ne!(new_guard, old_guard);
    assert!(new_guard.contains("hub_count_interpretation_markers"));
    assert!(new_guard.contains("known.kind='correction_count_explain'"));
    for object in crate::hub::corrections::count_schema::schema_objects().unwrap() {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(object.name())
            .fetch_one(&mut raw)
            .await
            .unwrap();
        use sqlx::Row;
        let actual = radroots_service_sqlite::SchemaObject::computed_digest(
            object.kind(),
            object.name(),
            &row.get::<String, _>("tbl_name"),
            &row.get::<String, _>("sql"),
        )
        .unwrap();
        assert_eq!(
            actual,
            object.digest(),
            "candidate count object {}",
            object.name()
        );
    }
    assert!(sqlx::query("UPDATE hub_count_interpretation_epochs SET legacy_stock_revision=legacy_stock_revision+1").execute(&mut raw).await.is_err());
    // No old root may be relabeled as count-aware, even if a caller copies
    // all activation fields and supplies a 32-byte hash.
    let error=sqlx::query(concat!(
        "INSERT INTO hub_count_interpretation_markers (operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,capture_hash) ",
        "SELECT o.operation_id,o.hub_id,1,e.legacy_stock_revision,e.legacy_catalog_revision,e.legacy_stock_revision+1,e.legacy_catalog_revision,zeroblob(32) ",
        "FROM hub_stock_operations o JOIN hub_count_interpretation_epochs e ON e.hub_id=o.hub_id ORDER BY o.stock_revision LIMIT 1"
    )).execute(&mut raw).await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .message()
            .contains("qualified review marker")
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut raw)
            .await
            .unwrap()
            .is_empty()
    );
    raw.close().await.unwrap();
    let after = crate::hub::custody::tests::material(&fixture)
        .await
        .into_iter()
        .filter(|(table, _)| {
            !matches!(
                table.as_str(),
                "hub_count_interpretation_epochs" | "hub_count_interpretation_markers"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(after, before);
}

#[tokio::test]
async fn candidate_activation_rejects_unqualified_predecessor_count_root_atomically() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    // Deliberately unqualified SQL fault root, not native Count execution.
    sqlx::query(concat!(
        "INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) ",
        "SELECT ?,hub_id,actor_public_key,'correction_count_explain',zeroblob(32),catalog_revision,stock_revision+1,?,recorded_at_unix_s ",
        "FROM hub_stock_operations WHERE operation_id=?"
    )).bind(operation(9).as_str()).bind([9u8;16].as_slice()).bind(operation(1).as_str()).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let mut raw = fixture.raw().await;
    let mut tx = raw.begin().await.unwrap();
    let error = sqlx::raw_sql(SQL21).execute(&mut *tx).await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .message()
            .contains("interpretation epoch")
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'hub_count_interpretation%'"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        0
    );
    raw.close().await.unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
}
