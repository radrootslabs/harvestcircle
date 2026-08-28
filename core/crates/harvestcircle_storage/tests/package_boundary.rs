use std::{fs, path::Path};

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn storage_package_keeps_one_sqlite_authority_and_a_sealed_public_surface() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_root.join("../..");
    let manifest = read(&crate_root.join("Cargo.toml"));
    let workspace_manifest = read(&workspace_root.join("Cargo.toml"));
    let lock = read(&workspace_root.join("Cargo.lock"));
    let root_source = read(&crate_root.join("src/lib.rs"));
    let contract_source = read(&crate_root.join("src/contract.rs"));
    let database_source = read(&crate_root.join("src/db.rs"));
    let journal_source = read(&crate_root.join("src/journal.rs"));
    let keyring_source = read(&crate_root.join("src/os_keyring.rs"));
    let api = read(&workspace_root.join("compatibility/harvestcircle-storage-api-v1.txt"));

    for forbidden in ["rusqlite", "refinery", "hmac", "rustix"] {
        assert!(
            !manifest.contains(forbidden),
            "storage manifest reintroduced direct {forbidden} authority"
        );
    }
    assert!(manifest.contains("radroots_service_sqlite.workspace = true"));
    assert!(manifest.contains("sqlx.workspace = true"));
    assert!(!manifest.contains("\nkeyring ="));
    assert!(manifest.contains("secret-service = { version = \"=5.1.0\""));
    assert!(manifest.contains("security-framework = \"=3.7.0\""));
    assert!(manifest.contains("security-framework-sys = \"=2.17.0\""));
    assert!(workspace_manifest.contains("sqlx = { version = \"=0.9.0\""));
    for forbidden_package in ["rusqlite", "refinery"] {
        assert!(
            !lock.contains(&format!("\nname = \"{forbidden_package}\"\n")),
            "lock contains forbidden SQLite package {forbidden_package}"
        );
    }

    for module in [
        "backup",
        "contract",
        "db",
        "identities",
        "identity_namespace",
        "installation",
        "journal",
        "os_keyring",
        "profiles",
    ] {
        assert!(root_source.contains(&format!("mod {module};")));
        assert!(!root_source.contains(&format!("pub mod {module};")));
    }
    assert!(!root_source.contains("harvestcircle_initial_schema_sql"));
    assert!(!keyring_source.contains("PoisonError::into_inner"));
    assert!(!keyring_source.contains("set_password"));
    assert!(keyring_source.contains("add_generic_password"));
    assert!(keyring_source.contains("CREDENTIAL_OPERATION_ATTRIBUTE"));
    assert!(keyring_source.contains("false,\n            \"application/octet-stream\""));
    assert!(!database_source.contains("pub fn host"));
    assert!(!database_source.contains("pub const fn host"));

    for required in [
        "pub struct harvestcircle_storage::Database",
        "pub async fn harvestcircle_storage::Database::open",
        "pub async fn harvestcircle_storage::Database::close",
        "pub async fn harvestcircle_storage::Database::capture_online_backup",
        "pub async fn harvestcircle_storage::Database::restore_verified_backup",
        "pub struct harvestcircle_storage::VerifiedHarvestCircleBackup",
        "pub fn harvestcircle_storage::verify_harvestcircle_backup",
        "impl harvestcircle_application::ports::DurableOperationRepository for harvestcircle_storage::Database",
        "harvestcircle_application::ports::BoxFuture",
        "pub fn harvestcircle_storage::OsKeyringSecretStore::contains(&self, harvestcircle_domain::key::PublicKey) -> harvestcircle_application::ports::BoxFuture",
        "pub fn harvestcircle_storage::OsKeyringSecretStore::put<'a>(&'a self, &'a harvestcircle_application::ports::DurableRequestId, harvestcircle_domain::key::PublicKey, harvestcircle_domain::key::SecretKeyInput) -> harvestcircle_application::ports::BoxFuture<'a",
        "pub fn harvestcircle_storage::OsKeyringSecretStore::delete<'a>(&'a self, &'a harvestcircle_application::ports::DurableRequestId, harvestcircle_domain::key::PublicKey) -> harvestcircle_application::ports::BoxFuture<'a",
        "pub fn harvestcircle_storage::harvestcircle_migration_catalog()",
        "pub fn harvestcircle_storage::harvestcircle_schema_catalog()",
        "pub const harvestcircle_storage::HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY: usize",
        "pub const harvestcircle_storage::HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH: usize",
        "pub const harvestcircle_storage::HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS: i64",
    ] {
        assert!(api.contains(required), "API baseline is missing {required}");
    }
    for forbidden in [
        "rusqlite::",
        "refinery::",
        "sqlx::",
        "OperationJournal",
        "harvestcircle_initial_schema_sql",
        "VerifiedServiceBackup",
        "StagedServiceRestore",
        "verify_backup_bundle",
        "stage_verified_restore",
        "finalize_staged_restore",
        "repair",
        "preflight",
    ] {
        assert!(
            !api.contains(forbidden),
            "API baseline exposes forbidden surface {forbidden}"
        );
    }

    for required in [
        "pub const HARVESTCIRCLE_STATE_SCHEMA_VERSION: u32 = 2;",
        "pub const HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY: usize = 1_024;",
        "pub const HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY: usize = 4_096;",
        "pub const HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH: usize = 256;",
        "pub const HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;",
        "bound_durable_operation_receipts",
        "completed_at_unix_s",
        "durable_operations_receipt_insert_guard",
        "durable_operations_receipt_update_guard",
    ] {
        assert!(
            contract_source.contains(required),
            "storage contract is missing {required}"
        );
    }
    for required in [
        "LIMIT 1025",
        "LIMIT 4097",
        "LIMIT 256",
        "completed_at_unix_s < ?",
        "completed_at_unix_s = ?",
    ] {
        assert!(
            journal_source.contains(required),
            "journal enforcement is missing {required}"
        );
    }
    for forbidden in [
        "DELETE FROM durable_operations WHERE terminal_outcome IS NOT NULL",
        "DELETE FROM durable_operations WHERE completed_at_unix_s IS NOT NULL;",
        "ORDER BY completed_at_unix_s DESC",
    ] {
        assert!(
            !journal_source.contains(forbidden),
            "journal reintroduced unbounded or in-window eviction: {forbidden}"
        );
    }
}
