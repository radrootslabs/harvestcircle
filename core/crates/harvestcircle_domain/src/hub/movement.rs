//! Conserved actual movement and physically attributable quantity work.
//!
//! Plans neither authorize an operator nor change persistence. The native adapter
//! must load complete current scope, check identity uniqueness and commit every
//! group, requirement, event and original result in one protected transaction.

use sha2::{Digest, Sha256};

use super::reference::{
    HubLabel, HubRecordId, HubReferenceError, ReferenceCatalog, ReferenceKind, ReferenceRecord,
    SourceProvenance, WholePacks,
};
use super::stock::{Occurrence, QuantityWork, StockError};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};

mod partition;
mod work;
pub use work::{Discrepancy, ReplanCommand, ReplanPlan, WorkRequirement};
mod requirements;
pub use requirements::{NewWorkCommand, NewWorkPlan};
mod command;
pub use command::{DiscrepancyAction, DiscrepancyCommand, HandlingCommand};

pub const HUB_GROUP_CAPACITY: usize = 16_384;
pub const HUB_WORK_CAPACITY: usize = 16_384;
pub const HUB_DISCREPANCY_CAPACITY: usize = 4_096;
pub const HUB_GROUP_REQUIREMENTS: usize = 64;
pub const HUB_REQUIREMENT_GROUPS: usize = 256;

/// Native-only namespaced identities retain one original operation across retries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandlingIdentities {
    pub group: HubRecordId,
    pub requirement: HubRecordId,
    pub discrepancy: HubRecordId,
}

