//! Fresh owned SQL fixtures only: no native association or retained upgrade claim.
use super::*;
use crate::hub::stock::tests::Fixture;
use sqlx::{Connection, Row};

fn objects21() -> Vec<SchemaObject> {
    let mut objects = crate::contract::retained_schema20_objects();
    objects.retain(|object| !super::super::count_schema::REPLACED_OBJECTS.contains(&object.name()));
    objects.extend(super::super::count_schema::schema_objects().unwrap());
    objects
}

#[test]
fn candidate_association_schema_measures_additive_catalog_and_provider_checksum() {
    use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};
    let mut objects = objects21();
    let old = objects.len();
    objects.extend(schema_objects().unwrap());
    assert_eq!(objects.len(), old + 14);
    let checksum = MigrationChecksum::for_sql(MIGRATION_SQL);
    let digest = SchemaVersionCatalog::computed_digest(22, objects).unwrap();
    println!("COUNT_RETURN22_CHECKSUM={:?}", checksum.as_bytes());
    println!("COUNT_RETURN22_DIGEST={:?}", digest.as_bytes());
}

#[test]
fn candidate_association_schema_pins_the_measured_checksum_and_catalog() {
    use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};
    let mut objects = objects21();
    objects.extend(schema_objects().unwrap());
    let checksum = MigrationChecksum::for_sql(MIGRATION_SQL);
    let digest = SchemaVersionCatalog::computed_digest(22, objects).unwrap();
    assert_eq!(checksum, MIGRATION_CHECKSUM);
    assert_eq!(digest.as_bytes(), &SCHEMA_DIGEST);
}

#[tokio::test]
async fn current22_fresh_inspector_checks_full_catalog_build_fields_and_unchanged_reopen() {
    use crate::hub::corrections::upgrade_tests::{all_objects, health, inventory, verify_objects};
    let fixture = Fixture::new();
    let db = fixture.open().await;
    assert_eq!(db.metadata().state_schema_version().get(), 22);
    let source = db.metadata().source_generation();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    health(&mut raw).await;
    let mut objects = objects21();
    objects.extend(schema_objects().unwrap());
    verify_objects(&mut raw, &objects).await;
    assert_eq!(all_objects(&mut raw).await.len(), objects.len() + 6);
    let before = inventory(&mut raw).await;
    assert_eq!(before.as_array().unwrap().len(), 76);
    for version in [21_i64, 22] {
        let row = sqlx::query("SELECT * FROM schema_migrations WHERE version=?")
            .bind(version)
            .fetch_one(&mut raw)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>("applied_at_unix_s"), 100);
        for (key, value) in [
            ("service_version", "0.1.0-alpha"),
            ("service_commit", "1111111111111111111111111111111111111111"),
            ("lib_revision", "189c49b74b4bafc142b00b76b296477931139e72"),
            ("rust_version", "1.99.0"),
            ("target", "test"),
            ("feature_profile", "test"),
        ] {
            assert_eq!(row.get::<String, _>(key), value);
        }
        for key in [
            "config_contract_version",
            "state_contract_version",
            "admin_contract_version",
            "status_contract_version",
            "provider_contract_version",
        ] {
            assert_eq!(row.get::<i64, _>(key), 1);
        }
    }
    raw.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(db.metadata().source_generation(), source);
    assert_eq!(db.metadata().state_schema_version().get(), 22);
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    assert_eq!(inventory(&mut raw).await, before);
    verify_objects(&mut raw, &objects).await;
    health(&mut raw).await;
    raw.close().await.unwrap();
}

