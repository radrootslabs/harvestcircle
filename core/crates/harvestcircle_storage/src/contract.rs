//! Sealed HarvestCircle service-state identity and schema contract.

use core::{fmt, num::NonZeroU32};
use std::error::Error;

use radroots_runtime_paths::RuntimeContext;
use radroots_service_sqlite::{
    MigrationCatalog, MigrationDescriptor, SchemaCatalog, SchemaCatalogContractError, SchemaDigest,
    SchemaObject, SchemaObjectKind, SchemaVersionCatalog, ServiceSqliteApplicationId,
    ServiceSqlitePaths,
};

pub const HARVESTCIRCLE_SERVICE_ID: &str = "harvestcircle";
pub const HARVESTCIRCLE_INSTANCE_ID: &str = "desktop";
pub const HARVESTCIRCLE_APPLICATION_ID: u32 = 0x4843_5231;
pub const HARVESTCIRCLE_STATE_SCHEMA_VERSION: u32 = 1;
pub const HARVESTCIRCLE_IDENTITY_CAPACITY: usize = 256;
pub const HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY: usize = 1_024;
pub const HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES: usize = 4_096;
pub const HARVESTCIRCLE_RELAY_ENDPOINT_CAPACITY: usize = 16;
pub const HARVESTCIRCLE_RELAY_URL_UTF8_BYTES: usize = 2_048;
pub const HARVESTCIRCLE_EVENTS_PER_RELAY_CAPACITY: usize = 64;
pub const HARVESTCIRCLE_EVENTS_TOTAL_CAPACITY: usize = 1_024;
pub const HARVESTCIRCLE_OBSERVER_CAPACITY: usize = 32;
pub const HARVESTCIRCLE_ACTOR_MAILBOX_CAPACITY: usize = 64;
pub const HARVESTCIRCLE_COMMAND_DEADLINE_MIN_MS: u64 = 1;
pub const HARVESTCIRCLE_COMMAND_DEADLINE_MAX_MS: u64 = 30_000;

pub(crate) const CREATE_ACCOUNT_IDENTITIES_SQL: &str = r#"CREATE TABLE account_identities (
    public_key BLOB NOT NULL PRIMARY KEY CHECK (length(public_key) = 32),
    npub TEXT NOT NULL UNIQUE CHECK (length(CAST(npub AS BLOB)) = 63),
    label TEXT CHECK (label IS NULL OR length(CAST(label AS BLOB)) BETWEEN 1 AND 80),
    created_at_unix_s INTEGER NOT NULL CHECK (created_at_unix_s >= 0),
    last_used_at_unix_s INTEGER CHECK (last_used_at_unix_s IS NULL OR last_used_at_unix_s >= 0)
) STRICT"#;

pub(crate) const CREATE_LOCAL_SIGNER_BINDINGS_SQL: &str = r#"CREATE TABLE local_signer_bindings (
    account_public_key BLOB NOT NULL,
    binding_public_key BLOB NOT NULL,
    binding_kind TEXT NOT NULL CHECK (binding_kind = 'local_secret'),
    availability TEXT NOT NULL CHECK (
        availability IN ('available', 'credential_missing', 'store_unavailable')
    ),
    PRIMARY KEY (account_public_key, binding_public_key),
    UNIQUE (account_public_key, binding_kind),
    FOREIGN KEY (account_public_key) REFERENCES account_identities(public_key) ON DELETE CASCADE,
    CHECK (length(account_public_key) = 32),
    CHECK (length(binding_public_key) = 32),
    CHECK (account_public_key = binding_public_key)
) STRICT"#;

pub(crate) const CREATE_RUNTIME_STATE_SQL: &str = r#"CREATE TABLE runtime_state (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    selected_public_key BLOB REFERENCES account_identities(public_key) ON DELETE SET NULL,
    active_account_public_key BLOB,
    active_binding_public_key BLOB,
    session_generation INTEGER NOT NULL DEFAULT 0 CHECK (session_generation >= 0),
    FOREIGN KEY (active_account_public_key, active_binding_public_key)
        REFERENCES local_signer_bindings(account_public_key, binding_public_key)
        ON DELETE SET NULL,
    CHECK (selected_public_key IS NULL OR length(selected_public_key) = 32),
    CHECK (
        (active_account_public_key IS NULL AND active_binding_public_key IS NULL)
        OR
        (length(active_account_public_key) = 32 AND length(active_binding_public_key) = 32)
    )
) STRICT"#;