impl HandlingIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, MovementError> {
        let derive = |purpose: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(b"harvestcircle.hub.handling.identity.v1\0");
            hash.update(purpose);
            hash.update(operation.as_bytes());
            let digest = hash.finalize();
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&digest[..16]);
            HubRecordId::from_bytes(bytes).map_err(MovementError::from)
        };
        Ok(Self {
            group: derive(b"group\0")?,
            requirement: derive(b"work\0")?,
            discrepancy: derive(b"discrepancy\0")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovementError {
    Reference(HubReferenceError),
    Stock(StockError),
    StaleRevision,
    RevisionExhausted,
    InvalidGroup,
    InsufficientStock,
    UnchangedLocation,
    PhysicalIdentificationRequired,
    IdentityConflict,
    InvalidWorkScope,
    Capacity,
    NoRemainingWork,
    AlreadyResolved,
}

impl From<HubReferenceError> for MovementError {
    fn from(value: HubReferenceError) -> Self {
        Self::Reference(value)
    }
}
impl From<StockError> for MovementError {
    fn from(value: StockError) -> Self {
        Self::Stock(value)
    }
}

/// An operator-observed tag, separate container or other distinguishable portion.
/// A generated database ID or a claimed quantity alone is not this identification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalIdentification {
    pub label: HubLabel,
    pub observation: SourceProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkProgress {
    Pending,
    Completed,
    NoLongerRequired,
}

/// Every pack in a physical group has this same requirement status. Partial
/// treatment first separates an identified group; it never guesses mixed credit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkAllocation {
    pub requirement: WorkRequirement,
    pub progress: WorkProgress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalStockGroup {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub identification: Option<PhysicalIdentification>,
    pub location: HubRecordId,
    pub packs: WholePacks,
    pub revision: HubRevision,
    /// Derived lower bound from this portion's causal physical history, not an
    /// assertion that an unknown occurrence happened at this exact instant.
    pub causal_floor: Option<UnixTimestamp>,
    pub work: Vec<WorkAllocation>,
}

impl PhysicalStockGroup {
    pub fn validate(&self) -> Result<(), MovementError> {
        if self.packs.value() == 0 || self.parent == Some(self.id) {
            return Err(MovementError::InvalidGroup);
        }
        if self.parent.is_some() && self.identification.is_none() {
            return Err(MovementError::PhysicalIdentificationRequired);
        }
        if self.work.len() > HUB_GROUP_REQUIREMENTS {
            return Err(MovementError::Capacity);
        }
        for (index, allocation) in self.work.iter().enumerate() {
            let requirement = &allocation.requirement;
            requirement.validate()?;
            if requirement.trace != self.trace
                || requirement.variant != self.variant
                || self.work[..index]
                    .iter()
                    .any(|other| other.requirement.id == requirement.id)
            {
                return Err(MovementError::InvalidWorkScope);
            }
            let accounted = match allocation.progress {
                WorkProgress::Pending => requirement.quantity.remaining(),
                WorkProgress::Completed => requirement.quantity.completed(),
                WorkProgress::NoLongerRequired => requirement.quantity.no_longer_required(),
            };
            if accounted.value() < self.packs.value() {
                return Err(MovementError::InvalidWorkScope);
            }
            if allocation.progress == WorkProgress::Pending
                && requirement
                    .target
                    .as_ref()
                    .is_some_and(|target| target.id == self.location)
            {
                return Err(MovementError::InvalidWorkScope);
            }
        }
        Ok(())
    }

    pub(super) fn validate_catalog(&self, catalog: &ReferenceCatalog) -> Result<(), MovementError> {
        self.validate()?;
        catalog.require_kind(self.variant, ReferenceKind::Pack)?;
        catalog.require_kind(self.location, ReferenceKind::Location)?;
        for allocation in &self.work {
            if let Some(target) = &allocation.requirement.target {
                catalog.require_kind(target.id, ReferenceKind::Location)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovementCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub to: HubRecordId,
    pub packs: WholePacks,
    /// Required for a partial move. A whole-group move preserves existing identity.
    pub separated_portion: Option<PhysicalIdentification>,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovementPlan {
    pub stock_revision: HubRevision,
    pub from: HubRecordId,
    pub destination: ReferenceRecord,
    pub remaining: Option<PhysicalStockGroup>,
    pub moved: PhysicalStockGroup,
    /// Only requirements with actual newly completed work are changed.
    pub work_updates: Vec<WorkRequirement>,
}

impl MovementCommand {
    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        stock_revision: HubRevision,
        source: &PhysicalStockGroup,
        separated_group_id: HubRecordId,
        recorded_at: UnixTimestamp,
    ) -> Result<MovementPlan, MovementError> {
        source.validate_catalog(catalog)?;
        if self.expected_stock_revision != stock_revision
            || self.expected_catalog_revision != catalog.revision()
            || self.group != source.id
            || self.expected_group_revision != source.revision
        {
            return Err(MovementError::StaleRevision);
        }
        if self.packs.value() == 0 || self.packs.value() > source.packs.value() {
            return Err(MovementError::InsufficientStock);
        }
        let causal_floor = source.observation_floor(self.occurred, recorded_at)?;
        let destination = catalog
            .require_kind(self.to, ReferenceKind::Location)?
            .clone();
        if self.to == source.location {
            return Err(MovementError::UnchangedLocation);
        }
        let partition = source.partition(
            self.packs,
            separated_group_id,
            self.separated_portion.as_ref(),
        )?;
        let mut moved = partition.selected;
        let mut remaining = partition.remainder;
        moved.location = self.to;
        moved.causal_floor = causal_floor;
        let mut work_updates = Vec::new();
        for (index, allocation) in moved.work.iter_mut().enumerate() {
            if allocation.progress == WorkProgress::Pending
                && allocation
                    .requirement
                    .target
                    .as_ref()
                    .is_some_and(|target| target.id == self.to)
            {
                allocation.requirement = allocation.requirement.complete(self.packs)?;
                allocation.progress = WorkProgress::Completed;
                work_updates.push(allocation.requirement.clone());
                if let Some(parent) = remaining.as_mut() {
                    parent.work[index].requirement = allocation.requirement.clone();
                }
            }
        }
        moved.validate()?;
        if let Some(parent) = &remaining {
            parent.validate()?;
        }
        Ok(MovementPlan {
            stock_revision: next(stock_revision)?,
            from: source.location,
            destination,
            remaining,
            moved,
            work_updates,
        })
    }

    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        separated_group_id: HubRecordId,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.movement.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(separated_group_id.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.group.as_bytes());
        hash.update(self.expected_group_revision.value().to_be_bytes());
        hash.update(self.to.as_bytes());
        hash.update(self.packs.value().to_be_bytes());
        hash.update([u8::from(self.separated_portion.is_some())]);
        if let Some(identity) = &self.separated_portion {
            hash_text(&mut hash, identity.label.as_str());
            hash_text(&mut hash, identity.observation.as_str());
        }
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

pub(super) fn next(revision: HubRevision) -> Result<HubRevision, MovementError> {
    revision
        .next()
        .map_err(|_| MovementError::RevisionExhausted)
}

pub(super) fn hash_text(hash: &mut Sha256, text: &str) {
    hash.update(
        u64::try_from(text.len())
            .expect("bounded hub text")
            .to_be_bytes(),
    );
    hash.update(text.as_bytes());
}

pub(super) fn hash_occurrence(hash: &mut Sha256, occurrence: Occurrence) {
    match occurrence {
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
}

#[cfg(test)]
mod tests;
