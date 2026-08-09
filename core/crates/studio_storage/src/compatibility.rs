use std::path::Path;

use radroots_studio_domain::{
    AccountIdentity, PersistedPublicKeyClassification, SafeError, SafeErrorCode, SafeMessage,
    classify_persisted_public_key,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

use crate::CURRENT_SCHEMA_VERSION;

const KNOWN_TABLES: &[(&str, u32)] = &[
    ("application_schema", 1),
    ("accounts", 2),
    ("app_state", 2),
    ("profile_cache", 3),
    ("account_namespace", 4),
    ("operation_journal", 5),
    ("account_identities", 6),
    ("local_signer_bindings", 6),
    ("runtime_state", 6),
    ("profile_cache_v6", 6),
    ("durable_operations", 6),
    ("account_preferences", 8),
    ("installation_identity", 10),
];

const PUBLIC_KEY_COLUMNS: &[(&str, &str)] = &[
    ("accounts", "pubkey"),
    ("app_state", "selected_pubkey"),
    ("profile_cache", "subject_pubkey"),
    ("account_namespace", "owner_pubkey"),
    ("operation_journal", "subject_pubkey"),
    ("account_identities", "public_key"),
    ("local_signer_bindings", "account_public_key"),
    ("local_signer_bindings", "binding_public_key"),
    ("runtime_state", "selected_public_key"),
    ("runtime_state", "active_account_public_key"),
    ("runtime_state", "active_binding_public_key"),
    ("profile_cache_v6", "subject_public_key"),
    ("durable_operations", "account_public_key"),
    ("durable_operations", "binding_public_key"),
    ("durable_operations", "prior_selected_public_key"),
    ("account_preferences", "owner_public_key"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatabasePreflight {
    Fresh,
    Ready {
        schema_version: u32,
    },
    Quarantined {
        schema_version: u32,
        issues: Vec<PersistedIdentityIssue>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistedIdentityIssueKind {
    MalformedEncoding,
    NonCanonicalEncoding,
    InvalidCurvePoint,
    DisplayIdentityMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedIdentityIssue {
    table: &'static str,
    column: &'static str,
    row_id: i64,
    kind: PersistedIdentityIssueKind,
    fingerprint: [u8; 32],
}

impl PersistedIdentityIssue {
    #[must_use]
    pub const fn table(&self) -> &'static str {
        self.table
    }

    #[must_use]
    pub const fn column(&self) -> &'static str {
        self.column
    }

    #[must_use]
    pub const fn row_id(&self) -> i64 {
        self.row_id
    }

    #[must_use]
    pub const fn kind(&self) -> PersistedIdentityIssueKind {
        self.kind
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

pub(crate) fn preflight(path: &Path) -> Result<DatabasePreflight, SafeError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(corrupt_storage_error());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DatabasePreflight::Fresh);
        }
        Err(_) => return Err(corrupt_storage_error()),
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection =
        Connection::open_with_flags(path, flags).map_err(|_| corrupt_storage_error())?;
    connection
        .pragma_update(None, "trusted_schema", "OFF")
        .map_err(|_| corrupt_storage_error())?;
    let integrity: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(|_| corrupt_storage_error())?;
    if integrity != "ok" {
        return Err(corrupt_storage_error());
    }
    let schema_version = schema_version(&connection)?;
    if schema_version == 0 || schema_version > CURRENT_SCHEMA_VERSION {
        return Err(unsupported_schema_error());
    }
    validate_schema_inventory(&connection, schema_version)?;

    let mut issues = Vec::new();
    for &(table, column) in PUBLIC_KEY_COLUMNS {
        if column_exists(&connection, table, column)? {
            scan_public_key_column(&connection, table, column, &mut issues)?;
        }
    }
    scan_display_identities(&connection, "accounts", "pubkey", "npub", &mut issues)?;
    scan_display_identities(
        &connection,
        "account_identities",
        "public_key",
        "npub",
        &mut issues,
    )?;
    issues.sort_by_key(|issue| (issue.table, issue.column, issue.row_id));
    if issues.is_empty() {
        Ok(DatabasePreflight::Ready { schema_version })
    } else {
        Ok(DatabasePreflight::Quarantined {
            schema_version,
            issues,
        })
    }
}

fn schema_version(connection: &Connection) -> Result<u32, SafeError> {
    if !table_exists(connection, "refinery_schema_history")? {
        return Err(unsupported_schema_error());
    }
    connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM refinery_schema_history",
            [],
            |row| row.get(0),
        )
        .map_err(|_| corrupt_storage_error())
}

fn validate_schema_inventory(connection: &Connection, version: u32) -> Result<(), SafeError> {
    for &(table, introduced) in KNOWN_TABLES {
        let present = table_exists(connection, table)?;
        if present != (version >= introduced) {
            return Err(corrupt_storage_error());
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'refinery_schema_history'",
        )
        .map_err(|_| corrupt_storage_error())?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| corrupt_storage_error())?;
    for name in names {
        let name = name.map_err(|_| corrupt_storage_error())?;
        if !KNOWN_TABLES.iter().any(|(known, _)| *known == name) {
            return Err(corrupt_storage_error());
        }
    }
    Ok(())
}

fn scan_public_key_column(
    connection: &Connection,
    table: &'static str,
    column: &'static str,
    issues: &mut Vec<PersistedIdentityIssue>,
) -> Result<(), SafeError> {
    let sql = format!("SELECT rowid, {column} FROM {table} WHERE {column} IS NOT NULL");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|_| corrupt_storage_error())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| corrupt_storage_error())?;
    for row in rows {
        let (row_id, value) = row.map_err(|_| corrupt_storage_error())?;
        let kind = match classify_persisted_public_key(&value) {
            PersistedPublicKeyClassification::Canonical(_) => continue,
            PersistedPublicKeyClassification::MalformedEncoding => {
                PersistedIdentityIssueKind::MalformedEncoding
            }
            PersistedPublicKeyClassification::NonCanonicalEncoding => {
                PersistedIdentityIssueKind::NonCanonicalEncoding
            }
            PersistedPublicKeyClassification::InvalidCurvePoint => {
                PersistedIdentityIssueKind::InvalidCurvePoint
            }
        };
        issues.push(issue(table, column, row_id, kind, &value));
    }
    Ok(())
}

