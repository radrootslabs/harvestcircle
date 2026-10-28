//! Shared conserved portion mechanics. This does not move a location or credit work.
use super::*;

pub(in crate::hub) struct PhysicalPartition {
    pub(in crate::hub) remainder: Option<PhysicalStockGroup>,
    pub(in crate::hub) selected: PhysicalStockGroup,
}

impl PhysicalStockGroup {
    pub(in crate::hub) fn partition(
        &self,
        packs: WholePacks,
        child_id: HubRecordId,
        identification: Option<&PhysicalIdentification>,
    ) -> Result<PhysicalPartition, MovementError> {
        self.validate()?;
        if packs.value() == 0 || packs.value() > self.packs.value() {
            return Err(MovementError::InsufficientStock);
        }
        let mut selected = self.clone();
        let remainder = if packs != self.packs {
            let identification =
                identification.ok_or(MovementError::PhysicalIdentificationRequired)?;
            if child_id == self.id
                || self
                    .identification
                    .as_ref()
                    .is_some_and(|original| original.label == identification.label)
            {
                return Err(MovementError::IdentityConflict);
            }
            selected.id = child_id;
            selected.parent = Some(self.id);
            selected.identification = Some(identification.clone());
            selected.revision = HubRevision::INITIAL;
            let mut remainder = self.clone();
            remainder.packs = self.packs.checked_sub(packs)?;
            remainder.revision = next(self.revision)?;
            Some(remainder)
        } else {
            if identification.is_some() {
                return Err(MovementError::IdentityConflict);
            }
            selected.revision = next(self.revision)?;
            None
        };
        selected.packs = packs;
        Ok(PhysicalPartition {
            remainder,
            selected,
        })
    }

    pub(in crate::hub) fn observation_floor(
        &self,
        occurred: Occurrence,
        recorded_at: UnixTimestamp,
    ) -> Result<Option<UnixTimestamp>, MovementError> {
        occurred.validate(recorded_at)?;
        if self.causal_floor.is_some_and(|floor| match occurred {
            Occurrence::Known(at) => at < floor,
            Occurrence::Interval { latest, .. } => latest < floor,
            Occurrence::Unknown => false,
        }) {
            return Err(MovementError::Stock(StockError::InvalidOccurrence));
        }
        let observed = match occurred {
            Occurrence::Known(at) => Some(at),
            Occurrence::Interval { earliest, .. } => Some(earliest),
            Occurrence::Unknown => None,
        };
        Ok(self.causal_floor.max(observed))
    }
}
