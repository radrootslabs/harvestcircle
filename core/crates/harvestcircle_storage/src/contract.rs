//! Sealed HarvestCircle service-state identity and schema contract.

use core::{fmt, num::NonZeroU32};
use std::error::Error;

use radroots_runtime_paths::RuntimeContext;
use radroots_service_sqlite::{
    MigrationCatalog, MigrationChecksum, MigrationDescriptor, SchemaCatalog,
    SchemaCatalogContractError, SchemaDigest, SchemaObject, SchemaObjectKind, SchemaVersionCatalog,
    ServiceSqliteApplicationId, ServiceSqlitePaths,
};

pub const HARVESTCIRCLE_SERVICE_ID: &str = "harvestcircle";
pub const HARVESTCIRCLE_INSTANCE_ID: &str = "desktop";
pub const HARVESTCIRCLE_APPLICATION_ID: u32 = 0x4843_5231;
pub(crate) const HARVESTCIRCLE_INITIAL_STATE_SCHEMA_VERSION: u32 = 1;
pub const HARVESTCIRCLE_STATE_SCHEMA_VERSION: u32 = 2;
pub const HARVESTCIRCLE_IDENTITY_CAPACITY: usize = 256;
pub const HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY: usize = 1_024;
pub const HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY: usize = 4_096;
pub const HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH: usize = 256;
pub const HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;
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

const CREATE_DURABLE_OPERATIONS_V2_SQL: &str = r#"CREATE TABLE durable_operations (
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
    )), completed_at_unix_s INTEGER CHECK (
    completed_at_unix_s IS NULL OR completed_at_unix_s >= 0
),
    CHECK (account_public_key = binding_public_key),
    CHECK (
        (phase = 'finalized' AND terminal_outcome IS NOT NULL)
        OR
        (phase <> 'finalized' AND terminal_outcome IS NULL AND resulting_revision IS NULL)
    )
) STRICT"#;

const CREATE_DURABLE_OPERATIONS_RECEIPT_INSERT_GUARD_SQL: &str = r#"CREATE TRIGGER durable_operations_receipt_insert_guard
BEFORE INSERT ON durable_operations
WHEN NOT (
    (
        NEW.phase = 'finalized'
        AND NEW.terminal_outcome IS NOT NULL
        AND NEW.completed_at_unix_s IS NOT NULL
    )
    OR
    (
        NEW.phase <> 'finalized'
        AND NEW.terminal_outcome IS NULL
        AND NEW.resulting_revision IS NULL
        AND NEW.completed_at_unix_s IS NULL
    )
)
BEGIN
    SELECT RAISE(ABORT, 'durable operation receipt invariant');
END"#;

const CREATE_DURABLE_OPERATIONS_RECEIPT_UPDATE_GUARD_SQL: &str = r#"CREATE TRIGGER durable_operations_receipt_update_guard
BEFORE UPDATE ON durable_operations
WHEN NOT (
    (
        NEW.phase = 'finalized'
        AND NEW.terminal_outcome IS NOT NULL
        AND NEW.completed_at_unix_s IS NOT NULL
    )
    OR
    (
        NEW.phase <> 'finalized'
        AND NEW.terminal_outcome IS NULL
        AND NEW.resulting_revision IS NULL
        AND NEW.completed_at_unix_s IS NULL
    )
)
BEGIN
    SELECT RAISE(ABORT, 'durable operation receipt invariant');
END"#;

pub(crate) const MIGRATE_DURABLE_OPERATIONS_V2_SQL: &str = r#"ALTER TABLE durable_operations
ADD COLUMN completed_at_unix_s INTEGER CHECK (
    completed_at_unix_s IS NULL OR completed_at_unix_s >= 0
);
UPDATE durable_operations
SET completed_at_unix_s = updated_at_unix_s
WHERE phase = 'finalized';
CREATE TRIGGER durable_operations_receipt_insert_guard
BEFORE INSERT ON durable_operations
WHEN NOT (
    (
        NEW.phase = 'finalized'
        AND NEW.terminal_outcome IS NOT NULL
        AND NEW.completed_at_unix_s IS NOT NULL
    )
    OR
    (
        NEW.phase <> 'finalized'
        AND NEW.terminal_outcome IS NULL
        AND NEW.resulting_revision IS NULL
        AND NEW.completed_at_unix_s IS NULL
    )
)
BEGIN
    SELECT RAISE(ABORT, 'durable operation receipt invariant');
