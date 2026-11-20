//! Native Return originals share the same decreasing immutable proof owner.
//! Fixed provenance survives body eviction; neither cache grants an effect.
use super::*;
use harvestcircle_domain::hub::{
    corrections::NormalReturnProvenance, returns::ReturnLinkResolution,
};

impl OriginalReader<'_, '_> {
    #[cfg(test)]
    pub(super) async fn test_return_key(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
    ) -> Result<Key, SafeError> {
        self.canonical_return_key(hub, supplied)
            .await
            .map(|(_, key)| key)
    }

    async fn canonical_return_key(
        &mut self,
        hub: HubId,
        supplied: &stock::read::OperationRecord,
    ) -> Result<(stock::read::OperationRecord, Key), SafeError> {
        let op = stock::read::operation_on(self.tx, hub, &supplied.id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let own = returns::read::own_on(self.tx, hub, &op).await?;
        let mode = InterpretationMode::Return {
            epoch: own.mode.map(|v| v.epoch),
            kind: own.mode.map(|v| v.kind),
        };
        let capture: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT substr(capture_hash,1,33) FROM hub_submissions WHERE operation_id=?",
        )
        .bind(op.id.as_str())
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(|_| storage_unavailable())?;
        let capture = capture
            .map(|v| v.try_into().map_err(|_| corrupt_storage()))
            .transpose()?
            .unwrap_or([0; 32]);
        let projection:Option<Vec<u8>>=sqlx::query_scalar("SELECT substr(projection_hash,1,33) FROM hub_correction_return_projections WHERE operation_id=?")
            .bind(op.id.as_str()).fetch_optional(&mut *self.tx).await.map_err(|_|storage_unavailable())?;
        let projection = projection
            .map(|v| v.try_into().map_err(|_| corrupt_storage()))
            .transpose()?;
        let seals = Seals {
            result: own.result_hash,
            capture,
            projection,
        };
        let key = Key::with_mode(hub, &op, mode, seals);
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
        Ok((op, key))
    }

    pub(in crate::hub) async fn return_provenance(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Option<NormalReturnProvenance>, SafeError> {
        let (op, key) = self.canonical_return_key(hub, &supplied).await?;
        if let Some(Certificate::Return(value)) = self.proofs.verified.get(&key) {
            #[cfg(test)]
            {
                self.proofs.certificate_hits += 1;
            }
            return Ok(value.as_deref().cloned());
        }
        Box::pin(self.return_original(hub, op)).await?;
        match self.proofs.verified.get(&key) {
            Some(Certificate::Return(value)) => Ok(value.as_deref().cloned()),
            _ => Err(corrupt_storage()),
        }
    }

    pub(in crate::hub) async fn return_original(
        &mut self,
        hub: HubId,
        supplied: stock::read::OperationRecord,
    ) -> Result<Arc<HubReturnReceipt>, SafeError> {
        let (op, key) = self.canonical_return_key(hub, &supplied).await?;
        if let Some(entry) = self.proofs.completed.get(&key) {
            #[cfg(test)]
            {
                self.proofs.hits += 1;
            }
            return match &entry.receipt {
                Receipt::Return(receipt) => Ok(Arc::clone(receipt)),
                _ => Err(corrupt_storage()),
            };
        }
        self.proofs.active.push(key.clone());
        #[cfg(test)]
        {
            self.proofs.rebuilt += 1;
        }
        let result = returns::read::rebuild_with_proofs_on(self, hub, op).await;
        if self.proofs.active.pop().as_ref() != Some(&key) {
            return Err(corrupt_storage());
        }
        let receipt = Arc::new(result?);
        let provenance =
            if receipt.original.linkage.resolution == ReturnLinkResolution::EstablishedScope {
                Some(Box::new(
                    NormalReturnProvenance::from_original(hub, &receipt.original, key.seals.result)
                        .map_err(|_| corrupt_storage())?,
                ))
            } else {
                None
            };
        if self.proofs.verified.len() >= CERTIFICATES && !self.proofs.verified.contains_key(&key) {
            return Err(corrupt_storage());
        }
        self.proofs
            .verified
            .insert(key.clone(), Certificate::Return(provenance));
        self.proofs.retain_receipt(
            key,
            Receipt::Return(Arc::clone(&receipt)),
            self.proofs.entry_limit,
            self.proofs.payload_limit,
        );
        Ok(receipt)
    }
}
