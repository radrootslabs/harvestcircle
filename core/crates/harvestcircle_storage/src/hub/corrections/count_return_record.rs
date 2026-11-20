//! Full association proof reconstructs the original pre-effect cut. Its saved
//! description cannot establish current uncertainty or authorize execution.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmittedMaterial, HubSubmittedRequest};
use harvestcircle_domain::hub::corrections::{
    CountReturnAssociationCommand, CountReturnAssociationPlan,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountReturnAssociationReceipt {
    pub operation: DurableRequestId,
    pub original: CountReturnAssociationPlan,
}

pub(super) fn result_digest(
    hub: HubId,
    op: &OperationRecord,
    command: &CountReturnAssociationCommand,
    plan: &CountReturnAssociationPlan,
    admission: [u8; 32],
) -> Result<[u8; 32], SafeError> {
    let bytes = serde_json::to_vec(&(
        (
            hub,
            op.id.as_str(),
            &op.kind,
            op.actor,
            op.hash,
            op.revision,
            op.catalog_revision,
            op.result,
            op.recorded_at,
        ),
        command,
        plan,
        admission,
    ))
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count-return.association.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(in crate::hub) fn rebuild_on<'a, 'reader, 'connection>(
    tx: &'a mut OriginalReader<'reader, 'connection>,
    hub: HubId,
    op: OperationRecord,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<HubCountReturnAssociationReceipt, SafeError>>
            + Send
            + 'a,
    >,
> {
    Box::pin(async move {
        let root = Box::pin(super::count_return_ledger::read_on(tx, hub, op)).await?;
        let own = crate::hub::actuals::common::operation_id(&root.op.id)
            .map_err(|_| corrupt_storage())?;
        let facts = Box::pin(super::count_return_context::facts_on(
            tx,
            hub,
            &root.command,
        ))
        .await
        .map_err(|_| corrupt_storage())?;
        let context = super::count_return_context::Context {
            facts,
            uncertain_submissions: false,
        };
        let plan = root
            .command
            .plan(&context.domain()?, own, root.op.actor, root.op.recorded_at)
            .map_err(|_| corrupt_storage())?;
        if plan.association.stock_revision != root.op.revision
            || plan.association.id != root.op.result
            || plan.association.packs.value() != root.packs
            || crate::hub::actuals::common::operation(plan.association.count_operation)
                .map_err(|_| corrupt_storage())?
                != root.count
            || super::count_return_ledger::projection_bytes(&plan.association)? != root.projection
        {
            return Err(corrupt_storage());
        }
        let material = [HubSubmittedMaterial::CountReturnAssociation(Box::new(
            context.material(own)?,
        ))];
        let admission = Box::pin(crate::hub::submission::verify_recorded_capture_on(
            tx,
            hub,
            &root.op,
            HubSubmittedRequest::CountReturnAssociation(&root.command),
            root.command.expected_stock_revision,
            root.command.expected_catalog_revision,
            &material,
        ))
        .await?;
        if admission != root.capture
            || result_digest(hub, &root.op, &root.command, &plan, admission)? != root.result
        {
            return Err(corrupt_storage());
        }
        Ok(HubCountReturnAssociationReceipt {
            operation: root.op.id,
            original: plan,
        })
    })
}

impl Database {
    pub async fn hub_count_return_association_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountReturnAssociationReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?.id();
                    reference::validate_basis_on(tx, hub).await?;
                    let Some(op) = stock::read::operation_on(tx, hub, &operation).await? else {
                        return Ok(None);
                    };
                    if op.kind != "count_return_associate" {
                        return Err(stock::conflict());
                    }
                    let mut reader = OriginalReader::new(tx);
                    Box::pin(reader.count_return_association(hub, op))
                        .await
                        .map(|receipt| Some(receipt.as_ref().clone()))
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
