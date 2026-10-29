#![doc = "HarvestCircle persistence adapters."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod availability_evidence;
mod backup;
mod contract;
mod db;
mod hub;
mod identities;
mod identity_namespace;
mod installation;
mod journal;
// The operating-system credential store requires an explicit, ignored host smoke test.
#[cfg_attr(coverage_nightly, coverage(off))]
mod os_keyring;
mod profiles;

pub use backup::{VerifiedHarvestCircleBackup, verify_harvestcircle_backup};
pub use contract::{
    HARVESTCIRCLE_ACTOR_MAILBOX_CAPACITY, HARVESTCIRCLE_APPLICATION_ID,
    HARVESTCIRCLE_COMMAND_DEADLINE_MAX_MS, HARVESTCIRCLE_COMMAND_DEADLINE_MIN_MS,
    HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY, HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH,
    HARVESTCIRCLE_EVENTS_PER_RELAY_CAPACITY, HARVESTCIRCLE_EVENTS_TOTAL_CAPACITY,
    HARVESTCIRCLE_IDENTITY_CAPACITY, HARVESTCIRCLE_INSTANCE_ID, HARVESTCIRCLE_OBSERVER_CAPACITY,
    HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES, HARVESTCIRCLE_RELAY_ENDPOINT_CAPACITY,
    HARVESTCIRCLE_RELAY_URL_UTF8_BYTES, HARVESTCIRCLE_SERVICE_ID,
    HARVESTCIRCLE_STATE_SCHEMA_VERSION, HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS,
    HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY, HarvestCircleStorageContract,
    HarvestCircleStorageContractError, harvestcircle_migration_catalog,
    harvestcircle_schema_catalog,
};
pub use db::{CURRENT_SCHEMA_VERSION, Database};
pub use hub::handling::departure::{
    HubDepartureReceipt, HubDepartureReview, HubDepartureReviewInput, HubDepartureUse,
};
pub use hub::handling::dispatch::{
    HubDispatchFilter, HubDispatchKind, HubDispatchPage, HubDispatchQuery, HubDispatchReceipt,
    HubDispatchRow, HubDispatchState, HubDispatchView,
};
pub use hub::handling::trace::{
    HubTraceHistoryEntry, HubTraceHistoryKind, HubTraceHistoryPage, HubTraceHistoryQuery,
    HubTracePage, HubTraceQuery, HubTraceRow, HubTraceSnapshotQuery,
};
pub use hub::handling::{
    HubActionGroup, HubDiscrepancyRow, HubGroupCreationKind, HubGroupView, HubHandlingKind,
    HubHandlingPage, HubHandlingQuery, HubHandlingReceipt, HubHandlingResult, HubHoldRow,
    HubLineagePage, HubLineageQuery, HubLineageRow, HubMovementResult, HubSplitResult,
    HubTreatmentPage, HubTreatmentQuery, HubTreatmentResult, HubTreatmentScope,
    HubTreatmentSnapshot, HubWorkActionResult, HubWorkChange, HubWorkOrigin, HubWorkRow,
};
pub use hub::reference::HubReferenceReceipt;
pub use hub::stock::{
    HubOpeningReceipt, HubStockPage, HubStockQuery, HubStockReceipt, HubStockRow, HubStockWork,
};
pub use os_keyring::{CREDENTIAL_SERVICE, OsKeyringSecretStore};
