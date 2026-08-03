#![doc = "Radroots Studio persistence adapters."]

pub mod account_namespace;
pub mod accounts;
pub mod application_adapter;
pub mod db;
pub mod journal;
pub mod os_keyring;
pub mod profiles;
pub mod runtime_actor;

pub use application_adapter::PersistentAppCore;
pub use db::{CURRENT_SCHEMA_VERSION, Database};
pub use os_keyring::{CREDENTIAL_SERVICE, OsKeyringSecretStore};
pub use runtime_actor::RuntimeActorHandle;
