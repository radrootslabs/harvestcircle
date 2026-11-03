//! Candidate guards run only on fresh fictional current fixtures, never retained worlds.
use crate::hub::stock::tests::{Fixture, setup};
use sqlx::{Connection, Row};

#[tokio::test]
async fn forward_custody_schema_is_additive_and_preserves_current_originals() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let original = setup(&db).await;
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let predecessor = crate::contract::retained_schema16_objects();
    let current = sqlx::query("SELECT name,tbl_name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(&mut raw).await.unwrap();
    for object in predecessor {
        let row = current
            .iter()
            .find(|row| row.get::<String, _>("name") == object.name())
            .unwrap();
        if !super::schema::REPLACED_OBJECTS.contains(&object.name())
            && !crate::hub::actuals::schema::REPLACED_OBJECTS.contains(&object.name())
        {
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
                "predecessor object {}",
                object.name()
            );
        }
    }
    for table in [
        "hub_returns",
        "hub_return_sources",
        "hub_dispositions",
        "hub_disposition_work_scope",
    ] {
        assert!(
            current
                .iter()
                .any(|row| row.get::<String, _>("name") == table)
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
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision FROM hub_context")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(catalog_revision) FROM hub_reference_versions")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        i64::try_from(original.revision().value()).unwrap()
    );
    raw.close().await.unwrap();
}
