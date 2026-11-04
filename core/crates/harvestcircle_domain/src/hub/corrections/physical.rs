//! Exact physical-debit funding for separately identified found remainders.
//! Native adapters must qualify complete original/return/funding readers and
//! persist every new child, review decision, credit and original atomically.
use super::{
    CorrectedDepartureFamily, CorrectionError, DepartureQuantityAccount,
    DepartureQuantityAmendment, DepartureQuantityDelta, OriginalDepartureQuantity,
};
use crate::hub::movement::{PhysicalIdentification, PhysicalStockGroup};
use crate::hub::portions::{HUB_LINEAGE_GROUPS, PhysicalLineage, PhysicalLineageNode};
use crate::hub::reference::{
    HubRecordId, ReferenceCatalog, ReferenceKind, ReferenceRecord, SourceProvenance, WholePacks,
};
use crate::hub::stock::Occurrence;
use crate::hub::treatment::{
    InitialGroupReview, TreatmentStamp, TreatmentState, treatment_record_id,
};
use crate::hub::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const HUB_FOUND_ALLOCATIONS: usize = 64;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalCorrectionError {
    UnsupportedFamily,
    Capacity,
    InvalidHistory,
    InvalidScope,
    InvalidQuantity,
    Duplicate,
    ReturnConflict,
    UnresolvedReturnScope,
    StaleRevision,
    UncertainSubmission,
    IdentityConflict,
    Treatment,
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
pub struct PhysicalDebitKey {
    pub operation: HubRecordId,
    pub group: HubRecordId,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum PhysicalDebitKind {
    NormalDeparture,
    ReviewedActual,
    AdditionalQuantity,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OriginalPhysicalDebit {
    pub key: PhysicalDebitKey,
    pub kind: PhysicalDebitKind,
    pub target: OriginalDepartureQuantity,
    pub before: PhysicalStockGroup,
    pub packs: WholePacks,
    pub stock_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
}
impl OriginalPhysicalDebit {
    fn validate(&self) -> Result<(), PhysicalCorrectionError> {
        self.before
            .validate()
            .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
        if self.key.group != self.before.id
            || self.target.trace != self.before.trace
            || self.target.variant != self.before.variant
            || self.packs.value() == 0
            || self.packs.value() > self.before.packs.value()
            || self.target.packs.value() == 0
            || self.target.stock_revision == HubRevision::INITIAL
            || self.before.revision.value() >= self.stock_revision.value()
            || self
                .before
                .causal_floor
                .is_some_and(|floor| floor > self.recorded_at)
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        match self.kind {
            PhysicalDebitKind::NormalDeparture | PhysicalDebitKind::ReviewedActual => {
                if self.key.operation != self.target.operation
                    || self.key.group != self.target.group
                    || self.packs != self.target.packs
                    || self.stock_revision != self.target.stock_revision
                    || (self.kind == PhysicalDebitKind::NormalDeparture
                        && self.before.packs != self.packs)
                {
                    return Err(PhysicalCorrectionError::InvalidHistory);
                }
            }
            PhysicalDebitKind::AdditionalQuantity => {
                if self.key.operation == self.target.operation
                    || self.stock_revision.value() <= self.target.stock_revision.value()
                {
                    return Err(PhysicalCorrectionError::InvalidHistory);
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct FoundFundingCredit {
    pub operation: HubRecordId,
    pub allocation: u8,
    pub debit: PhysicalDebitKey,
    pub packs: WholePacks,
    pub stock_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
}
/// Established return reservation remains bound to its original physical debit.
/// A later additional debit may never silently fund an earlier return selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct EstablishedReturnReservation {
    pub operation: HubRecordId,
    pub debit: PhysicalDebitKey,
    pub packs: WholePacks,
    pub stock_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalDebitFunding {
    original: OriginalPhysicalDebit,
    credits: Vec<FoundFundingCredit>,
    returns: Vec<EstablishedReturnReservation>,
    unresolved_returns: bool,
    recovered: WholePacks,
    returned: WholePacks,
    available: WholePacks,
}
impl PhysicalDebitFunding {
    pub fn restore(
        original: OriginalPhysicalDebit,
        credits: Vec<FoundFundingCredit>,
        returns: Vec<EstablishedReturnReservation>,
        unresolved_returns: bool,
    ) -> Result<Self, PhysicalCorrectionError> {
        original.validate()?;
        if credits.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY
            || returns.len() > crate::hub::stock::HUB_TRACE_CAPACITY
        {
            return Err(PhysicalCorrectionError::Capacity);
        }
        let mut recovered = zero();
        let mut returned = zero();
        let mut identities = BTreeSet::new();
        let mut operations = BTreeMap::new();
        let mut revisions = BTreeMap::new();
        revisions.insert(original.stock_revision.value(), original.key.operation);
        for credit in &credits {
            if credit.debit != original.key
                || usize::from(credit.allocation) >= HUB_FOUND_ALLOCATIONS
                || credit.packs.value() == 0
                || credit.operation == original.key.operation
                || credit.operation == original.target.operation
                || credit.stock_revision.value() <= original.stock_revision.value()
                || credit.recorded_at < original.recorded_at
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }
            if !identities.insert((credit.operation, credit.allocation)) {
                return Err(PhysicalCorrectionError::Duplicate);
            }
            if operations
                .insert(
                    credit.operation,
                    (credit.stock_revision, credit.recorded_at),
                )
                .is_some_and(|prior| prior != (credit.stock_revision, credit.recorded_at))
                || revisions
                    .insert(credit.stock_revision.value(), credit.operation)
                    .is_some_and(|prior| prior != credit.operation)
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }
            recovered = add(recovered, credit.packs)?;
        }
        let mut return_operations = BTreeSet::new();
        for reservation in &returns {
            if reservation.debit != original.key
                || reservation.packs.value() == 0
                || reservation.operation == original.key.operation
                || reservation.operation == original.target.operation
                || reservation.stock_revision.value() <= original.stock_revision.value()
                || reservation.recorded_at < original.recorded_at
                || operations.contains_key(&reservation.operation)
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }
            if !return_operations.insert(reservation.operation) {
                return Err(PhysicalCorrectionError::Duplicate);
            }
            if revisions
                .insert(reservation.stock_revision.value(), reservation.operation)
                .is_some_and(|prior| prior != reservation.operation)
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }
            returned = add(returned, reservation.packs)?;
        }
        let available = original
            .packs
            .checked_sub(add(recovered, returned)?)
            .map_err(|_| PhysicalCorrectionError::ReturnConflict)?;
        Ok(Self {
            original,
            credits,
            returns,
            unresolved_returns,
            recovered,
            returned,
            available,
        })
    }
    #[must_use]
    pub const fn original(&self) -> &OriginalPhysicalDebit {
        &self.original
    }
    #[must_use]
    pub fn credits(&self) -> &[FoundFundingCredit] {
        &self.credits
    }
    #[must_use]
    pub fn returns(&self) -> &[EstablishedReturnReservation] {
        &self.returns
    }
    #[must_use]
    pub const fn recovered(&self) -> WholePacks {
        self.recovered
    }
    #[must_use]
    pub const fn returned(&self) -> WholePacks {
        self.returned
    }
    pub fn available(&self) -> Result<WholePacks, PhysicalCorrectionError> {
        if self.unresolved_returns {
            Err(PhysicalCorrectionError::UnresolvedReturnScope)
        } else {
            Ok(self.available)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct FoundStockInput {
    pub debit: PhysicalDebitKey,
    pub packs: WholePacks,
    pub location: HubRecordId,
    pub identification: PhysicalIdentification,
    /// Independent observation establishing stock outside the post-debit remainder.
    pub cohort_evidence: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct FoundStockCommand {
    pub target_operation: HubRecordId,
    pub target_group: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub allocations: Vec<FoundStockInput>,
    pub basis: SourceProvenance,
}
pub struct FoundStockContext<'a> {
    pub family: CorrectedDepartureFamily,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub latest_recorded_at: UnixTimestamp,
    pub target: &'a DepartureQuantityAccount,
    /// Complete target-line funding, including exhausted scopes and zero parents.
    pub funding: &'a [PhysicalDebitFunding],
    pub lineage: &'a PhysicalLineage,
    pub treatment: &'a TreatmentState,
    /// Complete active identity labels in the native hub, not client-selected names.
    pub active_identifications: &'a [PhysicalIdentification],
    pub uncertain_submissions: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct FoundStockAllocation {
    pub credit: FoundFundingCredit,
    pub funding: OriginalPhysicalDebit,
    pub available_before: WholePacks,
    pub group: PhysicalStockGroup,
    pub location: ReferenceRecord,
    pub cohort_evidence: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoundStockPlan {
    pub family: CorrectedDepartureFamily,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub original_target: OriginalDepartureQuantity,
    pub amendment: DepartureQuantityAmendment,
    pub allocations: Vec<FoundStockAllocation>,
    pub review_required: InitialGroupReview,
    pub after_lineage: PhysicalLineage,
    pub after_treatment: TreatmentState,
    pub basis: SourceProvenance,
}
pub fn found_stock_group_id(
    operation: HubRecordId,
    allocation: u8,
) -> Result<HubRecordId, PhysicalCorrectionError> {
    if usize::from(allocation) >= HUB_FOUND_ALLOCATIONS {
        return Err(PhysicalCorrectionError::Capacity);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.found.group.v1\0");
    hash.update(operation.as_bytes());
    hash.update([allocation]);
    HubRecordId::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| PhysicalCorrectionError::IdentityConflict)
}
fn zero() -> WholePacks {
    WholePacks::from_value(0).expect("zero packs")
}
fn add(a: WholePacks, b: WholePacks) -> Result<WholePacks, PhysicalCorrectionError> {
    a.checked_add(b)
        .map_err(|_| PhysicalCorrectionError::InvalidQuantity)
}
fn quantity_error(error: CorrectionError) -> PhysicalCorrectionError {
    match error {
        CorrectionError::Capacity => PhysicalCorrectionError::Capacity,
        CorrectionError::Duplicate => PhysicalCorrectionError::Duplicate,
        CorrectionError::ReturnConflict => PhysicalCorrectionError::ReturnConflict,
        CorrectionError::InvalidQuantity => PhysicalCorrectionError::InvalidQuantity,
        _ => PhysicalCorrectionError::InvalidHistory,
    }
}
mod found;
#[cfg(test)]
mod tests;