END;
CREATE TRIGGER durable_operations_receipt_update_guard
BEFORE UPDATE ON durable_operations
WHEN NOT (
    (
        NEW.phase = 'finalized'
        AND NEW.terminal_outcome IS NOT NULL
        AND NEW.completed_at_unix_s IS NOT NULL
    )
    OR
    (
        NEW.phase <> 'finalized'
        AND NEW.terminal_outcome IS NULL
        AND NEW.resulting_revision IS NULL
        AND NEW.completed_at_unix_s IS NULL
    )
)
BEGIN
    SELECT RAISE(ABORT, 'durable operation receipt invariant');
END;
UPDATE durable_operations SET request_id = request_id"#;

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
const DURABLE_OPERATIONS_V2_DIGEST: [u8; 32] = [
    162, 24, 8, 218, 50, 32, 224, 252, 60, 229, 209, 160, 204, 46, 226, 37, 88, 102, 22, 2, 156,
    177, 28, 19, 235, 161, 26, 125, 65, 126, 59, 135,
];
const DURABLE_OPERATIONS_RECEIPT_INSERT_GUARD_DIGEST: [u8; 32] = [
    130, 178, 229, 235, 158, 182, 51, 151, 133, 196, 115, 85, 8, 166, 169, 136, 62, 26, 211, 92,
    215, 57, 254, 158, 103, 196, 53, 39, 106, 201, 183, 14,
];
const DURABLE_OPERATIONS_RECEIPT_UPDATE_GUARD_DIGEST: [u8; 32] = [
    48, 131, 220, 252, 243, 158, 221, 88, 8, 140, 207, 187, 34, 138, 145, 215, 92, 99, 32, 72, 254,
    25, 96, 241, 33, 172, 150, 186, 98, 114, 27, 142,
];
const VERSION_TWO_DIGEST: [u8; 32] = [
    78, 151, 73, 238, 2, 15, 71, 52, 11, 111, 100, 95, 135, 11, 170, 138, 84, 105, 106, 177, 27, 7,
    134, 156, 53, 68, 22, 220, 116, 199, 147, 5,
];
const DURABLE_OPERATIONS_V2_MIGRATION_CHECKSUM: MigrationChecksum =
    MigrationChecksum::from_bytes([
        107, 95, 237, 250, 255, 0, 44, 110, 142, 194, 92, 163, 84, 27, 96, 31, 210, 37, 151, 186,
        210, 83, 137, 114, 251, 20, 30, 31, 11, 136, 207, 168,
    ]);

/// A sealed binding between one HarvestCircle runtime context and the governed state catalogs.
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
            .expect("HarvestCircle current schema version is nonzero")
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
    let migration = MigrationDescriptor::sql(
        2,
        "bound_durable_operation_receipts",
        MIGRATE_DURABLE_OPERATIONS_V2_SQL,
        DURABLE_OPERATIONS_V2_MIGRATION_CHECKSUM,
    )
    .map_err(|_| HarvestCircleStorageContractError::MigrationCatalog)?;
    MigrationCatalog::new([migration])
        .map_err(|_| HarvestCircleStorageContractError::MigrationCatalog)
}

pub fn harvestcircle_schema_catalog() -> Result<SchemaCatalog, HarvestCircleStorageContractError> {
    let migrations = harvestcircle_migration_catalog()?;
    schema_catalog_for(&migrations)
}

fn schema_catalog_for(
    migrations: &MigrationCatalog,
) -> Result<SchemaCatalog, HarvestCircleStorageContractError> {
    let version_one = SchemaVersionCatalog::new(
        HARVESTCIRCLE_INITIAL_STATE_SCHEMA_VERSION,
        schema_objects(CREATE_DURABLE_OPERATIONS_SQL)?,
        SchemaDigest::from_bytes(VERSION_ONE_DIGEST),
    )
    .map_err(schema_error)?;
    let version_two_objects = schema_objects(CREATE_DURABLE_OPERATIONS_V2_SQL)?;
    let version_two = SchemaVersionCatalog::new(
        HARVESTCIRCLE_STATE_SCHEMA_VERSION,
        version_two_objects,
        SchemaDigest::from_bytes(VERSION_TWO_DIGEST),
    )
    .map_err(schema_error)?;
    SchemaCatalog::new(migrations, [version_one, version_two]).map_err(schema_error)
}

