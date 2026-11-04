//! Immutable original proofs belong to one read phase of one native transaction.
//! No owner, custody, current permission, pending intent or admission is cached.
use super::{model::*, reviews};
use crate::{db::corrupt_storage, hub::stock};
use harvestcircle_domain::{SafeError, hub::HubId};
use radroots_service_sqlite::ServiceSqliteTransaction;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Write},
    ops::{Deref, DerefMut},
    sync::Arc,
};

const ENTRIES: usize = harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY;
const PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[cfg(test)]
mod tests;

/// Includes the complete canonical operation identity, not caller-selected cuts.
/// New native interpretation modes must additionally bind their canonical epoch.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    hub: [u8; 16],
    operation: String,
    actor: [u8; 32],
    intent: [u8; 32],
    stock: u64,
    catalog: u64,
    result: [u8; 16],
    recorded: i64,
    schema: u32,
}
impl Key {
    fn canonical(hub: HubId, op: &stock::read::OperationRecord) -> Self {
        Self {
            hub: *hub.as_bytes(),
            operation: op.id.as_str().to_owned(),
            actor: *op.actor.as_bytes(),
            intent: op.hash,
            stock: op.revision.value(),
            catalog: op.catalog_revision.value(),
            result: *op.result.as_bytes(),
            recorded: op.recorded_at.as_seconds(),
            schema: crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION,
        }
    }
}

struct Entry {
    receipt: Arc<HubActualReviewReceipt>,
    charge: usize,
}

struct Proofs {
    completed: BTreeMap<Key, Entry>,
    order: VecDeque<Key>,
    active: Vec<Key>,
    bytes: usize,
    entry_limit: usize,
    payload_limit: usize,
    #[cfg(test)]
    rebuilt: usize,
    #[cfg(test)]
    hits: usize,
}
impl Default for Proofs {
    fn default() -> Self {
        Self {
            completed: BTreeMap::new(),
            order: VecDeque::new(),
            active: Vec::new(),
            bytes: 0,
            entry_limit: ENTRIES,
            payload_limit: PAYLOAD_BYTES,
            #[cfg(test)]
            rebuilt: 0,
            #[cfg(test)]
            hits: 0,
        }
    }
}

/// The proof store cannot be supplied independently of its transaction borrow.
/// Construct only for a read phase; drop before any writer mutation and create
/// a new reader for readback. Nested full-original readers share this owner.
pub(in crate::hub) struct OriginalReader<'borrow, 'connection> {
    tx: &'borrow mut ServiceSqliteTransaction<'connection>,
    proofs: Proofs,
}
impl<'borrow, 'connection> OriginalReader<'borrow, 'connection> {
    pub(in crate::hub) fn new(tx: &'borrow mut ServiceSqliteTransaction<'connection>) -> Self {
        Self {
            tx,
            proofs: Proofs::default(),
        }
    }

    #[cfg(test)]
    pub(in crate::hub) fn for_test_budget(
        tx: &'borrow mut ServiceSqliteTransaction<'connection>,
        entries: usize,
        bytes: usize,
    ) -> Self {
        let mut reader = Self::new(tx);
        reader.proofs.entry_limit = entries;
        reader.proofs.payload_limit = bytes;
        reader
    }

    #[cfg(test)]
    pub(in crate::hub) fn proof_counts(&self) -> (usize, usize, usize, usize) {
        (
            self.proofs.rebuilt,
            self.proofs.hits,
            self.proofs.completed.len(),
            self.proofs.bytes,
        )
    }

    pub(in crate::hub) async fn actual_review(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubActualReviewReceipt>, SafeError> {
        // Re-read the canonical native header before selecting a cached proof.
        let op = stock::read::operation_on(self.tx, hub, &supplied.id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let key = Key::canonical(hub, &op);
        if op.kind != "actual_review"
            || supplied.kind != op.kind
            || Key::canonical(hub, &supplied) != key
        {
            return Err(corrupt_storage());
        }
        // Check the active edge BEFORE a hit: a previously verified receipt
        // cannot hide a cyclic, self or forward full-original dependency.
        if self
            .proofs
            .active
            .last()
            .is_some_and(|parent| parent.hub != key.hub || key.stock >= parent.stock)
            || self.proofs.active.len() >= ENTRIES
        {
            return Err(corrupt_storage());
        }
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return Ok(Arc::clone(&entry.receipt));
        }
        self.proofs.active.push(key.clone());
        #[cfg(test)]
        {
            self.proofs.rebuilt += 1;
        }
        let result = Box::pin(reviews::rebuild_on(self, hub, op)).await;
        if self.proofs.active.pop().as_ref() != Some(&key) {
            return Err(corrupt_storage());
        }
        let receipt = Arc::new(result?);
        self.proofs.retain(key, Arc::clone(&receipt));
        Ok(receipt)
    }
}

// Shallow readers continue through the pinned provider's protected executor.
// This exposes no SqliteConnection and adds no statement-control escape hatch.
impl<'connection> Deref for OriginalReader<'_, 'connection> {
    type Target = ServiceSqliteTransaction<'connection>;
    fn deref(&self) -> &Self::Target {
        self.tx
    }
}
impl DerefMut for OriginalReader<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.tx
    }
}

impl Proofs {
    fn retain(&mut self, key: Key, receipt: Arc<HubActualReviewReceipt>) {
        self.retain_bounded(key, receipt, self.entry_limit, self.payload_limit);
    }

    fn retain_bounded(
        &mut self,
        key: Key,
        receipt: Arc<HubActualReviewReceipt>,
        entries: usize,
        bytes: usize,
    ) {
        let Some(charge) = charge(&key, &receipt) else {
            return;
        };
        if entries == 0 || charge > bytes {
            return;
        }
        while self.completed.len() >= entries || self.bytes > bytes - charge {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.completed.remove(&oldest) {
                self.bytes -= removed.charge;
            }
        }
        self.bytes += charge;
        self.order.push_back(key.clone());
        self.completed.insert(key, Entry { receipt, charge });
    }
}

/// Bounded serialized payload accounting plus key/entry overhead. Oversized
/// receipts are returned uncached; cache pressure never reduces product limits.
fn charge(key: &Key, receipt: &HubActualReviewReceipt) -> Option<usize> {
    struct Count(usize);
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|v| *v <= PAYLOAD_BYTES)
                .ok_or_else(|| io::Error::other("Original proof cache payload limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    let common = (
        receipt.operation.as_str(),
        receipt.revision,
        receipt.catalog_revision,
        &receipt.lineage,
        &receipt.historical_holds,
    );
    let encoded = match &receipt.original {
        HubActualReviewOriginal::Departure(plan) => {
            serde_json::to_writer(&mut count, &(common, plan))
        }
        HubActualReviewOriginal::Duplicate(plan) => {
            serde_json::to_writer(&mut count, &(common, plan))
        }
    };
    encoded.ok()?;
    count
        .0
        .checked_add(std::mem::size_of::<Entry>() + std::mem::size_of::<HubActualReviewReceipt>())?
        .checked_add(2 * (std::mem::size_of::<Key>() + key.operation.len()))
        .filter(|v| *v <= PAYLOAD_BYTES)
}
