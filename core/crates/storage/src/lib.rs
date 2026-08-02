#![doc = "Radroots Studio persistence adapters."]

pub mod account_namespace;
pub mod accounts;
pub mod application_adapter;
pub mod db;
pub mod journal;
pub mod profiles;

pub use application_adapter::PersistentAppCore;
pub use db::Database;
