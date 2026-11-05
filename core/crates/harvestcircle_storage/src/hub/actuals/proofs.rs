//! Immutable original proofs belong to one read phase of one native transaction.
//! No owner, custody, current permission, pending intent or admission is cached.
use super::{frozen, mode, model::*, projection, reviews};
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{
        corrections::{
            count_class_record::{self, HubCountClassificationReceipt},
            count_mode,
            count_record::{self, HubCountExplanationReceipt},
        },
        stock,
    },
};
use harvestcircle_domain::{SafeError, hub::HubId};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Write},
    ops::{Deref, DerefMut},
    sync::Arc,
};

const ENTRIES: usize = harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY;
// Receipt bodies retain the existing shared entry/64MiB budget. Fixed-shape
// certificates additionally cover every permitted correction root; cache limits
// must not reject a valid maximum count history after bodies are evicted.
const CERTIFICATES: usize = ENTRIES + harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS;
const PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[cfg(test)]
mod tests;

/// Includes the complete canonical operation identity, not caller-selected cuts.
/// Epoch/mode and native capture/result seals are resolved before either cache.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    hub: [u8; 16],
    operation: String,
    kind: String,
    actor: [u8; 32],
    intent: [u8; 32],
    stock: u64,
    catalog: u64,
    result: [u8; 16],
    recorded: i64,
    schema: u32,
    mode: InterpretationMode,
    seals: Seals,
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum InterpretationMode {
    Actual(mode::Mode),
    Count(count_mode::Mode),
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Seals {
    result: [u8; 32],
    capture: [u8; 32],
    projection: Option<[u8; 32]>,
}
impl Key {
    fn canonical(
        hub: HubId,
        op: &stock::read::OperationRecord,
        mode: mode::Mode,
        seals: Seals,
    ) -> Self {
        Self::with_mode(hub, op, InterpretationMode::Actual(mode), seals)
    }
    fn with_mode(
        hub: HubId,
        op: &stock::read::OperationRecord,
        mode: InterpretationMode,
        seals: Seals,
    ) -> Self {
        Self {
            hub: *hub.as_bytes(),
            operation: op.id.as_str().to_owned(),
            kind: op.kind.clone(),
            actor: *op.actor.as_bytes(),
            intent: op.hash,
            stock: op.revision.value(),
            catalog: op.catalog_revision.value(),
            result: *op.result.as_bytes(),
            recorded: op.recorded_at.as_seconds(),
            schema: crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION,
            mode,
            seals,
        }
    }
}

struct Entry {
    receipt: Receipt,
    charge: usize,
}
#[derive(Clone)]
enum Receipt {
    Actual(Arc<HubActualReviewReceipt>),
    Count(Arc<HubCountExplanationReceipt>),
    Classification(Arc<HubCountClassificationReceipt>),
}
enum Certificate {
    Actual(bool),
    Count,
}

struct Proofs {
    completed: BTreeMap<Key, Entry>,
    order: VecDeque<Key>,
    active: Vec<Key>,
    // One small fixed-shape identity and review kind per bounded original.
    // These certificates retain neither receipt bodies nor interpretation sets.
    // They expire with this transaction read phase and convey no authority.
    verified: BTreeMap<Key, Certificate>,
    bytes: usize,
    entry_limit: usize,
    payload_limit: usize,
    #[cfg(test)]
    rebuilt: usize,
    #[cfg(test)]
    hits: usize,
    #[cfg(test)]
    certificate_hits: usize,
}
impl Default for Proofs {
    fn default() -> Self {
        Self {
            completed: BTreeMap::new(),
            order: VecDeque::new(),
            active: Vec::new(),
            verified: BTreeMap::new(),
            bytes: 0,
            entry_limit: ENTRIES,
            payload_limit: PAYLOAD_BYTES,
            #[cfg(test)]
            rebuilt: 0,
            #[cfg(test)]
            hits: 0,
            #[cfg(test)]
            certificate_hits: 0,
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

    #[cfg(test)]
    pub(in crate::hub) fn certificate_counts(&self) -> (usize, usize) {
        (self.proofs.certificate_hits, self.proofs.verified.len())
    }

    async fn canonical_key(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
    ) -> Result<(stock::read::OperationRecord, Key), SafeError> {
        // Re-read the canonical native header before selecting a cached proof.
        let op = stock::read::operation_on(self.tx, hub, &supplied.id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let mode = mode::original_on(self.tx, hub, &op).await?;
        let row = sqlx::query("SELECT substr(r.result_hash,1,33) AS result_hash,substr(r.projection_hash,1,33) AS projection_hash,substr(s.capture_hash,1,33) AS capture_hash FROM hub_actual_reviews r JOIN hub_submissions s ON s.operation_id=r.operation_id WHERE r.operation_id=? LIMIT 1")
            .bind(op.id.as_str()).fetch_optional(&mut *self.tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
        let bytes = |name| -> Result<[u8; 32], SafeError> {
            row.try_get::<Vec<u8>, _>(name)
                .map_err(|_| corrupt_storage())?
                .try_into()
                .map_err(|_| corrupt_storage())
        };
        let seals = Seals {
            result: bytes("result_hash")?,
            capture: bytes("capture_hash")?,
            projection: row
                .try_get::<Option<Vec<u8>>, _>("projection_hash")
                .map_err(|_| corrupt_storage())?
                .map(|v| v.try_into().map_err(|_| corrupt_storage()))
                .transpose()?,
        };
        if mode
            .capture_hash()
            .is_some_and(|capture| capture != seals.capture)
        {
            return Err(corrupt_storage());
        }
        let key = Key::canonical(hub, &op, mode, seals);
        if op.kind != "actual_review"
            || supplied.kind != op.kind
            || Key::canonical(hub, supplied, mode, seals) != key
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
            || self.proofs.active.len() >= CERTIFICATES
        {
            return Err(corrupt_storage());
        }
        Ok((op, key))
    }

    /// Verify the complete original once even when its full receipt is evicted.
    /// The boolean is only its immutable review kind, never effect permission.
    pub(in crate::hub) async fn actual_is_departure(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
    ) -> Result<bool, SafeError> {
        let (op, key) = self.canonical_key(hub, supplied).await?;
        if let Some(Certificate::Actual(physical)) = self.proofs.verified.get(&key) {
            #[cfg(test)]
            {
                self.proofs.certificate_hits += 1;
            }
            return Ok(*physical);
        }
        let receipt = Box::pin(self.actual_review(hub, op)).await?;
        Ok(matches!(
            receipt.original,
            HubActualReviewOriginal::Departure(_)
        ))
    }

    pub(in crate::hub) async fn actual_known(
        &mut self,
        hub: HubId,
        op: &stock::read::OperationRecord,
    ) -> Result<harvestcircle_domain::hub::actuals::ActualKnownDeparture, SafeError> {
        if !Box::pin(self.actual_is_departure(hub, op)).await? {
            return Err(corrupt_storage());
        }
        projection::known_on(self.tx, hub, op).await
    }

    pub(in crate::hub) async fn actual_before(
        &mut self,
        hub: HubId,
        op: &stock::read::OperationRecord,
    ) -> Result<harvestcircle_domain::hub::movement::PhysicalStockGroup, SafeError> {
        if !Box::pin(self.actual_is_departure(hub, op)).await? {
            return Err(corrupt_storage());
        }
        // Only this owner invokes the shallow frozen-group decoder, after the
        // complete original certificate was checked in this same read phase.
        frozen::verified_before_on(self.tx, hub, op).await
    }

    pub(in crate::hub) async fn actual_review(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubActualReviewReceipt>, SafeError> {
        let (op, key) = self.canonical_key(hub, &supplied).await?;
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return match &entry.receipt {
                Receipt::Actual(receipt) => Ok(Arc::clone(receipt)),
                _ => Err(corrupt_storage()),
            };
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
        if self.proofs.verified.len() >= CERTIFICATES && !self.proofs.verified.contains_key(&key) {
            return Err(corrupt_storage());
        }
        self.proofs.verified.insert(
            key.clone(),
            Certificate::Actual(matches!(
                receipt.original,
                HubActualReviewOriginal::Departure(_)
            )),
        );
        self.proofs.retain(key, Arc::clone(&receipt));
        Ok(receipt)
    }

    async fn canonical_count_key(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
        kind: &str,
    ) -> Result<(stock::read::OperationRecord, Key), SafeError> {
        let op = stock::read::operation_on(self.tx, hub, &supplied.id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != kind
            || !matches!(
                kind,
                "correction_count_explain" | "correction_count_classify"
            )
            || supplied.kind != op.kind
        {
            return Err(corrupt_storage());
        }
        let mode = count_mode::original_on(self.tx, hub, &op).await?;
        let row = if kind == "correction_count_explain" {
            sqlx::query(concat!(
            "SELECT substr(c.result_hash,1,33) AS result_hash,substr(s.capture_hash,1,33) AS capture_hash,",
            "substr(p.projection_hash,1,33) AS projection_hash FROM hub_corrections c ",
            "JOIN hub_submissions s ON s.operation_id=c.operation_id JOIN hub_correction_count_explanations p ON p.operation_id=c.operation_id ",
            "WHERE c.operation_id=? AND c.family='count_explain' LIMIT 1"
        )).bind(op.id.as_str()).fetch_optional(&mut *self.tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?
        } else {
            sqlx::query(concat!(
                "SELECT substr(c.result_hash,1,33) AS result_hash,substr(s.capture_hash,1,33) AS capture_hash,",
                "substr(p.projection_hash,1,33) AS projection_hash FROM hub_corrections c ",
                "JOIN hub_submissions s ON s.operation_id=c.operation_id JOIN hub_correction_quantity_changes p ON p.operation_id=c.operation_id ",
                "WHERE c.operation_id=? AND c.family='count_classify' AND p.delta_kind='classification' LIMIT 1"
            )).bind(op.id.as_str()).fetch_optional(&mut *self.tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?
        };
        let bytes = |field| -> Result<[u8; 32], SafeError> {
            row.try_get::<Vec<u8>, _>(field)
                .map_err(|_| corrupt_storage())?
                .try_into()
                .map_err(|_| corrupt_storage())
        };
        let seals = Seals {
            result: bytes("result_hash")?,
            capture: bytes("capture_hash")?,
            projection: Some(bytes("projection_hash")?),
        };
        let key = Key::with_mode(hub, &op, InterpretationMode::Count(mode), seals);
        if seals.capture != mode.capture
            || Key::with_mode(hub, supplied, InterpretationMode::Count(mode), seals) != key
            || self
                .proofs
                .active
                .last()
                .is_some_and(|parent| parent.hub != key.hub || key.stock >= parent.stock)
            || self.proofs.active.len() >= CERTIFICATES
        {
            return Err(corrupt_storage());
        }
        Ok((op, key))
    }

    /// The certificate covers only an immutable original in this read phase.
    /// It creates no current permission, pending clearance or fresh admission.
    pub(in crate::hub) async fn verify_count_explanation(
        &mut self,
        hub: HubId,
        op: &stock::read::OperationRecord,
    ) -> Result<(), SafeError> {
        let (canonical, key) = self
            .canonical_count_key(hub, op, "correction_count_explain")
            .await?;
        if matches!(self.proofs.verified.get(&key), Some(Certificate::Count)) {
            #[cfg(test)]
            {
                self.proofs.certificate_hits += 1;
            }
            return Ok(());
        }
        Box::pin(self.count_explanation(hub, canonical)).await?;
        Ok(())
    }

    pub(in crate::hub) async fn count_explanation(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubCountExplanationReceipt>, SafeError> {
        let (op, key) = self
            .canonical_count_key(hub, &supplied, "correction_count_explain")
            .await?;
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return match &entry.receipt {
                Receipt::Count(receipt) => Ok(Arc::clone(receipt)),
                _ => Err(corrupt_storage()),
            };
        }
        self.proofs.active.push(key.clone());
        #[cfg(test)]
        {
            self.proofs.rebuilt += 1;
        }
        let result = Box::pin(count_record::rebuild_on(self, hub, op)).await;
        if self.proofs.active.pop().as_ref() != Some(&key) {
            return Err(corrupt_storage());
        }
        let receipt = Arc::new(result?);
        if self.proofs.verified.len() >= CERTIFICATES && !self.proofs.verified.contains_key(&key) {
            return Err(corrupt_storage());
        }
        self.proofs.verified.insert(key.clone(), Certificate::Count);
        self.proofs.retain_receipt(
            key,
            Receipt::Count(Arc::clone(&receipt)),
            self.proofs.entry_limit,
            self.proofs.payload_limit,
        );
        Ok(receipt)
    }

    pub(in crate::hub) async fn verify_count_classification(
        &mut self,
        hub: HubId,
        op: &stock::read::OperationRecord,
    ) -> Result<(), SafeError> {
        let (canonical, key) = self
            .canonical_count_key(hub, op, "correction_count_classify")
            .await?;
        if matches!(self.proofs.verified.get(&key), Some(Certificate::Count)) {
            #[cfg(test)]
            {
                self.proofs.certificate_hits += 1;
            }
            return Ok(());
        }
        Box::pin(self.count_classification(hub, canonical)).await?;
        Ok(())
    }
    pub(in crate::hub) async fn count_classification(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubCountClassificationReceipt>, SafeError> {
        let (op, key) = self
            .canonical_count_key(hub, &supplied, "correction_count_classify")
            .await?;
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return match &entry.receipt {
                Receipt::Classification(receipt) => Ok(Arc::clone(receipt)),
                _ => Err(corrupt_storage()),
            };
        }
        self.proofs.active.push(key.clone());
        #[cfg(test)]
        {
            self.proofs.rebuilt += 1;
        }
        let result = count_class_record::rebuild_on(self, hub, op).await;
        if self.proofs.active.pop().as_ref() != Some(&key) {
            return Err(corrupt_storage());
        }
        let receipt = Arc::new(result?);
        if self.proofs.verified.len() >= CERTIFICATES && !self.proofs.verified.contains_key(&key) {
            return Err(corrupt_storage());
        }
        self.proofs.verified.insert(key.clone(), Certificate::Count);
        self.proofs.retain_receipt(
            key,
            Receipt::Classification(Arc::clone(&receipt)),
            self.proofs.entry_limit,
            self.proofs.payload_limit,
        );
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
        self.retain_receipt(key, Receipt::Actual(receipt), entries, bytes);
    }
    fn retain_receipt(&mut self, key: Key, receipt: Receipt, entries: usize, bytes: usize) {
        let Some(charge) = charge_receipt(&key, &receipt) else {
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
    charge_payload(key, Payload::Actual(receipt))
}
enum Payload<'a> {
    Actual(&'a HubActualReviewReceipt),
    Count(&'a HubCountExplanationReceipt),
    Classification(&'a HubCountClassificationReceipt),
}
fn charge_receipt(key: &Key, receipt: &Receipt) -> Option<usize> {
    match receipt {
        Receipt::Actual(receipt) => charge(key, receipt),
        Receipt::Count(receipt) => charge_payload(key, Payload::Count(receipt)),
        Receipt::Classification(receipt) => charge_payload(key, Payload::Classification(receipt)),
    }
}
fn charge_payload(key: &Key, receipt: Payload<'_>) -> Option<usize> {
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
    let encoded = match receipt {
        Payload::Classification(receipt) => {
            serde_json::to_writer(&mut count, &(receipt.operation.as_str(), &receipt.original))
        }
        Payload::Count(receipt) => serde_json::to_writer(
            &mut count,
            &(
                receipt.operation.as_str(),
                &receipt.original,
                &receipt.source,
            ),
        ),
        Payload::Actual(receipt) => {
            let common = (
                receipt.operation.as_str(),
                receipt.revision,
                receipt.catalog_revision,
                &receipt.lineage,
                &receipt.historical_holds,
            );
            match &receipt.original {
                HubActualReviewOriginal::Departure(plan) => {
                    serde_json::to_writer(&mut count, &(common, plan))
                }
                HubActualReviewOriginal::Duplicate(plan) => {
                    serde_json::to_writer(&mut count, &(common, plan))
                }
            }
        }
    };
    encoded.ok()?;
    count
        .0
        .checked_add(
            std::mem::size_of::<Entry>()
                + std::mem::size_of::<HubActualReviewReceipt>()
                    .max(std::mem::size_of::<HubCountExplanationReceipt>())
                    .max(std::mem::size_of::<HubCountClassificationReceipt>()),
        )?
        .checked_add(2 * (std::mem::size_of::<Key>() + key.operation.len() + key.kind.len()))
        .filter(|v| *v <= PAYLOAD_BYTES)
}
