//! Actual non-buyer removal of an explicitly identified complete physical group.
//!
//! Partial removal first requires ordinary observed separation. This does not
//! create a departure, recipient delivery, count loss, hold release or refund.
//! Native current authority, custody, uncertainty and atomic persistence remain
//! prerequisites; a pure record is not an execution grant.
use super::departure::{ConsumedStockGroup, DepartureError, DepartureWorkChange, retired_work};
use super::movement::{MovementError, PhysicalStockGroup, hash_occurrence, hash_text};
use super::portions::PhysicalLineage;
use super::reference::{
    HubRecordId, ReferenceCatalog, ReferenceKind, ReferenceRecord, ReferenceValue,
    SourceProvenance, WholePacks,
};
use super::stock::Occurrence;
use super::treatment::{PhysicalAction, TreatmentError, TreatmentSelection, TreatmentState};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionKind {
    Destroyed,
    OtherObservedRemoval,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispositionError {
    InvalidIdentity,
    InvalidNativeContext,
    InvalidHistory,
    StaleRevision,
    PhysicalSeparationRequired,
    UnresolvedActivity,
    RevisionExhausted,
    Movement(MovementError),
    Treatment(TreatmentError),
    Work(DepartureError),
}
impl From<MovementError> for DispositionError {
    fn from(value: MovementError) -> Self {
        Self::Movement(value)
    }
}
impl From<TreatmentError> for DispositionError {
    fn from(value: TreatmentError) -> Self {
        Self::Treatment(value)
    }
}
impl From<DepartureError> for DispositionError {
    fn from(value: DepartureError) -> Self {
        Self::Work(value)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispositionIdentities {
    pub operation: HubRecordId,
    pub disposition: HubRecordId,
}
impl DispositionIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, DispositionError> {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.disposition.identity.v1\0");
        hash.update(operation.as_bytes());
        Ok(Self {
            operation,
            disposition: HubRecordId::from_bytes(
                hash.finalize()[..16].try_into().expect("SHA256 prefix"),
            )
            .map_err(|_| DispositionError::InvalidIdentity)?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispositionCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub packs: WholePacks,
    pub kind: DispositionKind,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}
/// Derived from current native submission/actual-review sources, never UI clear flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispositionActivity {
    pub uncertain_submissions: bool,
    pub unresolved_actuals: bool,
}
pub struct DispositionContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub group: &'a PhysicalStockGroup,
    pub lineage: &'a PhysicalLineage,
    pub treatment: &'a TreatmentState,
    pub activity: DispositionActivity,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionRecord {
    pub identities: DispositionIdentities,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub location: ReferenceRecord,
    pub kind: DispositionKind,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
    /// No fabricated zero group: retain exact before scope and its next version.
    pub consumed: ConsumedStockGroup,
    /// Only pending work retires; other historical progress stays in consumed.before.
    pub work_changes: Vec<DepartureWorkChange>,
}
impl DispositionCommand {
    pub fn plan(
        &self,
        context: &DispositionContext<'_>,
        ids: DispositionIdentities,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<DispositionRecord, DispositionError> {
        if ids != DispositionIdentities::from_operation_bytes(ids.operation)?
            || ids.operation == ids.disposition
        {
            return Err(DispositionError::InvalidIdentity);
        }
        let group = context.group;
        group.validate_catalog(context.catalog)?;
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
            || self.group != group.id
            || self.expected_group_revision != group.revision
        {
            return Err(DispositionError::StaleRevision);
        }
        if self.packs != group.packs {
            return Err(DispositionError::PhysicalSeparationRequired);
        }
        if context.activity.uncertain_submissions || context.activity.unresolved_actuals {
            return Err(DispositionError::UnresolvedActivity);
        }
        let graph = context.lineage;
        let node = graph
            .node(group.id)
            .ok_or(DispositionError::InvalidNativeContext)?;
        if graph.hub() != context.hub
            || graph.trace() != group.trace
            || graph.variant() != group.variant
            || graph.revision() != context.stock_revision
            || node.parent != group.parent
            || node.packs != group.packs
            || node.trace != group.trace
            || node.variant != group.variant
        {
            return Err(DispositionError::InvalidNativeContext);
        }
        context
            .treatment
            .project(graph, context.catalog.handling_basis())?
            .require(
                &TreatmentSelection::new(vec![group.id])?,
                PhysicalAction::Disposition,
            )?;
        let reference = |id, kind| {
            context
                .catalog
                .require_kind(id, kind)
                .cloned()
                .map_err(|_| DispositionError::InvalidNativeContext)
        };
        let variant = reference(group.variant, ReferenceKind::Pack)?;
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(DispositionError::InvalidNativeContext);
        };
        let consumed = ConsumedStockGroup {
            before: group.clone(),
            after_revision: group
                .revision
                .next()
                .map_err(|_| DispositionError::RevisionExhausted)?,
            causal_floor: group.observation_floor(self.occurred, now)?,
        };
        let record = DispositionRecord {
            identities: ids,
            hub: context.hub,
            stock_revision: context
                .stock_revision
                .next()
                .map_err(|_| DispositionError::RevisionExhausted)?,
            catalog_revision: context.catalog.revision(),
            product: reference(product, ReferenceKind::Product)?,
            variant,
            location: reference(group.location, ReferenceKind::Location)?,
            kind: self.kind,
            actor,
            recorded_at: now,
            occurred: self.occurred,
            observation: self.observation.clone(),
            work_changes: retired_work(std::slice::from_ref(&consumed))?,
            consumed,
        };
        record.validate()?;
        Ok(record)
    }
    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        ids: DispositionIdentities,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.disposition.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(ids.operation.as_bytes());
        hash.update(ids.disposition.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.group.as_bytes());
        hash.update(self.expected_group_revision.value().to_be_bytes());
        hash.update(self.packs.value().to_be_bytes());
        hash.update([match self.kind {
            DispositionKind::Destroyed => 1,
            DispositionKind::OtherObservedRemoval => 2,
        }]);
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
impl DispositionRecord {
    pub fn validate(&self) -> Result<(), DispositionError> {
        self.consumed.before.validate()?;
        let group = &self.consumed.before;
        if self.identities
            != DispositionIdentities::from_operation_bytes(self.identities.operation)?
            || self.identities.operation == self.identities.disposition
            || group.revision.value() >= self.stock_revision.value()
            || self.variant.id != group.variant
            || self.location.id != group.location
            || self.product.value.kind() != ReferenceKind::Product
            || self.location.value.kind() != ReferenceKind::Location
            || !matches!(self.variant.value,ReferenceValue::Pack{product,..}if product==self.product.id)
            || [
                self.product.revision,
                self.variant.revision,
                self.location.revision,
            ]
            .iter()
            .any(|r| r.value() > self.catalog_revision.value())
            || self.consumed.after_revision
                != group
                    .revision
                    .next()
                    .map_err(|_| DispositionError::RevisionExhausted)?
            || self.consumed.causal_floor
                != group.observation_floor(self.occurred, self.recorded_at)?
            || self.work_changes != retired_work(std::slice::from_ref(&self.consumed))?
        {
            return Err(DispositionError::InvalidHistory);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
