//! Fault injection is restricted to fresh, test-owned SQLite worlds.
use crate::hub::stock::tests::Fixture;
use sqlx::{AssertSqlSafe, Connection, Row};

/// Table-value witness for large fresh fixtures. Retain only one row at a time,
/// including when immutable correction captures contain a long prior history.
pub(in crate::hub) async fn material_digest(fixture: &Fixture) -> [u8; 32] {
    material_digest_for(fixture, |_| true).await
}

pub(in crate::hub) async fn material_digest_for(
    fixture: &Fixture,
    include: fn(&str) -> bool,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    fn field(hash: &mut Sha256, value: &[u8]) {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    let mut hash = Sha256::new();
    let mut raw = fixture.raw().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'hub_%' ORDER BY name",
    )
    .fetch_all(&mut raw)
    .await
    .unwrap();
    for table in tables {
        if !include(&table) {
            continue;
        }
        assert!(
            table
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        );
        field(&mut hash, table.as_bytes());
        let columns = sqlx::query(AssertSqlSafe(format!("PRAGMA table_info({table})")))
            .fetch_all(&mut raw)
            .await
            .unwrap();
        let mut expressions = Vec::new();
        let mut keys = Vec::new();
        for column in &columns {
            let name: String = column.get("name");
            assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
            field(&mut hash, name.as_bytes());
            expressions.push(format!("quote({name})"));
            let key: i64 = column.get("pk");
            if key > 0 {
                keys.push((key, name));
            }
        }
        keys.sort();
        let order = if keys.is_empty() {
            "rowid".to_owned()
        } else {
            keys.into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(",")
        };
        let sql = format!(
            "SELECT {} FROM {table} ORDER BY {order} LIMIT 1 OFFSET ?",
            expressions.join(",")
        );
        let mut count = 0i64;
        while let Some(row) = sqlx::query(AssertSqlSafe(sql.clone()))
            .bind(count)
            .fetch_optional(&mut raw)
            .await
            .unwrap()
        {
            for column in 0..expressions.len() {
                let value: String = row.get(column);
                field(&mut hash, value.as_bytes());
            }
            count += 1;
        }
        hash.update(count.to_le_bytes());
    }
    raw.close().await.unwrap();
    hash.finalize().into()
}

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

pub(in crate::hub) async fn corrupt(fixture: &Fixture, trigger: &str, sql: &'static str) {
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