#[tokio::test]
#[ignore = "Historical20-to22 migration body retained; actual20 first-consumer qualification remains separately due"]
async fn candidate_existing_hub_epoch_is_exact_and_every_predecessor_row_stays_unchanged() {
    let (fixture, db, query) = super::super::count_review::tests::world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let mut tx = raw.begin().await.unwrap();
    sqlx::raw_sql(super::super::count_schema::MIGRATION_SQL)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(MIGRATION_SQL)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let epoch: (i64, i64) = sqlx::query_as(
        "SELECT legacy_stock_revision,legacy_catalog_revision FROM hub_count_return_epochs",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    assert_eq!(
        epoch,
        (
            query.facts.expected_stock_revision.value() as i64,
            query.facts.expected_catalog_revision.value() as i64
        )
    );
    for object in schema_objects().unwrap() {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(object.name())
            .fetch_one(&mut raw)
            .await
            .unwrap();
        let actual = SchemaObject::computed_digest(
            object.kind(),
            object.name(),
            &row.get::<String, _>("tbl_name"),
            &row.get::<String, _>("sql"),
        )
        .unwrap();
        assert_eq!(actual, object.digest(), "{}", object.name());
    }
    assert!(
        sqlx::query(
            "UPDATE hub_count_return_epochs SET legacy_stock_revision=legacy_stock_revision+1"
        )
        .execute(&mut raw)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM hub_count_return_epochs")
            .execute(&mut raw)
            .await
            .is_err()
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
                "hub_count_interpretation_epochs"
                    | "hub_count_interpretation_markers"
                    | "hub_count_return_epochs"
                    | "hub_count_return_associations"
                    | "hub_count_return_intake_modes"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(after, before);
}

mod intake_modes;

#[tokio::test]
async fn fresh_hub_initialization_follows_parent_epoch_without_assuming_actual_trigger_order() {
    for reverse in [false, true] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        db.close().await.unwrap();
        let mut raw = fixture.raw().await;
        let mut tx = raw.begin().await.unwrap();
        // Fixture::open already installs the registered current22 schema.
        // This checks initialization order, not a predecessor upgrade.
        if reverse {
            // Recreate the old trigger last, exercising the opposite order.
            let old: &'static str = include_str!("../../actuals/schema-v20.sql")
                .split(";\n")
                .find(|sql| {
                    sql.trim()
                        .starts_with("CREATE TRIGGER hub_actual_interpretation_initialize AFTER")
                })
                .unwrap();
            sqlx::query("DROP TRIGGER hub_actual_interpretation_initialize")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::raw_sql(old).execute(&mut *tx).await.unwrap();
        }
        // Exact minimal new-hub SQL, solely for initialization trigger proof.
        // This is not a native owned command or an admitted association.
        sqlx::query("INSERT INTO hub_context (singleton,hub_id,owner_public_key,revision,initialization_id,created_at_unix_s) VALUES (1,?,?,1,?,100)")
            .bind([88u8;16].as_slice())
            .bind(crate::hub::stock::tests::owner().as_bytes().as_slice())
            .bind(crate::hub::stock::tests::operation(1).as_str())
            .execute(&mut *tx).await.unwrap();
        sqlx::query(
            "INSERT INTO hub_stock_state (singleton,hub_id,revision,event_count) VALUES (1,?,1,0)",
        )
        .bind([88u8; 16].as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
        for (table, sql) in [
            (
                "actual",
                "SELECT hub_id,epoch,legacy_stock_revision,legacy_catalog_revision FROM hub_actual_interpretation_epochs",
            ),
            (
                "count",
                "SELECT hub_id,epoch,legacy_stock_revision,legacy_catalog_revision FROM hub_count_interpretation_epochs",
            ),
            (
                "return",
                "SELECT hub_id,epoch,legacy_stock_revision,legacy_catalog_revision FROM hub_count_return_epochs",
            ),
        ] {
            let row: (Vec<u8>, i64, i64, i64) =
                sqlx::query_as(sql).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(
                row,
                (vec![88u8; 16], 1, 1, 1),
                "{table}, reversed order {reverse}"
            );
        }
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut *tx)
                .await
                .unwrap()
                .is_empty()
        );
        tx.rollback().await.unwrap();
        raw.close().await.unwrap();
    }
}
