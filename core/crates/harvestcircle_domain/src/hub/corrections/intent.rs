//! Purpose-separated correction intent. These hashes identify bounded input;
//! they neither authenticate a caller nor authorize an effect or archived replay.
use super::*;
use crate::{
    PublicKey,
    hub::{
        HubId,
        movement::{hash_occurrence, hash_text},
        stock::{hash_optional_id, hash_source},
    },
};
use sha2::{Digest, Sha256};

fn header(
    purpose: &[u8],
    hub: HubId,
    actor: PublicKey,
    operation: HubRecordId,
    stock: HubRevision,
    catalog: HubRevision,
) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(purpose);
    hash.update(hub.as_bytes());
    hash.update(actor.as_bytes());
    hash.update(operation.as_bytes());
    hash.update(stock.value().to_be_bytes());
    hash.update(catalog.value().to_be_bytes());
    hash
}
impl FoundCorrectionCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.found.recorded.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.observation_operation.as_bytes());
        hash.finalize().into()
    }
}
impl SourceAttributionCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.source.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.trace.as_bytes());
        hash_optional_id(&mut hash, self.expected_prior);
        hash_source(&mut hash, &self.source);
        hash_text(&mut hash, self.basis.as_str());
        hash.finalize().into()
    }
}
impl RecipientAttributionCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.recipient.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.departure_operation.as_bytes());
        hash_optional_id(&mut hash, self.expected_prior);
        hash_optional_id(&mut hash, self.recipient);
        hash_text(&mut hash, self.basis.as_str());
        hash.finalize().into()
    }
}
impl CorrectionFollowUpCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.follow_up.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.correction.as_bytes());
        let (kind, id) = match self.target {
            CorrectionImpact::Population(id) => (1, id),
            CorrectionImpact::Source(id) => (2, id),
            CorrectionImpact::Departure(id) => (3, id),
            CorrectionImpact::IssuedVersion(id) => (4, id),
            CorrectionImpact::Recipient(id) => (5, id),
            CorrectionImpact::UnresolvedSource(id) => (6, id),
            CorrectionImpact::UnresolvedRecipient(id) => (7, id),
        };
        hash.update([kind]);
        hash.update(id.as_bytes());
        hash_optional_id(&mut hash, self.expected_prior);
        hash.update([match self.state {
            CorrectionManualWork::Outstanding => 0,
            CorrectionManualWork::WorkRecorded => 1,
        }]);
        hash_text(&mut hash, self.basis.as_str());
        hash.finalize().into()
    }
}
impl AdditionalQuantityCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let facts = &self.facts;
        let mut hash = header(
            b"harvestcircle.hub.correction.additional.command.v2\0",
            hub,
            actor,
            operation,
            facts.expected_stock_revision,
            facts.expected_catalog_revision,
        );
        hash.update(facts.report.as_bytes());
        hash.update(facts.target_operation.as_bytes());
        hash.update(facts.target_group.as_bytes());
        hash_optional_id(&mut hash, facts.expected_prior);
        hash.update(facts.corrected_line_packs.value().to_be_bytes());
        hash_optional_id(&mut hash, facts.recipient);
        hash_occurrence(&mut hash, facts.occurred);
        for evidence in [
            &facts.source_evidence,
            &facts.quantity_evidence,
            &facts.recipient_evidence,
            &facts.timing_evidence,
        ] {
            hash_text(&mut hash, evidence.as_str());
        }
        hash.update(self.source_operation.as_bytes());
        hash.update(self.source_group.as_bytes());
        hash.update(self.balancing_group.as_bytes());
        hash.update(self.expected_balancing_revision.value().to_be_bytes());
        hash_text(&mut hash, self.duplicate_review_basis.as_str());
        hash.finalize().into()
    }
}
impl CountExplanationFacts {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.count_explain.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.report.as_bytes());
        hash.update(self.count_reconciliation.as_bytes());
        hash_optional_id(&mut hash, self.expected_prior);
        hash.update(self.packs.value().to_be_bytes());
        hash.update(self.recipient.as_bytes());
        hash_occurrence(&mut hash, self.occurred);
        for evidence in [
            &self.source_evidence,
            &self.quantity_evidence,
            &self.recipient_evidence,
            &self.timing_evidence,
        ] {
            hash_text(&mut hash, evidence.as_str());
        }
        hash.finalize().into()
    }
}
impl CountExplanationCommand {
    /// Version two binds the explicit historical source. The old facts-only
    /// fingerprint remains immutable and is never a complete native command.
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.correction.count_explain.command.v2\0");
        hash.update(self.facts.fingerprint(hub, actor, operation));
        hash.update(self.source_operation.as_bytes());
        hash.update(self.source_group.as_bytes());
        hash.finalize().into()
    }
}
impl FoundStockCommand {
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        operation: HubRecordId,
    ) -> Result<[u8; 32], CorrectionObservationError> {
        if self.allocations.is_empty() || self.allocations.len() > HUB_FOUND_ALLOCATIONS {
            return Err(CorrectionObservationError::Capacity);
        }
        let mut hash = header(
            b"harvestcircle.hub.correction.found.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        // Reuse the complete bounded observed-facts encoder; its own purpose is
        // nested beneath this distinct effect intent, never substituted for it.
        let facts = CorrectionObservationCommand {
            previous: None,
            facts: CorrectionObservedFacts::Found {
                target_operation: self.target_operation,
                target_group: self.target_group,
                allocations: self.allocations.clone(),
                basis: self.basis.clone(),
            },
        };
        hash.update(facts.fingerprint(hub, actor, operation)?);
        Ok(hash.finalize().into())
    }
}

#[cfg(test)]
mod tests;

impl CountClassificationCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = header(
            b"harvestcircle.hub.correction.count_classify.recorded.command.v1\0",
            hub,
            actor,
            operation,
            self.expected_stock_revision,
            self.expected_catalog_revision,
        );
        hash.update(self.observation_operation.as_bytes());
        hash_optional_id(&mut hash, self.expected_account_prior);
        hash_optional_id(&mut hash, self.expected_prior);
        hash.finalize().into()
    }
}