fn scan_display_identities(
    connection: &Connection,
    table: &'static str,
    key_column: &'static str,
    npub_column: &'static str,
    issues: &mut Vec<PersistedIdentityIssue>,
) -> Result<(), SafeError> {
    if !column_exists(connection, table, key_column)?
        || !column_exists(connection, table, npub_column)?
    {
        return Ok(());
    }
    let sql = format!("SELECT rowid, {key_column}, {npub_column} FROM {table}");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|_| corrupt_storage_error())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| corrupt_storage_error())?;
    for row in rows {
        let (row_id, key, npub) = row.map_err(|_| corrupt_storage_error())?;
        let PersistedPublicKeyClassification::Canonical(public_key) =
            classify_persisted_public_key(&key)
        else {
            continue;
        };
        if AccountIdentity::verify(public_key, npub.clone()).is_err() {
            issues.push(issue(
                table,
                npub_column,
                row_id,
                PersistedIdentityIssueKind::DisplayIdentityMismatch,
                &npub,
            ));
        }
    }
    Ok(())
}

fn issue(
    table: &'static str,
    column: &'static str,
    row_id: i64,
    kind: PersistedIdentityIssueKind,
    value: &str,
) -> PersistedIdentityIssue {
    PersistedIdentityIssue {
        table,
        column,
        row_id,
        kind,
        fingerprint: Sha256::digest(value.as_bytes()).into(),
    }
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, SafeError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(|_| corrupt_storage_error())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, SafeError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let sql = format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)");
    connection
        .query_row(&sql, [column], |row| row.get(0))
        .map_err(|_| corrupt_storage_error())
}

const fn corrupt_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The application database could not be read."),
    )
}

const fn unsupported_schema_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::UnsupportedSchemaVersion,
        SafeMessage::new("The application database schema is not supported."),
    )
}