pub(crate) const CREATE_PROFILE_CACHE_SQL: &str = r#"CREATE TABLE profile_cache (
    subject_public_key BLOB NOT NULL PRIMARY KEY
        REFERENCES account_identities(public_key) ON DELETE CASCADE,
    event_id BLOB NOT NULL CHECK (length(event_id) = 32),
    event_created_at_unix_s INTEGER NOT NULL CHECK (event_created_at_unix_s >= 0),
    name TEXT CHECK (name IS NULL OR length(CAST(name AS BLOB)) BETWEEN 1 AND 128),
    display_name TEXT CHECK (display_name IS NULL OR length(CAST(display_name AS BLOB)) BETWEEN 1 AND 128),
    nip05 TEXT CHECK (nip05 IS NULL OR length(CAST(nip05 AS BLOB)) BETWEEN 1 AND 320),
    about TEXT CHECK (about IS NULL OR length(CAST(about AS BLOB)) BETWEEN 1 AND 4096),
    picture TEXT CHECK (picture IS NULL OR length(CAST(picture AS BLOB)) BETWEEN 1 AND 2048),
    refreshed_at_unix_s INTEGER NOT NULL CHECK (refreshed_at_unix_s >= 0),
    refresh_status TEXT NOT NULL CHECK (refresh_status IN ('success', 'offline', 'invalid_data'))
) STRICT"#;

pub(crate) const CREATE_ACCOUNT_PREFERENCES_SQL: &str = r#"CREATE TABLE account_preferences (
    owner_public_key BLOB NOT NULL REFERENCES account_identities(public_key) ON DELETE CASCADE,
    preference_key TEXT NOT NULL CHECK (preference_key = 'namespace_probe'),
    preference_value TEXT NOT NULL CHECK (
        length(CAST(preference_value AS BLOB)) BETWEEN 1 AND 4096
    ),
    PRIMARY KEY (owner_public_key, preference_key),
    CHECK (length(owner_public_key) = 32)
) STRICT"#;

pub(crate) const CREATE_DURABLE_OPERATIONS_SQL: &str = r#"CREATE TABLE durable_operations (
    request_id TEXT NOT NULL PRIMARY KEY CHECK (
        length(CAST(request_id AS BLOB)) = 36
        AND request_id = lower(request_id)
        AND request_id NOT GLOB '*[^0-9a-f-]*'
        AND substr(request_id, 9, 1) = '-'
        AND substr(request_id, 14, 1) = '-'
        AND substr(request_id, 15, 1) = '7'
        AND substr(request_id, 19, 1) = '-'
        AND substr(request_id, 20, 1) IN ('8', '9', 'a', 'b')
        AND substr(request_id, 24, 1) = '-'
    ),
    operation_kind TEXT NOT NULL CHECK (operation_kind IN ('create', 'import', 'repair', 'remove')),
    account_public_key BLOB NOT NULL CHECK (length(account_public_key) = 32),
    binding_public_key BLOB NOT NULL CHECK (length(binding_public_key) = 32),
    expected_revision INTEGER CHECK (expected_revision IS NULL OR expected_revision >= 0),
    phase TEXT NOT NULL CHECK (phase IN (
        'intent_recorded', 'credential_written', 'metadata_committed', 'selection_committed',
        'compensation_pending', 'credential_deleted', 'metadata_deleted', 'finalized'
    )),
    terminal_outcome TEXT CHECK (
        terminal_outcome IS NULL OR terminal_outcome IN ('completed', 'cancelled', 'failed')
    ),
    prior_selected_public_key BLOB CHECK (
        prior_selected_public_key IS NULL OR length(prior_selected_public_key) = 32
    ),
    prior_binding_availability TEXT CHECK (
        prior_binding_availability IS NULL OR prior_binding_availability IN (
            'available', 'credential_missing', 'store_unavailable'
        )
    ),
    resulting_revision INTEGER CHECK (resulting_revision IS NULL OR resulting_revision >= 0),
    updated_at_unix_s INTEGER NOT NULL CHECK (updated_at_unix_s >= 0),
    diagnostic_code TEXT CHECK (diagnostic_code IS NULL OR diagnostic_code IN (
        'storage_unavailable', 'keyring_unavailable', 'credential_missing',
        'compensation_failed', 'conflict', 'expired'
    )),
    CHECK (account_public_key = binding_public_key),
    CHECK (
        (phase = 'finalized' AND terminal_outcome IS NOT NULL)
        OR
        (phase <> 'finalized' AND terminal_outcome IS NULL AND resulting_revision IS NULL)
    )
) STRICT"#;