fn schema_objects(
    durable_operations_sql: &'static str,
) -> Result<Vec<SchemaObject>, HarvestCircleStorageContractError> {
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
    let schema_sql = [
        CREATE_ACCOUNT_IDENTITIES_SQL,
        CREATE_LOCAL_SIGNER_BINDINGS_SQL,
        CREATE_RUNTIME_STATE_SQL,
        CREATE_PROFILE_CACHE_SQL,
        CREATE_ACCOUNT_PREFERENCES_SQL,
        durable_operations_sql,
        CREATE_INSTALLATION_IDENTITY_SQL,
        CREATE_INSTALLATION_IDENTITY_NO_UPDATE_SQL,
        CREATE_INSTALLATION_IDENTITY_NO_DELETE_SQL,
    ];
    let mut objects = identities
        .into_iter()
        .zip(schema_sql)
        .zip(OBJECT_DIGESTS)
        .map(|(((kind, name, table), sql), digest)| {
            let digest = if sql == CREATE_DURABLE_OPERATIONS_V2_SQL {
                SchemaDigest::from_bytes(DURABLE_OPERATIONS_V2_DIGEST)
            } else {
                SchemaDigest::from_bytes(digest)
            };
            SchemaObject::new(kind, name, table, sql, digest).map_err(schema_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if durable_operations_sql == CREATE_DURABLE_OPERATIONS_V2_SQL {
        for (name, sql, digest) in [
            (
                "durable_operations_receipt_insert_guard",
                CREATE_DURABLE_OPERATIONS_RECEIPT_INSERT_GUARD_SQL,
                DURABLE_OPERATIONS_RECEIPT_INSERT_GUARD_DIGEST,
            ),
            (
                "durable_operations_receipt_update_guard",
                CREATE_DURABLE_OPERATIONS_RECEIPT_UPDATE_GUARD_SQL,
                DURABLE_OPERATIONS_RECEIPT_UPDATE_GUARD_DIGEST,
            ),
        ] {
            objects.push(
                SchemaObject::new(
                    SchemaObjectKind::Trigger,
                    name,
                    "durable_operations",
                    sql,
                    SchemaDigest::from_bytes(digest),
                )
                .map_err(schema_error)?,
            );
        }
    }
    Ok(objects)
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
        assert_eq!(contract.state_schema_version().get(), 2);
        assert_eq!(contract.migrations().current_version(), 2);
        assert_eq!(contract.migrations().descriptors().len(), 1);
        assert_eq!(contract.schema().versions().len(), 2);
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
            HARVESTCIRCLE_INITIAL_STATE_SCHEMA_VERSION.to_string()
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
        sqlx::query("PRAGMA trusted_schema = OFF")
            .execute(&mut connection)
            .await
            .expect("trusted schema");
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

    #[tokio::test]
    async fn migration_v2_adds_the_exact_bounded_journal_schema() {
        let mut connection = sqlx::SqliteConnection::connect(":memory:")
            .await
            .expect("memory database");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut connection)
            .await
            .expect("foreign keys");
        sqlx::query("PRAGMA trusted_schema = OFF")
            .execute(&mut connection)
            .await
            .expect("trusted schema");
        for statement in harvestcircle_initial_schema_sql() {
            sqlx::query(*statement)
                .execute(&mut connection)
                .await
                .expect("schema statement");
        }
        let identity = [7_u8; 32];
        sqlx::query(
            "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
             binding_public_key, phase, terminal_outcome, updated_at_unix_s) \
             VALUES ('01890f3e-7b1c-7000-8000-000000000001', 'create', ?, ?, \
                     'finalized', 'completed', 10)",
        )
        .bind(identity.as_slice())
        .bind(identity.as_slice())
        .execute(&mut connection)
        .await
        .expect("terminal v1 row");
        sqlx::query(
            "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
             binding_public_key, phase, updated_at_unix_s) \
             VALUES ('01890f3e-7b1c-7000-8000-000000000002', 'remove', ?, ?, \
                     'intent_recorded', 11)",
        )
        .bind(identity.as_slice())
        .bind(identity.as_slice())
        .execute(&mut connection)
        .await
        .expect("unfinished v1 row");
        sqlx::raw_sql(MIGRATE_DURABLE_OPERATIONS_V2_SQL)
            .execute(&mut connection)
            .await
            .expect("migration");
        let actual: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'durable_operations'",
        )
        .fetch_one(&mut connection)
        .await
        .expect("schema SQL");
        assert_eq!(actual, CREATE_DURABLE_OPERATIONS_V2_SQL);
        let rows = sqlx::query(
            "SELECT request_id, completed_at_unix_s FROM durable_operations ORDER BY request_id",
        )
        .fetch_all(&mut connection)
        .await
        .expect("migrated rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get::<Option<i64>, _>("completed_at_unix_s"),
            Some(10)
        );
        assert_eq!(rows[1].get::<Option<i64>, _>("completed_at_unix_s"), None);
        assert!(
            sqlx::query(
                "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
                 binding_public_key, phase, terminal_outcome, updated_at_unix_s) \
                 VALUES ('01890f3e-7b1c-7000-8000-000000000004', 'create', ?, ?, \
                         'finalized', 'completed', 13)",
            )
            .bind(identity.as_slice())
            .bind(identity.as_slice())
            .execute(&mut connection)
            .await
            .is_err(),
            "insert guard must require terminal completion time"
        );
        assert!(
            sqlx::query(
                "UPDATE durable_operations SET phase = 'finalized', \
                 terminal_outcome = 'completed' WHERE request_id = \
                 '01890f3e-7b1c-7000-8000-000000000002'",
            )
            .execute(&mut connection)
            .await
            .is_err(),
            "update guard must require terminal completion time"
        );

        let migration = MigrationChecksum::for_sql(MIGRATE_DURABLE_OPERATIONS_V2_SQL);
        let objects = schema_objects(CREATE_DURABLE_OPERATIONS_V2_SQL).expect("objects");
        let object = objects
            .iter()
            .find(|object| object.name() == "durable_operations")
            .expect("durable operations")
            .digest();
        let snapshot = SchemaVersionCatalog::computed_digest(2, objects).expect("snapshot");
        assert_eq!(migration, DURABLE_OPERATIONS_V2_MIGRATION_CHECKSUM);
        assert_eq!(
            object,
            SchemaDigest::from_bytes(DURABLE_OPERATIONS_V2_DIGEST)
        );
        assert_eq!(snapshot, SchemaDigest::from_bytes(VERSION_TWO_DIGEST));
    }

    #[tokio::test]
    async fn migration_v2_rolls_back_without_partial_schema_on_invalid_v1_state() {
        let mut connection = sqlx::SqliteConnection::connect(":memory:")
            .await
            .expect("memory database");
        for statement in harvestcircle_initial_schema_sql() {
            sqlx::query(*statement)
                .execute(&mut connection)
                .await
                .expect("schema statement");
        }
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await
            .expect("fixture policy");
        let identity = [9_u8; 32];
        sqlx::query(
            "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
             binding_public_key, phase, updated_at_unix_s) \
             VALUES ('01890f3e-7b1c-7000-8000-000000000003', 'create', ?, ?, 'finalized', 12)",
        )
        .bind(identity.as_slice())
        .bind(identity.as_slice())
        .execute(&mut connection)
        .await
        .expect("invalid v1 fixture");
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut connection)
            .await
            .expect("restore policy");

        let mut transaction = connection.begin().await.expect("migration transaction");
        assert!(
            sqlx::raw_sql(MIGRATE_DURABLE_OPERATIONS_V2_SQL)
                .execute(&mut *transaction)
                .await
                .is_err()
        );
        transaction.rollback().await.expect("rollback");

        let columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info('durable_operations') \
             WHERE name = 'completed_at_unix_s'",
        )
        .fetch_one(&mut connection)
        .await
        .expect("column inventory");
        let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_operations")
            .fetch_one(&mut connection)
            .await
            .expect("retained row");
        assert_eq!(columns, 0);
        assert_eq!(retained, 1);
    }
}
