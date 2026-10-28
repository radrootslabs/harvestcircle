use super::*;
use crate::hub::dispatch::{HUB_DISPATCH_LINES, IdentificationStatus};

/// Positive original scope followed by complete physical exit. There is no
/// fabricated zero-pack PhysicalStockGroup; native storage retains its ancestry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumedStockGroup {
    pub before: PhysicalStockGroup,
    pub after_revision: HubRevision,
    pub causal_floor: Option<UnixTimestamp>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepartureWorkChange {
    pub before: WorkRequirement,
    pub after: WorkRequirement,
    pub retired: WholePacks,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepartureRecord {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
    /// Exact reviewed paper history before use, never current replacement data.
    pub paper: DispatchRecord,
    pub consumed: Vec<ConsumedStockGroup>,
    pub work_changes: Vec<DepartureWorkChange>,
}
impl DepartureRecord {
    pub fn validate(&self) -> Result<(), DepartureError> {
        self.paper.validate()?;
        let issued = self
            .paper
            .versions
            .last()
            .ok_or(DepartureError::IdentificationUnavailable)?;
        if self.id != DepartureIdentities::from_operation_bytes(self.operation)?.departure
            || self.hub != self.paper.hub
            || self.paper.draft.is_some()
            || issued.status != IdentificationStatus::Issued
            || issued.closed.is_some()
            || self.stock_revision.value() <= self.paper.updated.stock_revision.value()
            || self.catalog_revision.value() < issued.material.catalog_revision.value()
            || self.consumed.is_empty()
            || self.consumed.len() > HUB_DISPATCH_LINES
            || self.consumed.len() != issued.material.lines.len()
        {
            return Err(DepartureError::InvalidHistory);
        }
        self.occurred
            .validate(self.recorded_at)
            .map_err(|_| DepartureError::InvalidHistory)?;
        for (effect, line) in self.consumed.iter().zip(&issued.material.lines) {
            let group = &effect.before;
            group.validate()?;
            if group.id != line.group
                || group.revision != line.group_revision
                || group.trace != line.trace.id
                || group.variant != line.variant.id
                || group.location != line.location.id
                || group.packs != line.packs
                || group.identification != line.identification
                || effect.after_revision != next(group.revision)?
                || group.revision.value() >= self.stock_revision.value()
                || effect.causal_floor
                    != group.observation_floor(self.occurred, self.recorded_at)?
            {
                return Err(DepartureError::InvalidHistory);
            }
        }
        if self.work_changes != retired_work(&self.consumed)? {
            return Err(DepartureError::InvalidHistory);
        }
        Ok(())
    }
    #[must_use]
    pub fn physical_event_count(&self) -> usize {
        self.consumed.len()
    }
}
