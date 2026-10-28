//! Local hub state has no foreign key to removable account or credential rows.
pub(crate) mod reference;
pub(crate) mod stock;
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::{HubId, HubOwnership, HubRevision};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};
use radroots_service_sqlite::{SchemaObjectKind, ServiceSqliteTransaction};
use sqlx::Row;

use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};

const CONTEXT_SQL: &str = "CREATE TABLE hub_context (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    hub_id BLOB NOT NULL UNIQUE CHECK (length(hub_id) = 16 AND hub_id <> zeroblob(16)),
    owner_public_key BLOB NOT NULL CHECK (length(owner_public_key) = 32),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    initialization_id TEXT NOT NULL UNIQUE CHECK (length(initialization_id) = 36),
    created_at_unix_s INTEGER NOT NULL CHECK (created_at_unix_s >= 0)
) STRICT";
const HISTORY_SQL: &str = "CREATE TABLE hub_history (
    sequence INTEGER NOT NULL PRIMARY KEY CHECK (sequence >= 1),
    hub_id BLOB NOT NULL CHECK (length(hub_id) = 16),
    actor_public_key BLOB NOT NULL CHECK (length(actor_public_key) = 32),
    event_kind TEXT NOT NULL CHECK (event_kind IN ('initialized', 'ownership_transferred')),
    recorded_at_unix_s INTEGER NOT NULL CHECK (recorded_at_unix_s >= 0),
    owner_public_key BLOB NOT NULL CHECK (length(owner_public_key) = 32),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) = 36)
) STRICT";

const HISTORY_NO_UPDATE_SQL: &str = "CREATE TRIGGER hub_history_no_update BEFORE UPDATE ON hub_history BEGIN SELECT RAISE(ABORT, 'hub history is immutable'); END";
const HISTORY_NO_DELETE_SQL: &str = "CREATE TRIGGER hub_history_no_delete BEFORE DELETE ON hub_history BEGIN SELECT RAISE(ABORT, 'hub history is immutable'); END";

// Frozen migration 4. Evolve through another migration, never edit published SQL.
pub(crate) const MIGRATE_HUB_V4_SQL: &str = concat!(
    "CREATE TABLE hub_context (\n",
    "    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),\n",
    "    hub_id BLOB NOT NULL UNIQUE CHECK (length(hub_id) = 16 AND hub_id <> zeroblob(16)),\n",
    "    owner_public_key BLOB NOT NULL CHECK (length(owner_public_key) = 32),\n",
    "    revision INTEGER NOT NULL CHECK (revision >= 1),\n",
    "    initialization_id TEXT NOT NULL UNIQUE CHECK (length(initialization_id) = 36),\n",
    "    created_at_unix_s INTEGER NOT NULL CHECK (created_at_unix_s >= 0)\n",
    ") STRICT;\n",
    "CREATE TABLE hub_history (\n",
    "    sequence INTEGER NOT NULL PRIMARY KEY CHECK (sequence >= 1),\n",
    "    hub_id BLOB NOT NULL CHECK (length(hub_id) = 16),\n",
    "    actor_public_key BLOB NOT NULL CHECK (length(actor_public_key) = 32),\n",
    "    event_kind TEXT NOT NULL CHECK (event_kind IN ('initialized', 'ownership_transferred')),\n",
    "    recorded_at_unix_s INTEGER NOT NULL CHECK (recorded_at_unix_s >= 0),\n",
    "    owner_public_key BLOB NOT NULL CHECK (length(owner_public_key) = 32),\n",
    "    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) = 36)\n",
    ") STRICT;\n",
    "CREATE TRIGGER hub_history_no_update BEFORE UPDATE ON hub_history BEGIN SELECT RAISE(ABORT, 'hub history is immutable'); END;\n",
    "CREATE TRIGGER hub_history_no_delete BEFORE DELETE ON hub_history BEGIN SELECT RAISE(ABORT, 'hub history is immutable'); END"
);
pub(crate) const MIGRATION_CHECKSUM: radroots_service_sqlite::MigrationChecksum =
    radroots_service_sqlite::MigrationChecksum::from_bytes([
        49, 9, 1, 26, 69, 129, 76, 216, 153, 48, 0, 52, 72, 229, 82, 239, 8, 219, 188, 19, 243,
        137, 112, 121, 38, 97, 107, 240, 131, 115, 68, 138,
    ]);

pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    253, 33, 160, 115, 203, 243, 145, 135, 45, 153, 149, 144, 254, 59, 224, 119, 214, 67, 232, 65,
    108, 150, 71, 185, 171, 134, 16, 145, 109, 45, 166, 206,
];

