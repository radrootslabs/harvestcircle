#![doc = "Radroots Studio persistence adapters."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod account_namespace;
pub mod accounts;
mod compatibility;
pub mod db;
mod installation;
pub mod journal;
// The operating-system credential store requires an explicit, ignored host smoke test.
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod os_keyring;
pub mod profiles;
mod recovery;
mod repair;

pub use compatibility::{DatabasePreflight, PersistedIdentityIssue, PersistedIdentityIssueKind};
pub use db::{CURRENT_SCHEMA_VERSION, Database};
pub use os_keyring::{CREDENTIAL_SERVICE, OsKeyringSecretStore};
pub use repair::{QuarantineExportReceipt, RepairAuthorization, RepairCandidate};
