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
    let database_source = read(&crate_root.join("src/db.rs"));
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
    assert!(workspace_manifest.contains("sqlx = { version = \"=0.9.0\""));
    for forbidden_package in ["rusqlite", "refinery"] {
        assert!(
            !lock.contains(&format!("\nname = \"{forbidden_package}\"\n")),
            "lock contains forbidden SQLite package {forbidden_package}"
        );
    }

    for module in [
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
    assert!(!database_source.contains("pub fn host"));
    assert!(!database_source.contains("pub const fn host"));

    for required in [
        "pub struct harvestcircle_storage::Database",
        "pub async fn harvestcircle_storage::Database::open",
        "pub async fn harvestcircle_storage::Database::close",
        "impl harvestcircle_application::ports::DurableOperationRepository for harvestcircle_storage::Database",
        "harvestcircle_application::ports::BoxFuture",
        "pub fn harvestcircle_storage::harvestcircle_migration_catalog()",
        "pub fn harvestcircle_storage::harvestcircle_schema_catalog()",
    ] {
        assert!(api.contains(required), "API baseline is missing {required}");
    }
    for forbidden in [
        "rusqlite::",
        "refinery::",
        "sqlx::",
        "OperationJournal",
        "harvestcircle_initial_schema_sql",
        "repair",
        "preflight",
    ] {
        assert!(
            !api.contains(forbidden),
            "API baseline exposes forbidden surface {forbidden}"
        );
    }
}
