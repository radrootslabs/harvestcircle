//! Registered schema constraints on disposable current fixtures only. Raw roots
//! below are SQL fault fixtures, never native admission or effect acceptance.
use super::*;
use sqlx::Row;

#[tokio::test]
async fn registered_schema_retains18_and_exact_forward_replacements_without_clock_truth() {
    use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};
    assert_eq!(
        MigrationChecksum::for_sql(crate::hub::actuals::schema::MIGRATION_SQL),
        crate::hub::actuals::schema::MIGRATION_CHECKSUM
    );
    assert_eq!(
        SchemaVersionCatalog::computed_digest(18, crate::contract::retained_schema18_objects())
            .unwrap()
            .as_bytes(),
        &crate::hub::actuals::schema::SCHEMA_DIGEST
    );
    let mut replacements = crate::hub::corrections::schema::schema_objects().unwrap();
    replacements.extend(crate::hub::actuals::interpretation_schema::schema_objects().unwrap());
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let original = received(&db).await;
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    for object in crate::hub::actuals::schema::schema_objects().unwrap() {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=? LIMIT 1")
            .bind(object.name())
            .fetch_one(&mut raw)
            .await
            .unwrap();
        let digest = radroots_service_sqlite::SchemaObject::computed_digest(
            object.kind(),
            object.name(),
            &row.get::<String, _>("tbl_name"),
            &row.get::<String, _>("sql"),
        )
        .unwrap();
        let expected = if crate::hub::corrections::schema::REPLACED_OBJECTS.contains(&object.name())
            || crate::hub::actuals::interpretation_schema::REPLACED_OBJECTS.contains(&object.name())
        {
            let replacement = replacements
                .iter()
                .find(|candidate| candidate.name() == object.name())
                .unwrap();
            assert_ne!(
                replacement.digest(),
                object.digest(),
                "forward definition remains distinct from18"
            );
            replacement.digest()
        } else {
            object.digest()
        };
        assert_eq!(
            digest,
            expected,
            "registered actual schema object {}",
            object.name()
        );
    }
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
    // Deliberately synthetic immutable operation root: this tests SQL constraints
    // only. It has no native admission, marker, result seal or execution grant.
    let operation = operation(80);
    let report = id(80);
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) SELECT ?,hub_id,actor_public_key,'actual_report',zeroblob(32),catalog_revision,3,?,150 FROM hub_stock_operations WHERE operation_id=?")
        .bind(operation.as_str()).bind(report.as_bytes().as_slice()).bind(original.operation.as_str()).execute(&mut raw).await.unwrap();
    for (kind, start, end) in [
        ("known", None, Some(160)),
        ("known", Some(160), None),
        ("interval", Some(160), None),
        ("interval", None, Some(160)),
        ("interval", Some(170), Some(160)),
        ("unknown", Some(160), Some(160)),
    ] {
        assert!(sqlx::query("INSERT INTO hub_actual_reports (report_id,operation_id,policy_version,source_kind,source_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) VALUES (?,?,1,'unknown',0,?,?,?,'Raw clock claim',zeroblob(32))")
            .bind(report.as_bytes().as_slice()).bind(operation.as_str()).bind(kind).bind(start).bind(end).execute(&mut raw).await.is_err());
    }
    sqlx::query("INSERT INTO hub_actual_reports (report_id,operation_id,policy_version,source_kind,source_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) VALUES (?,?,1,'unknown',0,'known',160,160,'Future claim remains unverified',zeroblob(32))")
        .bind(report.as_bytes().as_slice()).bind(operation.as_str()).execute(&mut raw).await.unwrap();
    let row=sqlx::query("SELECT occurrence_start_unix_s,(SELECT recorded_at_unix_s FROM hub_stock_operations WHERE operation_id=hub_actual_reports.operation_id) AS recorded FROM hub_actual_reports WHERE report_id=?")
        .bind(report.as_bytes().as_slice()).fetch_one(&mut raw).await.unwrap();
    assert_eq!(row.get::<i64, _>("occurrence_start_unix_s"), 160);
    assert_eq!(row.get::<i64, _>("recorded"), 150);
    assert!(sqlx::query("UPDATE hub_actual_reports SET occurrence_start_unix_s=140,occurrence_end_unix_s=140 WHERE report_id=?").bind(report.as_bytes().as_slice()).execute(&mut raw).await.is_err());
    assert!(
        sqlx::query("DELETE FROM hub_actual_reports WHERE report_id=?")
            .bind(report.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO hub_actual_report_reviews (report_id,operation_id,review_operation_id) VALUES (?,?,NULL)").bind(report.as_bytes().as_slice()).bind(operation.as_str()).execute(&mut raw).await.is_err());
    assert!(
        sqlx::query(
            "INSERT INTO hub_actual_report_sources (report_id,ordinal,trace_id) VALUES (?,0,?)"
        )
        .bind(report.as_bytes().as_slice())
        .bind(original.original.trace.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .is_err()
    );
    raw.close().await.unwrap();
}
