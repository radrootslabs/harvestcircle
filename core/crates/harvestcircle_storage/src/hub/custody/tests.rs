//! Fault injection is restricted to fresh, test-owned SQLite worlds.
use crate::hub::stock::tests::Fixture;
use sqlx::{AssertSqlSafe, Connection, Row};

pub(in crate::hub) async fn material(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    let mut raw = fixture.raw().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'hub_%' ORDER BY name",
    )
    .fetch_all(&mut raw)
    .await
    .unwrap();
    let mut result = Vec::new();
    for table in tables {
        assert!(
            table
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        );
        let columns = sqlx::query(AssertSqlSafe(format!("PRAGMA table_info({table})")))
            .fetch_all(&mut raw)
            .await
            .unwrap();
        let expressions: Vec<_> = columns
            .iter()
            .map(|row| {
                let name: String = row.get("name");
                assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
                format!("quote({name})")
            })
            .collect();
        let sql = format!("SELECT {} FROM {table}", expressions.join(" || '|' || "));
        let mut rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(sql))
            .fetch_all(&mut raw)
            .await
            .unwrap();
        rows.sort();
        result.push((table, rows));
    }
    raw.close().await.unwrap();
    result
}

pub(super) async fn corrupt(fixture: &Fixture, trigger: &str, sql: &'static str) {
    assert!(
        trigger
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    );
    let mut raw = fixture.raw().await;
    // First prove the normal immutable original rejects the same mutation.
    assert!(sqlx::query(sql).execute(&mut raw).await.is_err());
    let definition: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?")
            .bind(trigger)
            .fetch_one(&mut raw)
            .await
            .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {trigger}")))
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(sql).execute(&mut raw).await.unwrap();
    sqlx::query(AssertSqlSafe(definition))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
}
