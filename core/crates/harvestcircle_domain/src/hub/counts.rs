//! Counts are retained observations, not permission to set a balance.
//!
//! Native storage loads current scope/activity and protects recording and review.
//! A pure plan neither establishes current authority nor commits physical effects.

use super::movement::{
    MovementError, PhysicalIdentification, PhysicalStockGroup, hash_occurrence, hash_text,
};
use super::reference::{
    HubRecordId, HubReferenceError, ReferenceCatalog, ReferenceKind, ReferenceRecord,
    ReferenceValue, SourceProvenance, WholePacks,
};
use super::stock::{Occurrence, StockError};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

mod reconcile;
pub use reconcile::{CountEffect, CountPostCommand, CountPostPlan, CountWorkChange};

pub const HUB_COUNT_CAPACITY: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountError {
    Reference(HubReferenceError),
    Movement(MovementError),
    Stock(StockError),
    StaleRevision,
    RevisionExhausted,
    InvalidHistory,
    InvalidInput,
    ReviewRequired,
}
impl From<HubReferenceError> for CountError {
    fn from(value: HubReferenceError) -> Self {
        Self::Reference(value)
    }
}
impl From<MovementError> for CountError {
    fn from(value: MovementError) -> Self {
        Self::Movement(value)
    }
}
impl From<StockError> for CountError {
    fn from(value: StockError) -> Self {
        Self::Stock(value)
    }
}

/// Trusted native facts, loaded afresh; no corresponding client safety flags exist.
pub struct CountContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub group: &'a PhysicalStockGroup,
    pub latest_physical_recorded_at: UnixTimestamp,
    pub previous_reconciliation: Option<HubRecordId>,
    pub latest_observation: Option<HubRecordId>,
    pub unresolved_actual: bool,
    pub uncertain_intent: bool,
}

