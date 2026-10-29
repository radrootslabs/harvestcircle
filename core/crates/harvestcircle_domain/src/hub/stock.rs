//! Whole-pack physical intake policy. Planning has no persistence or authority.
//!
//! The native adapter validates current authority and commits the plan, original
//! operation result and stock/work/history effects in one protected transaction.

use sha2::{Digest, Sha256};

use super::reference::{
    HubRecordId, HubReferenceError, LocationPurpose, ReferenceCatalog, ReferenceKind,
    ReferenceRecord, ReferenceValue, SourceAttribution, SourceCertainty, SourceProvenance,
    SourceReferenceKind, WholePacks,
};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};

pub const HUB_TRACE_CAPACITY: usize = 4_096;
pub const HUB_STOCK_EVENT_CAPACITY: usize = 32_768;
pub const HUB_STOCK_PAGE_ROWS: usize = 64;

/// Native-only deterministic identities retain the original operation across retries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptIdentities {
    pub trace: HubRecordId,
    pub group: HubRecordId,
    pub source: HubRecordId,
}

impl ReceiptIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, StockError> {
        let derive = |purpose: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(b"harvestcircle.hub.receipt.identity.v1\0");
            hash.update(purpose);
            hash.update(operation.as_bytes());
            let digest = hash.finalize();
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&digest[..16]);
            HubRecordId::from_bytes(bytes).map_err(StockError::from)
        };
        Ok(Self {
            trace: derive(b"trace\0")?,
            group: derive(b"group\0")?,
            source: derive(b"source\0")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StockError {
    Reference(HubReferenceError),
    StaleRevision,
    RevisionExhausted,
    EmptyQuantity,
    InvalidReceivingLocation,
    InvalidPutAwayTarget,
    InvalidOccurrence,
    InvalidWorkAccounting,
    SourceIdentityConflict,
}

impl From<HubReferenceError> for StockError {
    fn from(value: HubReferenceError) -> Self {
        Self::Reference(value)
    }
}

/// Recording order never establishes when an actual event occurred.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Occurrence {
    Known(UnixTimestamp),
    Interval {
        earliest: UnixTimestamp,
        latest: UnixTimestamp,
    },
    Unknown,
}

impl Occurrence {
    pub fn validate(self, recorded_at: UnixTimestamp) -> Result<(), StockError> {
        let valid = match self {
            Self::Known(at) => at <= recorded_at,
            Self::Interval { earliest, latest } => earliest <= latest && latest <= recorded_at,
            Self::Unknown => true,
        };
        valid.then_some(()).ok_or(StockError::InvalidOccurrence)
    }
}

/// One quantity requirement. A closure or planned location is not completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct QuantityWork {
    required: WholePacks,
    completed: WholePacks,
    no_longer_required: WholePacks,
    remaining: WholePacks,
}

impl QuantityWork {
    #[must_use]
    pub fn pending(required: WholePacks) -> Self {
        Self {
            required,
            completed: WholePacks::from_value(0).expect("zero whole packs"),
            no_longer_required: WholePacks::from_value(0).expect("zero whole packs"),
            remaining: required,
        }
    }

    pub fn restore(
        required: WholePacks,
        completed: WholePacks,
        no_longer_required: WholePacks,
        remaining: WholePacks,
    ) -> Result<Self, StockError> {
        let accounted = completed
            .checked_add(no_longer_required)?
            .checked_add(remaining)?;
        if accounted != required {
            return Err(StockError::InvalidWorkAccounting);
        }
        Ok(Self {
            required,
            completed,
            no_longer_required,
            remaining,
        })
    }

