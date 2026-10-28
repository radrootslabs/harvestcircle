//! Bounded reference data for the single-desktop whole-pack demonstration.
//!
//! These rules establish neither native identity nor persistence authority. The
//! adapter applies a change and its original-operation receipt in one protected
//! transaction. References are never inferred from labels or source-code equality.

use std::collections::BTreeMap;

use crate::hub::HubRevision;

pub const HUB_REFERENCE_CAPACITY: usize = 512;
pub const HUB_REFERENCE_HISTORY_CAPACITY: usize = 16_384;
pub const HUB_REFERENCE_PAGE_ROWS: usize = 64;
pub const HUB_LABEL_UTF8_BYTES: usize = 120;
pub const HUB_SOURCE_CODE_UTF8_BYTES: usize = 128;
pub const HUB_PROVENANCE_UTF8_BYTES: usize = 512;
pub const HUB_MAX_WHOLE_PACKS: u32 = 1_000_000;
pub const HUB_MAX_NOMINAL_GRAMS: u32 = 1_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubReferenceError {
    InvalidId,
    InvalidText,
    InvalidQuantity,
    DifferentVariant,
    Capacity,
    DuplicateId,
    MissingReference,
    WrongReferenceKind,
    StaleRevision,
    RevisionExhausted,
    ImmutablePack,
    ImmutableLocationPurpose,
    NoChange,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HubRecordId([u8; 16]);

impl HubRecordId {
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, HubReferenceError> {
        if bytes == [0; 16] {
            return Err(HubReferenceError::InvalidId);
        }
        Ok(Self(bytes))
    }

    pub fn from_hex(value: &str) -> Result<Self, HubReferenceError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(HubReferenceError::InvalidId);
        }
        let mut bytes = [0; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| HubReferenceError::InvalidId)?;
        }
        Self::from_bytes(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Original UTF-8 text is retained; blank, control and bidi-override input denies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubText<const MAX_BYTES: usize>(String);

impl<const MAX_BYTES: usize> HubText<MAX_BYTES> {
    pub fn new(value: impl Into<String>) -> Result<Self, HubReferenceError> {
        let value = value.into();
        if value.len() > MAX_BYTES
            || value.trim().is_empty()
            || value.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(HubReferenceError::InvalidText);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub type HubLabel = HubText<HUB_LABEL_UTF8_BYTES>;
pub type SourceCode = HubText<HUB_SOURCE_CODE_UTF8_BYTES>;
pub type SourceProvenance = HubText<HUB_PROVENANCE_UTF8_BYTES>;

/// Whole original packs; nominal mass is never an arithmetic stock unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WholePacks(u32);

impl WholePacks {
    pub fn from_value(value: u32) -> Result<Self, HubReferenceError> {
        if value > HUB_MAX_WHOLE_PACKS {
            return Err(HubReferenceError::InvalidQuantity);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, HubReferenceError> {
        Self::from_value(
            self.0
                .checked_add(other.0)
                .ok_or(HubReferenceError::InvalidQuantity)?,
        )
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, HubReferenceError> {
        Self::from_value(
            self.0
                .checked_sub(other.0)
                .ok_or(HubReferenceError::InvalidQuantity)?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VariantQuantity {
    pub variant: HubRecordId,
    pub packs: WholePacks,
}

impl VariantQuantity {
    pub fn checked_add(self, other: Self) -> Result<Self, HubReferenceError> {
        if self.variant != other.variant {
            return Err(HubReferenceError::DifferentVariant);
        }
        Ok(Self {
            variant: self.variant,
            packs: self.packs.checked_add(other.packs)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NominalGrams(u32);

impl NominalGrams {
    pub fn from_value(value: u32) -> Result<Self, HubReferenceError> {
        if value == 0 || value > HUB_MAX_NOMINAL_GRAMS {
            return Err(HubReferenceError::InvalidQuantity);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationPurpose {
    Receiving,
    Storage,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceKind {
    Product,
    Pack,
    Party,
    Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceValue {
    Product {
        label: HubLabel,
    },
    Pack {
        product: HubRecordId,
        label: HubLabel,
        nominal_grams: Option<NominalGrams>,
    },
    Party {
        label: HubLabel,
    },
    Location {
        label: HubLabel,
        purpose: LocationPurpose,
    },
}

impl ReferenceValue {
    #[must_use]
    pub const fn kind(&self) -> ReferenceKind {
        match self {
            Self::Product { .. } => ReferenceKind::Product,
            Self::Pack { .. } => ReferenceKind::Pack,
            Self::Party { .. } => ReferenceKind::Party,
            Self::Location { .. } => ReferenceKind::Location,
        }
    }

    #[must_use]
    pub fn label(&self) -> &HubLabel {
        match self {
            Self::Product { label }
            | Self::Pack { label, .. }
            | Self::Party { label }
            | Self::Location { label, .. } => label,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceRecord {
    pub id: HubRecordId,
    pub revision: HubRevision,
    pub value: ReferenceValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceChange {
    pub catalog_revision: HubRevision,
    pub previous: Option<ReferenceRecord>,
    pub current: ReferenceRecord,
}

/// Only current references are held here. Immutable historical versions belong
/// in SQL and in as-issued snapshots, never rewritten by a current label edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceCatalog {
    revision: HubRevision,
    records: BTreeMap<HubRecordId, ReferenceRecord>,
}

impl Default for ReferenceCatalog {
    fn default() -> Self {
        Self {
            revision: HubRevision::INITIAL,
            records: BTreeMap::new(),
        }
    }
}

impl ReferenceCatalog {
    pub fn restore(
        revision: HubRevision,
        records: Vec<ReferenceRecord>,
    ) -> Result<Self, HubReferenceError> {
        if records.len() > HUB_REFERENCE_CAPACITY {
            return Err(HubReferenceError::Capacity);
        }
        let mut catalog = Self {
            revision,
            records: BTreeMap::new(),
        };
        for record in records {
            if record.revision.value() > revision.value() {
                return Err(HubReferenceError::StaleRevision);
            }
            if catalog.records.insert(record.id, record).is_some() {
                return Err(HubReferenceError::DuplicateId);
            }
        }
        for record in catalog.records.values() {
            catalog.validate_value(&record.value)?;
        }
        Ok(catalog)
    }

    #[must_use]
    pub const fn revision(&self) -> HubRevision {
        self.revision
    }

    pub fn records(&self) -> impl ExactSizeIterator<Item = &ReferenceRecord> {
        self.records.values()
    }

    pub fn get(&self, id: HubRecordId) -> Result<&ReferenceRecord, HubReferenceError> {
        self.records
            .get(&id)
            .ok_or(HubReferenceError::MissingReference)
    }

    pub fn require_kind(
        &self,
        id: HubRecordId,
        kind: ReferenceKind,
    ) -> Result<&ReferenceRecord, HubReferenceError> {
        let record = self.get(id)?;
        if record.value.kind() != kind {
            return Err(HubReferenceError::WrongReferenceKind);
        }
        Ok(record)
    }

    pub fn create(
        &mut self,
        expected: HubRevision,
        id: HubRecordId,
        value: ReferenceValue,
    ) -> Result<ReferenceChange, HubReferenceError> {
        self.require_revision(expected)?;
        if self.records.contains_key(&id) {
            return Err(HubReferenceError::DuplicateId);
        }
        if self.records.len() >= HUB_REFERENCE_CAPACITY {
            return Err(HubReferenceError::Capacity);
        }
        self.validate_value(&value)?;
        let next = next_revision(self.revision)?;
        let record = ReferenceRecord {
            id,
            revision: HubRevision::INITIAL,
            value,
        };
        self.records.insert(id, record.clone());
        self.revision = next;
        Ok(ReferenceChange {
            catalog_revision: next,
            previous: None,
            current: record,
        })
    }

    pub fn revise(
        &mut self,
        expected: HubRevision,
        id: HubRecordId,
        record_revision: HubRevision,
        value: ReferenceValue,
    ) -> Result<ReferenceChange, HubReferenceError> {
        self.require_revision(expected)?;
        let previous = self.get(id)?.clone();
        if previous.revision != record_revision {
            return Err(HubReferenceError::StaleRevision);
        }
        if previous.value.kind() != value.kind() {
            return Err(HubReferenceError::WrongReferenceKind);
        }
        if previous.value == value {
            return Err(HubReferenceError::NoChange);
        }
        match (&previous.value, &value) {
            (ReferenceValue::Pack { .. }, _) => return Err(HubReferenceError::ImmutablePack),
            (
                ReferenceValue::Location { purpose: old, .. },
                ReferenceValue::Location { purpose: new, .. },
            ) if old != new => {
                return Err(HubReferenceError::ImmutableLocationPurpose);
            }
            _ => {}
        }
        self.validate_value(&value)?;
        let next = next_revision(self.revision)?;
        let record = ReferenceRecord {
            id,
            revision: next_revision(record_revision)?,
            value,
        };
        self.records.insert(id, record.clone());
        self.revision = next;
        Ok(ReferenceChange {
            catalog_revision: next,
            previous: Some(previous),
            current: record,
        })
    }

    fn require_revision(&self, expected: HubRevision) -> Result<(), HubReferenceError> {
        if expected != self.revision {
            return Err(HubReferenceError::StaleRevision);
        }
        Ok(())
    }

    fn validate_value(&self, value: &ReferenceValue) -> Result<(), HubReferenceError> {
        if let ReferenceValue::Pack { product, .. } = value {
            self.require_kind(*product, ReferenceKind::Product)?;
        }
        Ok(())
    }
}

fn next_revision(revision: HubRevision) -> Result<HubRevision, HubReferenceError> {
    HubRevision::from_value(revision.value() + 1).map_err(|_| HubReferenceError::RevisionExhausted)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceReferenceKind {
    ProducerBatch,
    SupplierReference,
    Other,
}

/// Two unknown issuers with the same code are not established as one source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReference {
    pub id: HubRecordId,
    pub issuer: Option<HubRecordId>,
    pub kind: SourceReferenceKind,
    pub code: SourceCode,
    pub provenance: SourceProvenance,
}

impl SourceReference {
    pub fn validate(&self, catalog: &ReferenceCatalog) -> Result<(), HubReferenceError> {
        if let Some(issuer) = self.issuer {
            catalog.require_kind(issuer, ReferenceKind::Party)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCertainty {
    ReportedPossible,
    Established,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceAttribution {
    Unknown,
    Referenced {
        source: SourceReference,
        certainty: SourceCertainty,
    },
}

#[cfg(test)]
mod tests;
