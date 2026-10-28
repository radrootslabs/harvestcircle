use sha2::{Digest, Sha256};

use crate::hub::movement::{
    MovementError, PhysicalIdentification, PhysicalStockGroup, hash_occurrence, hash_text, next,
};
use crate::hub::reference::{
    HubRecordId, ReferenceCatalog, ReferenceKind, ReferenceRecord, SourceProvenance, WholePacks,
};
use crate::hub::stock::Occurrence;
use crate::hub::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};

/// Observe a separately distinguishable portion at its existing location.
/// A whole-group location move or an unlabelled digital quantity is not a split.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub packs: WholePacks,
    pub identification: PhysicalIdentification,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitPlan {
    pub stock_revision: HubRevision,
    pub location: ReferenceRecord,
    pub remainder: PhysicalStockGroup,
    pub portion: PhysicalStockGroup,
}

impl SplitCommand {
    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        stock_revision: HubRevision,
        source: &PhysicalStockGroup,
        portion_id: HubRecordId,
        recorded_at: UnixTimestamp,
    ) -> Result<SplitPlan, MovementError> {
        source.validate_catalog(catalog)?;
        if self.expected_stock_revision != stock_revision
            || self.expected_catalog_revision != catalog.revision()
            || self.group != source.id
            || self.expected_group_revision != source.revision
        {
            return Err(MovementError::StaleRevision);
        }
        if self.packs.value() == 0 || self.packs.value() >= source.packs.value() {
            return Err(MovementError::InvalidGroup);
        }
        let floor = source.observation_floor(self.occurred, recorded_at)?;
        let partition = source.partition(self.packs, portion_id, Some(&self.identification))?;
        let remainder = partition.remainder.ok_or(MovementError::InvalidGroup)?;
        let mut portion = partition.selected;
        portion.causal_floor = floor;
        portion.validate()?;
        remainder.validate()?;
        Ok(SplitPlan {
            stock_revision: next(stock_revision)?,
            location: catalog
                .require_kind(source.location, ReferenceKind::Location)?
                .clone(),
            remainder,
            portion,
        })
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, portion_id: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.portion.split.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(portion_id.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.group.as_bytes());
        hash.update(self.expected_group_revision.value().to_be_bytes());
        hash.update(self.packs.value().to_be_bytes());
        hash_text(&mut hash, self.identification.label.as_str());
        hash_text(&mut hash, self.identification.observation.as_str());
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

#[cfg(test)]
mod tests;