pub(crate) const CREATE_INSTALLATION_IDENTITY_SQL: &str = r#"CREATE TABLE installation_identity (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    installation_id BLOB NOT NULL CHECK (length(installation_id) = 16)
) STRICT"#;

pub(crate) const CREATE_INSTALLATION_IDENTITY_NO_UPDATE_SQL: &str = r#"CREATE TRIGGER installation_identity_no_update
BEFORE UPDATE ON installation_identity
BEGIN
    SELECT RAISE(ABORT, 'installation identity is immutable');
END"#;

pub(crate) const CREATE_INSTALLATION_IDENTITY_NO_DELETE_SQL: &str = r#"CREATE TRIGGER installation_identity_no_delete
BEFORE DELETE ON installation_identity
BEGIN
    SELECT RAISE(ABORT, 'installation identity is immutable');
END"#;

const INITIAL_SCHEMA_SQL: [&str; 9] = [
    CREATE_ACCOUNT_IDENTITIES_SQL,
    CREATE_LOCAL_SIGNER_BINDINGS_SQL,
    CREATE_RUNTIME_STATE_SQL,
    CREATE_PROFILE_CACHE_SQL,
    CREATE_ACCOUNT_PREFERENCES_SQL,
    CREATE_DURABLE_OPERATIONS_SQL,
    CREATE_INSTALLATION_IDENTITY_SQL,
    CREATE_INSTALLATION_IDENTITY_NO_UPDATE_SQL,
    CREATE_INSTALLATION_IDENTITY_NO_DELETE_SQL,
];

const OBJECT_DIGESTS: [[u8; 32]; 9] = [
    [
        203, 193, 254, 189, 121, 130, 165, 156, 1, 155, 21, 35, 130, 72, 131, 44, 34, 217, 168,
        187, 96, 155, 40, 226, 113, 78, 22, 255, 8, 65, 23, 38,
    ],
    [
        204, 249, 174, 98, 165, 65, 184, 31, 254, 31, 101, 17, 139, 176, 170, 131, 88, 225, 158, 6,
        174, 77, 192, 131, 84, 153, 142, 200, 213, 235, 141, 82,
    ],
    [
        9, 60, 207, 79, 94, 50, 51, 84, 228, 163, 119, 152, 227, 137, 166, 31, 166, 235, 70, 79,
        228, 160, 229, 246, 4, 87, 11, 172, 36, 151, 102, 234,
    ],
    [
        17, 90, 168, 107, 5, 179, 134, 52, 25, 51, 228, 255, 236, 36, 157, 152, 26, 66, 108, 147,
        239, 116, 3, 99, 82, 220, 182, 236, 209, 136, 126, 97,
    ],
    [
        180, 232, 35, 143, 72, 174, 82, 223, 52, 122, 142, 211, 5, 167, 155, 75, 69, 223, 34, 117,
        131, 5, 132, 107, 175, 198, 215, 16, 71, 114, 4, 127,
    ],
    [
        135, 198, 48, 230, 122, 86, 86, 153, 66, 95, 22, 123, 24, 164, 49, 229, 246, 218, 210, 233,
        61, 182, 81, 194, 251, 121, 165, 203, 8, 29, 63, 27,
    ],
    [
        132, 111, 227, 84, 42, 121, 244, 99, 22, 255, 131, 104, 48, 33, 7, 146, 174, 120, 176, 103,
        37, 23, 171, 90, 90, 215, 142, 212, 32, 9, 250, 188,
    ],
    [
        142, 248, 11, 168, 116, 173, 238, 101, 167, 191, 95, 63, 180, 126, 229, 156, 164, 217, 108,
        71, 221, 145, 70, 169, 91, 117, 33, 93, 34, 250, 120, 151,
    ],
    [
        127, 153, 155, 84, 191, 170, 38, 18, 239, 225, 90, 123, 208, 172, 218, 99, 2, 212, 181,
        212, 194, 19, 99, 242, 225, 249, 202, 134, 204, 219, 200, 23,
    ],
];
const VERSION_ONE_DIGEST: [u8; 32] = [
    61, 122, 56, 39, 178, 126, 179, 157, 145, 167, 19, 2, 172, 134, 213, 107, 151, 196, 212, 57,
    17, 112, 163, 67, 240, 140, 61, 62, 5, 101, 14, 71,
];