    #[must_use]
    pub const fn required(self) -> WholePacks {
        self.required
    }
    #[must_use]
    pub const fn completed(self) -> WholePacks {
        self.completed
    }
    #[must_use]
    pub const fn no_longer_required(self) -> WholePacks {
        self.no_longer_required
    }
    #[must_use]
    pub const fn remaining(self) -> WholePacks {
        self.remaining
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReceiptCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub variant: HubRecordId,
    pub supplier: Option<HubRecordId>,
    pub producer: Option<HubRecordId>,
    pub source: SourceAttribution,
    pub packs: WholePacks,
    pub receiving: HubRecordId,
    pub planned_location: Option<HubRecordId>,
    pub occurred: Occurrence,
    /// Original manual confirmation, retained separately from source provenance.
    pub observation: SourceProvenance,
}

mod opening;
pub use opening::{OpeningCommand, OpeningError, OpeningIdentities, OpeningPlan};

/// A counted opening is a present observation, never a fabricated historical receipt.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningHistory {
    Unknown,
    ExternalReference(SourceProvenance),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PopulationOrigin {
    Receipt,
    CountedOpening {
        history: OpeningHistory,
    },
    CountedSurplus {
        count: HubRecordId,
        related_population: HubRecordId,
    },
}

/// Original reference versions survive later label changes and corrections.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct StockPopulation {
    pub id: HubRecordId,
    pub variant: ReferenceRecord,
    pub product: ReferenceRecord,
    pub supplier: Option<ReferenceRecord>,
    pub producer: Option<ReferenceRecord>,
    pub source: SourceAttribution,
    pub source_issuer: Option<ReferenceRecord>,
    pub original_packs: WholePacks,
    pub initial_location: ReferenceRecord,
    pub origin: PopulationOrigin,
    pub planned_location: Option<ReferenceRecord>,
    pub occurred: Occurrence,
    pub recorded_at: UnixTimestamp,
    pub actor: PublicKey,
    pub observation: SourceProvenance,
}

