//! Nonphysical correction observations. Preservation validates bounded syntax,
//! never factual source/funding, current stock, effect eligibility or authority.
use super::{FoundStockInput, HUB_FOUND_ALLOCATIONS};
use crate::hub::movement::hash_text;
use crate::hub::reference::{HubRecordId, SourceProvenance, WholePacks};
use crate::hub::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

pub const HUB_CORRECTION_OBSERVATION_CAPACITY: usize = 4_096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrectionObservationError {
    Capacity,
    InvalidIdentity,
    InvalidHistory,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "facts", rename_all = "snake_case")]
pub enum CorrectionObservedFacts {
    Found {
        target_operation: HubRecordId,
        target_group: HubRecordId,
        allocations: Vec<FoundStockInput>,
        basis: SourceProvenance,
    },
    CountClassification {
        explanation_operation: HubRecordId,
        corrected_packs: WholePacks,
        /// Optional existing positive departure report; zero is retained here
        /// directly, never disguised as an invented nullable departure report.
        report: Option<HubRecordId>,
        basis: SourceProvenance,
    },
}
impl CorrectionObservedFacts {
    #[must_use]
    pub const fn family(&self) -> &'static str {
        match self {
            Self::Found { .. } => "found",
            Self::CountClassification { .. } => "count_classification",
        }
    }
    #[must_use]
    pub const fn target(&self) -> HubRecordId {
        match self {
            Self::Found {
                target_operation, ..
            } => *target_operation,
            Self::CountClassification {
                explanation_operation,
                ..
            } => *explanation_operation,
        }
    }
    #[must_use]
    pub const fn group(&self) -> Option<HubRecordId> {
        match self {
            Self::Found { target_group, .. } => Some(*target_group),
            Self::CountClassification { .. } => None,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectionObservationCommand {
    /// A known recorded predecessor is retained, never overwritten. Native
    /// storage must enforce one latest unconsumed version in the same hub/family.
    pub previous: Option<HubRecordId>,
    pub facts: CorrectionObservedFacts,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectionObservationRecord {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub actor: PublicKey,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
    pub observation: CorrectionObservationCommand,
}
impl CorrectionObservationCommand {
    pub fn validate(&self) -> Result<(), CorrectionObservationError> {
        if let CorrectionObservedFacts::Found { allocations, .. } = &self.facts
            && (allocations.is_empty() || allocations.len() > HUB_FOUND_ALLOCATIONS)
        {
            return Err(CorrectionObservationError::Capacity);
        }
        // Duplicate labels, zero quantities, unknown native targets and invalid
        // funding are factual review outcomes, not reasons to discard a claim.
        Ok(())
    }
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        operation: HubRecordId,
    ) -> Result<[u8; 32], CorrectionObservationError> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.correction.observation.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        optional_id(&mut hash, self.previous);
        match &self.facts {
            CorrectionObservedFacts::Found {
                target_operation,
                target_group,
                allocations,
                basis,
            } => {
                hash.update(b"found\0");
                hash.update(target_operation.as_bytes());
                hash.update(target_group.as_bytes());
                hash.update(
                    u32::try_from(allocations.len())
                        .map_err(|_| CorrectionObservationError::Capacity)?
                        .to_be_bytes(),
                );
                for allocation in allocations {
                    hash.update(allocation.debit.operation.as_bytes());
                    hash.update(allocation.debit.group.as_bytes());
                    hash.update(allocation.packs.value().to_be_bytes());
                    hash.update(allocation.location.as_bytes());
                    hash_text(&mut hash, allocation.identification.label.as_str());
                    hash_text(&mut hash, allocation.identification.observation.as_str());
                    hash_text(&mut hash, allocation.cohort_evidence.as_str());
                }
                hash_text(&mut hash, basis.as_str());
            }
            CorrectionObservedFacts::CountClassification {
                explanation_operation,
                corrected_packs,
                report,
                basis,
            } => {
                hash.update(b"count_classification\0");
                hash.update(explanation_operation.as_bytes());
                hash.update(corrected_packs.value().to_be_bytes());
                optional_id(&mut hash, *report);
                hash_text(&mut hash, basis.as_str());
            }
        }
        Ok(hash.finalize().into())
    }
    pub fn preserve(
        &self,
        hub: HubId,
        stock: HubRevision,
        catalog: HubRevision,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CorrectionObservationRecord, CorrectionObservationError> {
        self.validate()?;
        if self.previous == Some(operation) {
            return Err(CorrectionObservationError::InvalidIdentity);
        }
        Ok(CorrectionObservationRecord {
            id: correction_observation_id(operation)?,
            operation,
            hub,
            actor,
            stock_revision: stock
                .next()
                .map_err(|_| CorrectionObservationError::InvalidHistory)?,
            catalog_revision: catalog,
            recorded_at: now,
            observation: self.clone(),
        })
    }
}
pub fn correction_observation_id(
    operation: HubRecordId,
) -> Result<HubRecordId, CorrectionObservationError> {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.observation.identity.v1\0");
    hash.update(operation.as_bytes());
    HubRecordId::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| CorrectionObservationError::InvalidIdentity)
}
pub fn correction_observation_result_digest(
    input: &[u8; 32],
    record: &CorrectionObservationRecord,
) -> Result<[u8; 32], CorrectionObservationError> {
    if record.id != correction_observation_id(record.operation)?
        || record.stock_revision == HubRevision::INITIAL
        || record.observation.previous == Some(record.operation)
        || record
            .observation
            .fingerprint(record.hub, record.actor, record.operation)?
            != *input
    {
        return Err(CorrectionObservationError::InvalidHistory);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.observation.result.v1\0");
    hash.update(input);
    hash.update(record.id.as_bytes());
    hash.update(record.operation.as_bytes());
    hash.update(record.hub.as_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    Ok(hash.finalize().into())
}
fn optional_id(hash: &mut Sha256, id: Option<HubRecordId>) {
    hash.update([u8::from(id.is_some())]);
    if let Some(id) = id {
        hash.update(id.as_bytes());
    }
}

#[cfg(test)]
mod tests;