/// A sealed binding between one HarvestCircle runtime context and the v1 state catalogs.
///
/// External callers cannot forge alternate paths or catalogs:
///
/// ```compile_fail
/// use harvestcircle_storage::HarvestCircleStorageContract;
///
/// let _ = HarvestCircleStorageContract {
///     paths: todo!(),
///     migrations: todo!(),
///     schema: todo!(),
/// };
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct HarvestCircleStorageContract {
    paths: ServiceSqlitePaths,
    migrations: MigrationCatalog,
    schema: SchemaCatalog,
}

impl HarvestCircleStorageContract {
    /// Binds the exact HarvestCircle service and desktop instance to canonical paths.
    pub fn from_runtime_context(
        context: &RuntimeContext,
    ) -> Result<Self, HarvestCircleStorageContractError> {
        if context.service().as_str() != HARVESTCIRCLE_SERVICE_ID
            || context.instance().as_str() != HARVESTCIRCLE_INSTANCE_ID
        {
            return Err(HarvestCircleStorageContractError::ContextIdentity);
        }
        let paths = ServiceSqlitePaths::from_runtime_context(context)
            .map_err(|_| HarvestCircleStorageContractError::CanonicalPaths)?;
        let migrations = harvestcircle_migration_catalog()?;
        let schema = schema_catalog_for(&migrations)?;
        Ok(Self {
            paths,
            migrations,
            schema,
        })
    }

    #[must_use]
    pub const fn paths(&self) -> &ServiceSqlitePaths {
        &self.paths
    }

    #[must_use]
    pub const fn migrations(&self) -> &MigrationCatalog {
        &self.migrations
    }

    #[must_use]
    pub const fn schema(&self) -> &SchemaCatalog {
        &self.schema
    }

    #[must_use]
    pub fn application_id(&self) -> ServiceSqliteApplicationId {
        ServiceSqliteApplicationId::new(HARVESTCIRCLE_APPLICATION_ID)
            .expect("HCR1 is a valid SQLite application ID")
    }

    #[must_use]
    pub const fn state_schema_version(&self) -> NonZeroU32 {
        NonZeroU32::new(HARVESTCIRCLE_STATE_SCHEMA_VERSION)
            .expect("HarvestCircle schema v1 is nonzero")
    }
}

impl fmt::Debug for HarvestCircleStorageContract {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HarvestCircleStorageContract")
            .field("service", &HARVESTCIRCLE_SERVICE_ID)
            .field("instance", &HARVESTCIRCLE_INSTANCE_ID)
            .field("paths", &"[redacted]")
            .field("schema_version", &HARVESTCIRCLE_STATE_SCHEMA_VERSION)
            .finish()
    }
}

/// Stable, path-free storage-contract construction failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarvestCircleStorageContractError {
    ContextIdentity,
    CanonicalPaths,
    MigrationCatalog,
    SchemaCatalog,
}

impl fmt::Display for HarvestCircleStorageContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ContextIdentity => "HarvestCircle storage context identity is invalid",
            Self::CanonicalPaths => "HarvestCircle storage paths are invalid",
            Self::MigrationCatalog => "HarvestCircle migration catalog is invalid",
            Self::SchemaCatalog => "HarvestCircle schema catalog is invalid",
        })
    }
}

impl Error for HarvestCircleStorageContractError {}

#[must_use]
pub(crate) const fn harvestcircle_initial_schema_sql() -> &'static [&'static str] {
    &INITIAL_SCHEMA_SQL
}

pub fn harvestcircle_migration_catalog()
-> Result<MigrationCatalog, HarvestCircleStorageContractError> {
    MigrationCatalog::new(std::iter::empty::<MigrationDescriptor>())
        .map_err(|_| HarvestCircleStorageContractError::MigrationCatalog)
}

pub fn harvestcircle_schema_catalog() -> Result<SchemaCatalog, HarvestCircleStorageContractError> {
    let migrations = harvestcircle_migration_catalog()?;
    schema_catalog_for(&migrations)
}

fn schema_catalog_for(
    migrations: &MigrationCatalog,
) -> Result<SchemaCatalog, HarvestCircleStorageContractError> {
    let version = SchemaVersionCatalog::new(
        HARVESTCIRCLE_STATE_SCHEMA_VERSION,
        schema_objects()?,
        SchemaDigest::from_bytes(VERSION_ONE_DIGEST),
    )
    .map_err(schema_error)?;
    SchemaCatalog::new(migrations, [version]).map_err(schema_error)
}

