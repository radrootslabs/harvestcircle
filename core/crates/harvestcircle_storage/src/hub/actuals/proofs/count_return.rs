//! Association proofs share the same decreasing active stack and body budget.
use super::*;
use harvestcircle_domain::hub::corrections::CountReturnAssociation;

impl OriginalReader<'_, '_> {
    async fn canonical_count_return_key(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
    ) -> Result<(stock::read::OperationRecord, Key), SafeError> {
        let op = stock::read::operation_on(self.tx, hub, &supplied.id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let root = count_return_ledger::read_on(self.tx, hub, op).await?;
        let mode = InterpretationMode::CountReturn(root.epoch);
        let seals = Seals {
            result: root.result,
            capture: root.capture,
            projection: Some(root.projection_hash),
        };
        let key = Key::with_mode(hub, &root.op, mode, seals);
        if Key::with_mode(hub, supplied, mode, seals) != key
            || self
                .proofs
                .active
                .last()
                .is_some_and(|parent| parent.hub != key.hub || key.stock >= parent.stock)
            || self.proofs.active.len() >= CERTIFICATES
        {
            return Err(corrupt_storage());
        }
        Ok((root.op, key))
    }

    pub(in crate::hub) async fn count_return_projection(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<CountReturnAssociation, SafeError> {
        let (op, key) = self.canonical_count_return_key(hub, &supplied).await?;
        if let Some(Certificate::CountReturn(record)) = self.proofs.verified.get(&key) {
            #[cfg(test)]
            {
                self.proofs.certificate_hits += 1;
            }
            return Ok(record.as_ref().clone());
        }
        let receipt = Box::pin(self.count_return_association(hub, op)).await?;
        Ok(receipt.original.association.clone())
    }

    pub(in crate::hub) async fn count_return_association(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubCountReturnAssociationReceipt>, SafeError> {
        let (op, key) = self.canonical_count_return_key(hub, &supplied).await?;
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return match &entry.receipt {
                Receipt::CountReturn(receipt) => Ok(Arc::clone(receipt)),
                _ => Err(corrupt_storage()),
            };
        }
        self.proofs.active.push(key.clone());
        #[cfg(test)]
        {
            self.proofs.rebuilt += 1;
        }
        let result = count_return_record::rebuild_on(self, hub, op).await;
        if self.proofs.active.pop().as_ref() != Some(&key) {
            return Err(corrupt_storage());
        }
        let receipt = Arc::new(result?);
        if self.proofs.verified.len() >= CERTIFICATES && !self.proofs.verified.contains_key(&key) {
            return Err(corrupt_storage());
        }
        self.proofs.verified.insert(
            key.clone(),
            Certificate::CountReturn(Box::new(receipt.original.association.clone())),
        );
        self.proofs.retain_receipt(
            key,
            Receipt::CountReturn(Arc::clone(&receipt)),
            self.proofs.entry_limit,
            self.proofs.payload_limit,
        );
        Ok(receipt)
    }
}
