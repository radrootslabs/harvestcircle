//! Distinct returned custody and observed disposition; predecessor originals stay frozen.
pub(in crate::hub) mod returns;
pub(crate) mod schema;
pub use returns::HubReturnReceipt;
#[cfg(test)]
mod schema_tests;

pub(in crate::hub) mod disposition;
pub use disposition::HubDispositionReceipt;

#[cfg(test)]
pub(in crate::hub) mod tests;

#[cfg(test)]
mod upgrade_tests;