impl StockPopulation {
    /// Missing/possible provenance stays visible; this is not acceptance.
    #[must_use]
    pub fn source_needs_review(&self) -> bool {
        match &self.source {
            SourceAttribution::Unknown => true,
            SourceAttribution::Referenced { source, certainty } => {
                *certainty != SourceCertainty::Established || source.issuer.is_none()
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StockPosition {
    pub group: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub location: HubRecordId,
    pub packs: WholePacks,
    pub revision: HubRevision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptPlan {
    pub stock_revision: HubRevision,
    pub trace: StockPopulation,
    pub position: StockPosition,
    pub put_away: QuantityWork,
}

impl ReceiptCommand {
    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        current_stock_revision: HubRevision,
        trace: HubRecordId,
        group: HubRecordId,
        actor: PublicKey,
        recorded_at: UnixTimestamp,
    ) -> Result<ReceiptPlan, StockError> {
        if self.expected_stock_revision != current_stock_revision
            || self.expected_catalog_revision != catalog.revision()
        {
            return Err(StockError::StaleRevision);
        }
        if self.packs.value() == 0 {
            return Err(StockError::EmptyQuantity);
        }
        self.occurred.validate(recorded_at)?;
        let variant = catalog
            .require_kind(self.variant, ReferenceKind::Pack)?
            .clone();
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(StockError::Reference(HubReferenceError::WrongReferenceKind));
        };
        let product = catalog
            .require_kind(product, ReferenceKind::Product)?
            .clone();
        let party = |id| catalog.require_kind(id, ReferenceKind::Party).cloned();
        let supplier = self.supplier.map(party).transpose()?;
        let producer = self.producer.map(party).transpose()?;
        let source_issuer = match &self.source {
            SourceAttribution::Unknown => None,
            SourceAttribution::Referenced { source, .. } => {
                source.validate(catalog)?;
                source.issuer.map(party).transpose()?
            }
        };
        let receiving = catalog
            .require_kind(self.receiving, ReferenceKind::Location)?
            .clone();
        if !matches!(
            receiving.value,
            ReferenceValue::Location {
                purpose: LocationPurpose::Receiving,
                ..
            }
        ) {
            return Err(StockError::InvalidReceivingLocation);
        }
        if self.planned_location == Some(self.receiving) {
            return Err(StockError::InvalidPutAwayTarget);
        }
        let planned_location = self
            .planned_location
            .map(|id| catalog.require_kind(id, ReferenceKind::Location).cloned())
            .transpose()?;
        let stock_revision = HubRevision::from_value(current_stock_revision.value() + 1)
            .map_err(|_| StockError::RevisionExhausted)?;
        Ok(ReceiptPlan {
            stock_revision,
            trace: StockPopulation {
                id: trace,
                variant,
                product,
                supplier,
                producer,
                source: self.source.clone(),
                source_issuer,
                original_packs: self.packs,
                initial_location: receiving,
                origin: PopulationOrigin::Receipt,
                planned_location,
                occurred: self.occurred,
                recorded_at,
                actor,
                observation: self.observation.clone(),
            },
            position: StockPosition {
                group,
                trace,
                variant: self.variant,
                location: self.receiving,
                packs: self.packs,
                revision: HubRevision::INITIAL,
            },
            put_away: QuantityWork::pending(self.packs),
        })
    }

    /// Frozen v1 semantic intent, independent of later labels and transport timeouts.
    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        trace: HubRecordId,
        group: HubRecordId,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.receipt.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(trace.as_bytes());
        hash.update(group.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.variant.as_bytes());
        hash_optional_id(&mut hash, self.supplier);
        hash_optional_id(&mut hash, self.producer);
        hash.update(self.packs.value().to_be_bytes());
        hash.update(self.receiving.as_bytes());
        hash_optional_id(&mut hash, self.planned_location);
        hash_source(&mut hash, &self.source);
        match self.occurred {
            Occurrence::Unknown => hash.update([0]),
            Occurrence::Known(at) => {
                hash.update([1]);
                hash.update(at.as_seconds().to_be_bytes());
            }
            Occurrence::Interval { earliest, latest } => {
                hash.update([2]);
                hash.update(earliest.as_seconds().to_be_bytes());
                hash.update(latest.as_seconds().to_be_bytes());
            }
        }
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

/// Reusing a source requires its complete original identity, never code equality.
pub fn validate_source_reuse(
    proposed: &SourceAttribution,
    existing: impl IntoIterator<Item = SourceAttribution>,
) -> Result<(), StockError> {
    if let SourceAttribution::Referenced { source, .. } = proposed {
        for previous in existing {
            if let SourceAttribution::Referenced {
                source: original, ..
            } = previous
                && original.id == source.id
                && original != *source
            {
                return Err(StockError::SourceIdentityConflict);
            }
        }
    }
    Ok(())
}

fn hash_optional_id(hash: &mut Sha256, id: Option<HubRecordId>) {
    hash.update([u8::from(id.is_some())]);
    if let Some(id) = id {
        hash.update(id.as_bytes());
    }
}

fn hash_text(hash: &mut Sha256, value: &str) {
    hash.update(
        u64::try_from(value.len())
            .expect("bounded receipt text")
            .to_be_bytes(),
    );
    hash.update(value.as_bytes());
}

fn hash_source(hash: &mut Sha256, source: &SourceAttribution) {
    match source {
        SourceAttribution::Unknown => hash.update([0]),
        SourceAttribution::Referenced { source, certainty } => {
            hash.update([1]);
            hash.update(source.id.as_bytes());
            hash_optional_id(hash, source.issuer);
            hash.update([match source.kind {
                SourceReferenceKind::ProducerBatch => 1,
                SourceReferenceKind::SupplierReference => 2,
                SourceReferenceKind::Other => 3,
            }]);
            hash_text(hash, source.code.as_str());
            hash_text(hash, source.provenance.as_str());
            hash.update([match certainty {
                SourceCertainty::ReportedPossible => 1,
                SourceCertainty::Established => 2,
            }]);
        }
    }
}

#[cfg(test)]
mod tests;