pub(crate) const fn quarantined_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageQuarantined,
        SafeMessage::new("The application database requires authenticated repair."),
    )
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use super::{
        DatabasePreflight, PersistedIdentityIssueKind, column_exists, preflight,
        scan_display_identities, scan_public_key_column,
    };
    use crate::Database;
    use radroots_studio_domain::{AccountIdentity, PublicKey, SafeErrorCode};

    #[test]
    fn preflight_rejects_non_files_missing_schema_zero_version_and_unknown_tables() {
        let directory = tempdir().expect("temporary directory");
        let missing = directory.path().join("missing.sqlite3");
        assert_eq!(
            preflight(&missing).expect("fresh preflight"),
            DatabasePreflight::Fresh
        );
        assert_eq!(
            preflight(directory.path())
                .expect_err("directory must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        let regular_parent = directory.path().join("regular-parent");
        std::fs::write(&regular_parent, b"not a directory").expect("write regular parent");
        assert_eq!(
            preflight(&regular_parent.join("nested.sqlite3"))
                .expect_err("non-directory parent must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );

        let no_schema = directory.path().join("no-schema.sqlite3");
        drop(Connection::open(&no_schema).expect("blank sqlite database"));
        assert_eq!(
            preflight(&no_schema)
                .expect_err("missing schema history")
                .code(),
            SafeErrorCode::UnsupportedSchemaVersion
        );

        let zero_schema = directory.path().join("zero-schema.sqlite3");
        let connection = Connection::open(&zero_schema).expect("zero schema database");
        connection
            .execute(
                "CREATE TABLE refinery_schema_history (version INTEGER NOT NULL)",
                [],
            )
            .expect("schema history");
        connection
            .execute(
                "INSERT INTO refinery_schema_history (version) VALUES (0)",
                [],
            )
            .expect("zero version");
        drop(connection);
        assert_eq!(
            preflight(&zero_schema)
                .expect_err("zero schema version")
                .code(),
            SafeErrorCode::UnsupportedSchemaVersion
        );

        let unknown = directory.path().join("unknown-table.sqlite3");
        drop(Database::open(&unknown).expect("current database"));
        let connection = Connection::open(&unknown).expect("open current database");
        connection
            .execute("CREATE TABLE ungoverned_table (value INTEGER)", [])
            .expect("unknown table");
        drop(connection);
        assert_eq!(
            preflight(&unknown)
                .expect_err("unknown table must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );
    }

    #[test]
    fn identity_scans_classify_all_persisted_key_and_display_failures() {
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute("CREATE TABLE identities (public_key TEXT, npub TEXT)", [])
            .expect("identity table");
        let canonical =
            PublicKey::from_hex("585591529da0bab31b3b1b1f986611cf5f435dca84f978c89ee8a40cca7103df")
                .expect("canonical key");
        let npub = AccountIdentity::derive(canonical)
            .expect("identity")
            .npub()
            .as_str()
            .to_owned();
        let values = [
            (canonical.to_hex(), npub),
            (canonical.to_hex().to_uppercase(), "invalid-npub".to_owned()),
            ("bad".to_owned(), "invalid-npub".to_owned()),
            ("00".repeat(32), "invalid-npub".to_owned()),
            (canonical.to_hex(), "invalid-npub".to_owned()),
        ];
        for (public_key, npub) in values {
            connection
                .execute(
                    "INSERT INTO identities (public_key, npub) VALUES (?1, ?2)",
                    params![public_key, npub],
                )
                .expect("identity row");
        }

        assert!(column_exists(&connection, "identities", "public_key").expect("column"));
        assert!(!column_exists(&connection, "missing", "public_key").expect("missing table"));
        assert!(!column_exists(&connection, "identities", "missing").expect("missing column"));
        let mut issues = Vec::new();
        scan_public_key_column(&connection, "identities", "public_key", &mut issues)
            .expect("scan public keys");
        scan_display_identities(&connection, "identities", "public_key", "npub", &mut issues)
            .expect("scan display identities");
        scan_display_identities(&connection, "missing", "public_key", "npub", &mut issues)
            .expect("skip missing table");
        connection
            .execute("CREATE TABLE key_only (public_key TEXT)", [])
            .expect("key-only table");
        scan_display_identities(&connection, "key_only", "public_key", "npub", &mut issues)
            .expect("skip missing display column");

        for kind in [
            PersistedIdentityIssueKind::MalformedEncoding,
            PersistedIdentityIssueKind::NonCanonicalEncoding,
            PersistedIdentityIssueKind::InvalidCurvePoint,
            PersistedIdentityIssueKind::DisplayIdentityMismatch,
        ] {
            assert!(issues.iter().any(|issue| issue.kind() == kind));
        }
        for issue in &issues {
            assert_eq!(issue.table(), "identities");
            assert!(matches!(issue.column(), "public_key" | "npub"));
            assert!(issue.row_id() > 0);
            assert_ne!(issue.fingerprint(), &[0_u8; 32]);
        }
    }
}
