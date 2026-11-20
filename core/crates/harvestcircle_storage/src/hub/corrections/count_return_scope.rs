//! Complete immutable association evidence for one native read cut. This value
//! carries no current owner, uncertainty, admission or execution permission.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedReturnScope;
use harvestcircle_domain::hub::{corrections::CountReturnAssociation, reference::WholePacks};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Append only a descriptor produced by native proof reconstruction. The full
/// admission material comparison authenticates its presence and exact bytes.
pub(super) fn append_material(
    material: &mut Vec<harvestcircle_application::hub_submission::HubSubmittedMaterial>,
    scope: Option<&HubSubmittedReturnScope>,
    id: HubRecordId,
    hub: HubId,
    cut: HubRevision,
    catalog: HubRevision,
) -> Result<(), SafeError> {
    if let Some(scope) = scope {
        if scope.hub != hub
            || scope.stock_revision != cut
            || scope.epoch_stock_revision > cut.value()
            || scope.epoch_catalog_revision > catalog.value()
            || scope.associations > 4096
        {
            return Err(corrupt_storage());
        }
        let mut descriptor = scope.clone();
        descriptor.id = id;
        material.push(
            harvestcircle_application::hub_submission::HubSubmittedMaterial::ReturnScope(Box::new(
                descriptor,
            )),
        );
    }
    Ok(())
}

pub(in crate::hub) struct VerifiedScope {
    hub: HubId,
    cut: HubRevision,
    epoch: super::count_return_ledger::Epoch,
    digest: [u8; 32],
    returns: BTreeMap<HubRecordId, CountReturnAssociation>,
}

impl VerifiedScope {
    /// The immutable activation cut selects interpretation, including an empty
    /// new prefix. Saved material never selects whether this proof is required.
    pub(in crate::hub) fn for_cut_on<'a, 'reader, 'connection>(
        tx: &'a mut OriginalReader<'reader, 'connection>,
        hub: HubId,
        cut: HubRevision,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<Self>, SafeError>> + Send + 'a>,
    > {
        Box::pin(async move {
            if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
                return Ok(None);
            }
            let epoch = super::count_return_ledger::epoch_on(tx, hub).await?;
            if cut.value() < epoch.stock {
                return Ok(None);
            }
            Ok(Some(Self::load_on(tx, hub, cut).await?))
        })
    }

    pub(in crate::hub) async fn load_on(
        tx: &mut OriginalReader<'_, '_>,
        hub: HubId,
        cut: HubRevision,
    ) -> Result<Self, SafeError> {
        // Structural current envelope only; do not reconstruct a current
        // ledger inside a historical full-original proof. Even an empty prefix
        // must not hide damaged event cardinality or the global journal head.
        let journal = stock::read::journal_state_on(tx, hub).await?;
        if cut.value() > journal.revision.value() {
            return Err(corrupt_storage());
        }
        let epoch = super::count_return_ledger::epoch_on(tx, hub).await?;
        let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
        let (count, maximum): (i64,i64) = sqlx::query_as(
            "SELECT count(*),COALESCE(max(stock_revision),1) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?"
        ).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
        if count.checked_add(1) != Some(through) || maximum != through {
            return Err(corrupt_storage());
        }
        // Full originals share this reader's decreasing stack and fixed proof
        // certificates. Never construct this scope from saved material or JSON.
        let rows = super::count_return_history::associations_on(tx, hub, cut).await?;
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.return.count-association-scope.v1\0");
        hash.update(hub.as_bytes());
        hash.update(cut.value().to_be_bytes());
        hash.update(epoch.stock.to_be_bytes());
        hash.update(epoch.catalog.to_be_bytes());
        hash.update(
            u32::try_from(rows.len())
                .map_err(|_| corrupt_storage())?
                .to_be_bytes(),
        );
        let mut returns = BTreeMap::new();
        for row in rows {
            let id = crate::hub::actuals::common::operation(row.operation)
                .map_err(|_| corrupt_storage())?;
            let op = stock::read::operation_on(tx, hub, &id)
                .await?
                .ok_or_else(corrupt_storage)?;
            let root = super::count_return_ledger::read_on(tx, hub, op).await?;
            let bytes = super::count_return_ledger::projection_bytes(&row)?;
            if root.projection != bytes
                || root.epoch != epoch
                || root.op.revision.value() > cut.value()
            {
                return Err(corrupt_storage());
            }
            hash.update(
                u32::try_from(bytes.len())
                    .map_err(|_| corrupt_storage())?
                    .to_be_bytes(),
            );
            hash.update(bytes);
            hash.update(root.projection_hash);
            hash.update(root.result);
            hash.update(root.capture);
            if returns.insert(row.return_operation, row).is_some() {
                return Err(corrupt_storage());
            }
        }
        // Lower funding may consume new native22 Return descriptors only after
        // every prior Return's full scope/capture has shared-owner proof here.
        // Fixed certificates avoid retaining all nonselected receipt bodies.
        for op in super::count_return_history::return_operations_on(tx, hub, cut).await? {
            Box::pin(tx.return_provenance(hub, op)).await?;
        }
        Ok(Self {
            hub,
            cut,
            epoch,
            digest: hash.finalize().into(),
            returns,
        })
    }

    pub(super) fn validate_for(&self, hub: HubId, cut: HubRevision) -> Result<[u8; 32], SafeError> {
        if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 || self.hub != hub || self.cut != cut {
            return Err(corrupt_storage());
        }
        Ok(self.digest)
    }

    /// Called only after the lower owning Return path has authenticated its
    /// original. Count scope is disjoint from every normal physical debit; it
    /// supplies no packs or new allowance to that normal event.
    pub(super) fn disjoint_return(
        &self,
        operation: HubRecordId,
        revision: HubRevision,
        variant: HubRecordId,
        packs: WholePacks,
        original_digest: [u8; 32],
        already_established: bool,
    ) -> Result<bool, SafeError> {
        let Some(row) = self.returns.get(&operation) else {
            return Ok(false);
        };
        if row.return_stock_revision != revision
            || row.variant != variant
            || row.packs != packs
            || row.return_original_digest != original_digest
            || already_established
        {
            return Err(corrupt_storage());
        }
        Ok(true)
    }

    pub(in crate::hub) fn material(
        &self,
        id: HubRecordId,
    ) -> Result<HubSubmittedReturnScope, SafeError> {
        Ok(HubSubmittedReturnScope {
            id,
            hub: self.hub,
            stock_revision: self.cut,
            epoch_stock_revision: self.epoch.stock,
            epoch_catalog_revision: self.epoch.catalog,
            associations: u32::try_from(self.returns.len()).map_err(|_| corrupt_storage())?,
            evidence_digest: self.digest,
        })
    }
}