pub(crate) const SCHEMA_OBJECTS: [(SchemaObjectKind, &str, &str, &str); 4] = [
    (
        SchemaObjectKind::Table,
        "hub_context",
        "hub_context",
        CONTEXT_SQL,
    ),
    (
        SchemaObjectKind::Table,
        "hub_history",
        "hub_history",
        HISTORY_SQL,
    ),
    (
        SchemaObjectKind::Trigger,
        "hub_history_no_update",
        "hub_history",
        HISTORY_NO_UPDATE_SQL,
    ),
    (
        SchemaObjectKind::Trigger,
        "hub_history_no_delete",
        "hub_history",
        HISTORY_NO_DELETE_SQL,
    ),
];

impl Database {
    /// Trusted native adapter read. Public clients must pass current authority.
    pub async fn load_hub(&self) -> Result<Option<HubOwnership>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    load_on(transaction)
                        .await
                        .map(|value| value.map(|(hub, _)| hub))
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    /// Atomically initializes one hub and its original actor/history receipt.
    /// The runtime verifies the active credential before entering this adapter.
    pub async fn initialize_hub(
        &self,
        candidate: HubOwnership,
        operation: &DurableRequestId,
        now: UnixTimestamp,
    ) -> Result<HubOwnership, SafeError> {
        let operation = operation.as_str().to_owned();
        self.host().transaction(|transaction| Box::pin(async move {
            if let Some((existing, original)) = load_on(transaction).await? {
                if original == operation && existing == candidate {
                    reference::validate_basis_on(transaction, existing.id()).await?;
                    return Ok(existing);
                }
                return Err(hub_conflict());
            }
            sqlx::query("INSERT INTO hub_context (singleton, hub_id, owner_public_key, revision, initialization_id, created_at_unix_s) VALUES (1, ?, ?, 1, ?, ?)")
                .bind(candidate.id().as_bytes().as_slice())
                .bind(candidate.owner().as_bytes().as_slice())
                .bind(&operation).bind(now.as_seconds())
                .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            sqlx::query("INSERT INTO hub_stock_state (singleton, hub_id, revision, event_count) VALUES (1, ?, 1, 0)")
                .bind(candidate.id().as_bytes().as_slice()).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            sqlx::query("INSERT INTO hub_handling_context (singleton, hub_id, basis_id, basis_version, site_timezone) VALUES (1, ?, 'whole-pack-demo-v1', 1, 'UTC')")
                .bind(candidate.id().as_bytes().as_slice()).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            sqlx::query("INSERT INTO hub_history (sequence, hub_id, actor_public_key, event_kind, recorded_at_unix_s, owner_public_key, operation_id) VALUES (1, ?, ?, 'initialized', ?, ?, ?)")
                .bind(candidate.id().as_bytes().as_slice())
                .bind(candidate.owner().as_bytes().as_slice())
                .bind(now.as_seconds()).bind(candidate.owner().as_bytes().as_slice()).bind(&operation)
                .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            Ok(candidate)
        })).await.map_err(map_transaction_error)
    }
}

pub(crate) async fn load_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
) -> Result<Option<(HubOwnership, String)>, SafeError> {
    let rows = sqlx::query("SELECT substr(hub_id, 1, 17) AS hub_id, length(hub_id) AS hub_id_bytes, substr(owner_public_key, 1, 33) AS owner_public_key, length(owner_public_key) AS owner_bytes, revision, substr(initialization_id, 1, 37) AS initialization_id FROM hub_context WHERE singleton = 1 LIMIT 2")
        .fetch_all(&mut *transaction).await.map_err(|_| corrupt_storage())?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if rows.len() != 1
        || row
            .try_get::<i64, _>("hub_id_bytes")
            .map_err(|_| corrupt_storage())?
            != 16
        || row
            .try_get::<i64, _>("owner_bytes")
            .map_err(|_| corrupt_storage())?
            != 32
    {
        return Err(corrupt_storage());
    }
    let id: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
    let owner: Vec<u8> = row
        .try_get("owner_public_key")
        .map_err(|_| corrupt_storage())?;
    let revision: i64 = row.try_get("revision").map_err(|_| corrupt_storage())?;
    let operation: String = row
        .try_get("initialization_id")
        .map_err(|_| corrupt_storage())?;
    let id = HubId::from_bytes(id.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let owner = PublicKey::from_bytes(owner.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let revision = HubRevision::from_value(u64::try_from(revision).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    DurableRequestId::parse(&operation).map_err(|_| corrupt_storage())?;
    Ok(Some((
        HubOwnership::restore(id, owner, revision),
        operation,
    )))
}

pub(crate) const fn hub_conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The hub context changed. Review the current hub before continuing."),
    )
}
