//! Explicit count-aware capture. Native mode selection/completeness and fresh
//! admission remain outside this descriptive, write-only representation.
use super::*;
use crate::hub::corrections::{
    CountLossQuantityAccount, DepartureQuantityAccount, OriginalRecipientAttribution,
    RecipientAttributionCorrection,
};
use crate::hub::counts::CountPostPlan;
use serde::{Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const ACTUAL_KNOWN_COUNT_INVENTORY_SCHEMA: u8 = 2;

/// Checked immutable inventory with canonical shared accounts. There is no
/// decoder or grant constructor; a native producer must select this version
/// from immutable mode/marker evidence, including when the inventory is empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualKnownCountInventory {
    facts: ActualKnownInterpretations,
}
impl ActualKnownCountInventory {
    pub fn new(mut facts: ActualKnownInterpretations) -> Result<Self, ActualReportError> {
        super::interpretation::validate_inventory(&facts, None)?;
        let mut accounts = BTreeMap::new();
        let mut selected: BTreeMap<HubRecordId, BTreeSet<HubRecordId>> = BTreeMap::new();
        for record in &mut facts.records {
            if let Some(value) = &record.count_explanation {
                let shared = accounts
                    .entry(value.count.operation)
                    .or_insert_with(|| Arc::clone(value));
                selected
                    .entry(value.count.operation)
                    .or_default()
                    .insert(record.original.operation);
                record.count_explanation = Some(Arc::clone(shared));
            }
        }
        // A complete supplied account cannot silently lose a zero or earlier
        // explanation record. Entire missing native accounts still require the
        // independent canonical native completeness proof.
        for (operation, account) in accounts {
            let members = selected
                .get(&operation)
                .ok_or(ActualReportError::InvalidHistory)?;
            if members.len() != account.account.explanations().len()
                || account
                    .account
                    .explanations()
                    .iter()
                    .any(|row| !members.contains(&row.operation))
            {
                return Err(ActualReportError::InvalidHistory);
            }
        }
        Ok(Self { facts })
    }
    #[must_use]
    pub const fn facts(&self) -> &ActualKnownInterpretations {
        &self.facts
    }

    /// Purpose-separated field consistency, never a native completeness proof.
    /// Each shared account is hashed once, then each selected event/reference.
    pub fn digest(&self) -> Result<[u8; 32], ActualReportError> {
        super::result::count_inventory_digest(&self.facts)
    }
}

#[derive(Serialize)]
struct CountEntry<'a> {
    operation: HubRecordId,
    count: &'a CountPostPlan,
    account: &'a CountLossQuantityAccount,
}
#[derive(Serialize)]
struct Record<'a> {
    stock_revision: HubRevision,
    catalog_revision: HubRevision,
    latest_recorded_at: UnixTimestamp,
    original: &'a ActualKnownDeparture,
    recipient_original: &'a OriginalRecipientAttribution,
    recipient_history: &'a [RecipientAttributionCorrection],
    quantities: &'a [DepartureQuantityAccount],
    #[serde(skip_serializing_if = "Option::is_none")]
    count_account: Option<HubRecordId>,
}
impl Serialize for ActualKnownCountInventory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Envelope<'a> {
            schema: u8,
            hub: HubId,
            stock_revision: HubRevision,
            catalog_revision: HubRevision,
            latest_recorded_at: UnixTimestamp,
            count_accounts: Vec<CountEntry<'a>>,
            records: Vec<Record<'a>>,
        }
        let mut accounts = BTreeMap::new();
        let records = self
            .facts
            .records
            .iter()
            .map(|record| {
                let count_account = record.count_explanation.as_deref().map(|value| {
                    accounts.entry(value.count.operation).or_insert(value);
                    value.count.operation
                });
                Record {
                    stock_revision: record.stock_revision,
                    catalog_revision: record.catalog_revision,
                    latest_recorded_at: record.latest_recorded_at,
                    original: &record.original,
                    recipient_original: &record.recipient_original,
                    recipient_history: &record.recipient_history,
                    quantities: &record.quantities,
                    count_account,
                }
            })
            .collect();
        let count_accounts = accounts
            .into_iter()
            .map(|(operation, value)| CountEntry {
                operation,
                count: &value.count,
                account: &value.account,
            })
            .collect();
        Envelope {
            schema: ACTUAL_KNOWN_COUNT_INVENTORY_SCHEMA,
            hub: self.facts.hub,
            stock_revision: self.facts.stock_revision,
            catalog_revision: self.facts.catalog_revision,
            latest_recorded_at: self.facts.latest_recorded_at,
            count_accounts,
            records,
        }
        .serialize(serializer)
    }
}