fn schema_objects() -> Result<Vec<SchemaObject>, HarvestCircleStorageContractError> {
    let identities = [
        (
            SchemaObjectKind::Table,
            "account_identities",
            "account_identities",
        ),
        (
            SchemaObjectKind::Table,
            "local_signer_bindings",
            "local_signer_bindings",
        ),
        (SchemaObjectKind::Table, "runtime_state", "runtime_state"),
        (SchemaObjectKind::Table, "profile_cache", "profile_cache"),
        (
            SchemaObjectKind::Table,
            "account_preferences",
            "account_preferences",
        ),
        (
            SchemaObjectKind::Table,
            "durable_operations",
            "durable_operations",
        ),
        (
            SchemaObjectKind::Table,
            "installation_identity",
            "installation_identity",
        ),
        (
            SchemaObjectKind::Trigger,
            "installation_identity_no_update",
            "installation_identity",
        ),
        (
            SchemaObjectKind::Trigger,
            "installation_identity_no_delete",
            "installation_identity",
        ),
    ];
    identities
        .into_iter()
        .zip(INITIAL_SCHEMA_SQL)
        .zip(OBJECT_DIGESTS)
        .map(|(((kind, name, table), sql), digest)| {
            SchemaObject::new(kind, name, table, sql, SchemaDigest::from_bytes(digest))
                .map_err(schema_error)
        })
        .collect()
}

const fn schema_error(_: SchemaCatalogContractError) -> HarvestCircleStorageContractError {
    HarvestCircleStorageContractError::SchemaCatalog
}

#[cfg(test)]
mod tests {
    use super::*;
    use radroots_runtime_paths::{
        InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
        RadrootsPlatform, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
    };
    use sqlx::{Connection, Row};

    fn context(service: &str, instance: &str) -> RuntimeContext {
        RuntimeContext::resolve(
            &RadrootsPathResolver::new(RadrootsPlatform::Macos, RadrootsHostEnvironment::default()),
            RuntimeContextBootstrap::new(
                RadrootsPathProfile::RepoLocal,
                Some(std::path::PathBuf::from("/tmp/harvestcircle-contract")),
                RuntimeContextSource::BootstrapCli,
                RuntimeContextSource::BootstrapCli,
            )
            .expect("bootstrap"),
            ServiceId::new(service).expect("service"),
            InstanceId::new(instance).expect("instance"),
        )
        .expect("context")
    }

    #[test]
    fn exact_context_paths_and_catalogs_are_sealed() {
        let contract = HarvestCircleStorageContract::from_runtime_context(&context(
            HARVESTCIRCLE_SERVICE_ID,
            HARVESTCIRCLE_INSTANCE_ID,
        ))
        .expect("contract");
        assert!(contract.paths().state_database().ends_with("state.sqlite"));
        assert!(contract.paths().state_lock().ends_with("state.lock"));
        assert_eq!(
            contract.application_id().get(),
            HARVESTCIRCLE_APPLICATION_ID
        );
        assert_eq!(contract.state_schema_version().get(), 1);
        assert_eq!(contract.migrations().current_version(), 1);
        assert!(contract.migrations().descriptors().is_empty());
        assert_eq!(contract.schema().versions().len(), 1);
        assert_eq!(harvestcircle_initial_schema_sql().len(), 9);
    }