impl CountContext<'_> {
    fn validate(&self) -> Result<(), CountError> {
        self.group.validate_catalog(self.catalog)?;
        if self
            .group
            .causal_floor
            .is_some_and(|at| at > self.latest_physical_recorded_at)
        {
            return Err(CountError::InvalidHistory);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountScope {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub trace: HubRecordId,
    pub group: HubRecordId,
    pub group_revision: HubRevision,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub location: ReferenceRecord,
    pub recorded_packs: WholePacks,
    pub latest_physical_recorded_at: UnixTimestamp,
    pub previous_reconciliation: Option<HubRecordId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountObservation {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub recorded_revision: HubRevision,
    pub scope: CountScope,
    pub counted_packs: WholePacks,
    pub cutoff: Occurrence,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountObserveCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub counted_packs: WholePacks,
    pub cutoff: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountDifference {
    None,
    Shortage(WholePacks),
    Surplus(WholePacks),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountBlockReason {
    ScopeChanged,
    NewerReconciliation,
    NotLatestObservation,
    CutoffUncertain,
    CutoffPrecedesRecordedActivity,
    UnresolvedActual,
    UncertainPriorOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountReview {
    pub difference: CountDifference,
    pub blockers: Vec<CountBlockReason>,
}
impl CountReview {
    #[must_use]
    pub fn ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

impl CountObserveCommand {
    pub fn plan(
        &self,
        context: &CountContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        recorded_at: UnixTimestamp,
    ) -> Result<CountObservation, CountError> {
        context.validate()?;
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
            || self.group != context.group.id
            || self.expected_group_revision != context.group.revision
        {
            return Err(CountError::StaleRevision);
        }
        self.cutoff.validate(recorded_at)?;
        let variant = context
            .catalog
            .require_kind(context.group.variant, ReferenceKind::Pack)?
            .clone();
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(CountError::InvalidHistory);
        };
        let observation = CountObservation {
            id: count_record_id(operation)?,
            operation,
            recorded_revision: next(context.stock_revision)?,
            scope: CountScope {
                hub: context.hub,
                stock_revision: context.stock_revision,
                catalog_revision: context.catalog.revision(),
                trace: context.group.trace,
                group: context.group.id,
                group_revision: context.group.revision,
                product: context
                    .catalog
                    .require_kind(product, ReferenceKind::Product)?
                    .clone(),
                variant,
                location: context
                    .catalog
                    .require_kind(context.group.location, ReferenceKind::Location)?
                    .clone(),
                recorded_packs: context.group.packs,
                latest_physical_recorded_at: context.latest_physical_recorded_at,
                previous_reconciliation: context.previous_reconciliation,
            },
            counted_packs: self.counted_packs,
            cutoff: self.cutoff,
            actor,
            recorded_at,
            observation: self.observation.clone(),
        };
        observation.validate()?;
        Ok(observation)
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.count.observe.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.group.as_bytes());
        hash.update(self.expected_group_revision.value().to_be_bytes());
        hash.update(self.counted_packs.value().to_be_bytes());
        hash_occurrence(&mut hash, self.cutoff);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

impl CountObservation {
    pub fn validate(&self) -> Result<(), CountError> {
        let scope = &self.scope;
        let valid_pack = matches!(scope.variant.value, ReferenceValue::Pack { product, .. } if product == scope.product.id);
        if self.id != count_record_id(self.operation)?
            || self.recorded_revision != next(scope.stock_revision)?
            || scope.recorded_packs.value() == 0
            || !valid_pack
            || scope.product.value.kind() != ReferenceKind::Product
            || scope.location.value.kind() != ReferenceKind::Location
            || [&scope.product, &scope.variant, &scope.location]
                .iter()
                .any(|r| r.revision.value() > scope.catalog_revision.value())
            || scope.latest_physical_recorded_at > self.recorded_at
        {
            return Err(CountError::InvalidHistory);
        }
        self.cutoff
            .validate(self.recorded_at)
            .map_err(|_| CountError::InvalidHistory)
    }

    pub fn difference(&self) -> Result<CountDifference, CountError> {
        self.validate()?;
        let recorded = self.scope.recorded_packs;
        Ok(match self.counted_packs.value().cmp(&recorded.value()) {
            std::cmp::Ordering::Equal => CountDifference::None,
            std::cmp::Ordering::Less => {
                CountDifference::Shortage(recorded.checked_sub(self.counted_packs)?)
            }
            std::cmp::Ordering::Greater => {
                CountDifference::Surplus(self.counted_packs.checked_sub(recorded)?)
            }
        })
    }

    /// Checked comparison only. Posting repeats these checks in the native transaction.
    pub fn review(&self, current: &CountContext<'_>) -> Result<CountReview, CountError> {
        self.validate()?;
        current.validate()?;
        let scope = &self.scope;
        if current.stock_revision.value() < self.recorded_revision.value()
            || current.catalog.revision().value() < scope.catalog_revision.value()
            || current.latest_physical_recorded_at < scope.latest_physical_recorded_at
        {
            return Err(CountError::InvalidHistory);
        }
        let mut blockers = Vec::new();
        if current.hub != scope.hub
            || current.group.id != scope.group
            || current.group.trace != scope.trace
            || current.group.variant != scope.variant.id
            || current.group.location != scope.location.id
            || current.group.revision != scope.group_revision
            || current.group.packs != scope.recorded_packs
        {
            blockers.push(CountBlockReason::ScopeChanged);
        }
        if current.previous_reconciliation != scope.previous_reconciliation {
            blockers.push(CountBlockReason::NewerReconciliation);
        }
        if current.latest_observation != Some(self.id) {
            blockers.push(CountBlockReason::NotLatestObservation);
        }
        match self.cutoff {
            Occurrence::Known(at) if at < current.latest_physical_recorded_at => {
                blockers.push(CountBlockReason::CutoffPrecedesRecordedActivity)
            }
            Occurrence::Known(_) => (),
            _ => blockers.push(CountBlockReason::CutoffUncertain),
        }
        if current.unresolved_actual {
            blockers.push(CountBlockReason::UnresolvedActual);
        }
        if current.uncertain_intent {
            blockers.push(CountBlockReason::UncertainPriorOutcome);
        }
        Ok(CountReview {
            difference: self.difference()?,
            blockers,
        })
    }
}

pub fn count_record_id(operation: HubRecordId) -> Result<HubRecordId, CountError> {
    derived_id(
        b"harvestcircle.hub.count.observation.identity.v1\0",
        operation,
    )
}
fn derived_id(purpose: &[u8], operation: HubRecordId) -> Result<HubRecordId, CountError> {
    let mut hash = Sha256::new();
    hash.update(purpose);
    hash.update(operation.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    HubRecordId::from_bytes(bytes).map_err(CountError::from)
}
fn next(value: HubRevision) -> Result<HubRevision, CountError> {
    value.next().map_err(|_| CountError::RevisionExhausted)
}

#[cfg(test)]
mod tests;
