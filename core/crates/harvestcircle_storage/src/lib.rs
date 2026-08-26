#![doc = "HarvestCircle persistence adapters."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod compatibility;
mod contract;
pub mod db;
pub mod identities;
pub mod identity_namespace;
mod installation;
pub mod journal;
// The operating-system credential store requires an explicit, ignored host smoke test.
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod os_keyring;
pub mod profiles;
mod recovery;
mod repair;

pub use compatibility::{DatabasePreflight, PersistedIdentityIssue, PersistedIdentityIssueKind};
pub use contract::{
    HARVESTCIRCLE_ACTOR_MAILBOX_CAPACITY, HARVESTCIRCLE_APPLICATION_ID,
    HARVESTCIRCLE_COMMAND_DEADLINE_MAX_MS, HARVESTCIRCLE_COMMAND_DEADLINE_MIN_MS,
    HARVESTCIRCLE_EVENTS_PER_RELAY_CAPACITY, HARVESTCIRCLE_EVENTS_TOTAL_CAPACITY,
    HARVESTCIRCLE_IDENTITY_CAPACITY, HARVESTCIRCLE_INSTANCE_ID, HARVESTCIRCLE_OBSERVER_CAPACITY,
    HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES, HARVESTCIRCLE_RELAY_ENDPOINT_CAPACITY,
    HARVESTCIRCLE_RELAY_URL_UTF8_BYTES, HARVESTCIRCLE_SERVICE_ID,
    HARVESTCIRCLE_STATE_SCHEMA_VERSION, HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY,
    HarvestCircleStorageContract, HarvestCircleStorageContractError,
    harvestcircle_initial_schema_sql, harvestcircle_migration_catalog,
    harvestcircle_schema_catalog,
};
pub use db::{CURRENT_SCHEMA_VERSION, Database};
pub use os_keyring::{CREDENTIAL_SERVICE, OsKeyringSecretStore};
pub use repair::{QuarantineExportReceipt, RepairAuthorization, RepairCandidate};