    #[test]
    fn context_identity_and_public_diagnostics_fail_closed() {
        for invalid in [
            context("myc", "desktop"),
            context("harvestcircle", "primary"),
        ] {
            assert_eq!(
                HarvestCircleStorageContract::from_runtime_context(&invalid),
                Err(HarvestCircleStorageContractError::ContextIdentity)
            );
        }
        for error in [
            HarvestCircleStorageContractError::ContextIdentity,
            HarvestCircleStorageContractError::CanonicalPaths,
            HarvestCircleStorageContractError::MigrationCatalog,
            HarvestCircleStorageContractError::SchemaCatalog,
        ] {
            assert!(error.source().is_none());
            assert!(!error.to_string().contains("/tmp"));
        }
        let debug = format!(
            "{:?}",
            HarvestCircleStorageContract::from_runtime_context(&context(
                HARVESTCIRCLE_SERVICE_ID,
                HARVESTCIRCLE_INSTANCE_ID,
            ))
            .unwrap()
        );
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("/tmp"));
    }

    #[test]
    fn machine_coordinates_match_the_typed_contract() {
        assert_eq!(
            harvestcircle_product::STORAGE_SERVICE_ID,
            HARVESTCIRCLE_SERVICE_ID
        );
        assert_eq!(
            harvestcircle_product::STORAGE_INSTANCE_ID,
            HARVESTCIRCLE_INSTANCE_ID
        );
        assert_eq!(
            harvestcircle_product::STORAGE_DATABASE_FILENAME,
            "state.sqlite"
        );
        assert_eq!(harvestcircle_product::STORAGE_LOCK_FILENAME, "state.lock");
        assert_eq!(
            harvestcircle_product::STORAGE_APPLICATION_ID,
            HARVESTCIRCLE_APPLICATION_ID.to_string()
        );
        assert_eq!(harvestcircle_product::STORAGE_APPLICATION_ID_TEXT, "HCR1");
        assert_eq!(
            harvestcircle_product::STORAGE_INITIAL_SCHEMA_VERSION,
            HARVESTCIRCLE_STATE_SCHEMA_VERSION.to_string()
        );
        assert_eq!(
            harvestcircle_product::LEGACY_DATABASE_FILENAME,
            "harvestcircle.sqlite3"
        );
        assert_eq!(
            harvestcircle_product::LEGACY_DATABASE_DISPOSITION,
            "untouched_and_unsupported"
        );
        assert_eq!(
            harvestcircle_product::PLATFORM_MACOS_ARCHITECTURE,
            "aarch64"
        );
        assert_eq!(harvestcircle_product::PLATFORM_LINUX_ARCHITECTURE, "x86_64");
        assert_eq!(
            harvestcircle_product::LIMIT_IDENTITIES,
            HARVESTCIRCLE_IDENTITY_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_UNFINISHED_DURABLE_OPERATIONS,
            HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_PREFERENCE_VALUE_UTF8_BYTES,
            HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_RELAY_ENDPOINTS,
            HARVESTCIRCLE_RELAY_ENDPOINT_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_RELAY_URL_BYTES,
            HARVESTCIRCLE_RELAY_URL_UTF8_BYTES.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_EVENTS_PER_RELAY,
            HARVESTCIRCLE_EVENTS_PER_RELAY_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_EVENTS_TOTAL,
            HARVESTCIRCLE_EVENTS_TOTAL_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_OBSERVERS,
            HARVESTCIRCLE_OBSERVER_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_ACTOR_MAILBOX,
            HARVESTCIRCLE_ACTOR_MAILBOX_CAPACITY.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_COMMAND_DEADLINE_MIN_MS,
            HARVESTCIRCLE_COMMAND_DEADLINE_MIN_MS.to_string()
        );
        assert_eq!(
            harvestcircle_product::LIMIT_COMMAND_DEADLINE_MAX_MS,
            HARVESTCIRCLE_COMMAND_DEADLINE_MAX_MS.to_string()
        );
        assert_eq!(
            harvestcircle_product::BACKUP_MEMBER_LIMIT,
            "caller_supplied_positive"
        );
    }

    #[tokio::test]
    async fn schema_sql_executes_as_one_fresh_strict_v1_inventory() {
        let mut connection = sqlx::SqliteConnection::connect(":memory:")
            .await
            .expect("memory database");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut connection)
            .await
            .expect("foreign keys");
        for statement in harvestcircle_initial_schema_sql() {
            sqlx::query(*statement)
                .execute(&mut connection)
                .await
                .expect("schema statement");
        }
        let rows = sqlx::query(
            "SELECT type, name, tbl_name FROM sqlite_schema \
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name LIMIT 10",
        )
        .fetch_all(&mut connection)
        .await
        .expect("inventory rows");
        let inventory = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("type"),
                    row.get::<String, _>("name"),
                    row.get::<String, _>("tbl_name"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(inventory.len(), 9);
        assert_eq!(
            inventory
                .iter()
                .filter(|(kind, _, _)| kind == "table")
                .count(),
            7
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|(kind, _, _)| kind == "trigger")
                .count(),
            2
        );
        assert!(inventory.iter().all(|(_, name, _)| {
            !matches!(
                name.as_str(),
                "application_schema" | "operation_journal" | "refinery_schema_history"
            )
        }));
    }
}
