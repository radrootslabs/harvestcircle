//! Full recorded classification proof from immutable facts, never a decoder of
//! a Pending request into an effect or a replacement for current authority.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmittedMaterial, HubSubmittedRequest};
use harvestcircle_domain::hub::corrections::{CountClassificationCommand, CountClassificationPlan};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountClassificationReceipt {
    pub operation: DurableRequestId,
    pub original: CountClassificationPlan,
}
pub(super) fn result_digest(
    hub: HubId,
    op: &OperationRecord,
    command: &CountClassificationCommand,
    plan: &CountClassificationPlan,
    admission: [u8; 32],
) -> Result<[u8; 32], SafeError> {
    let bytes = serde_json::to_vec(&(
        hub,
        op.id.as_str(),
        &op.kind,
        op.actor,
        op.hash,
        op.revision,
        op.catalog_revision,
        op.result,
        op.recorded_at,
        command,
        plan,
        admission,
    ))
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.count.classification.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}
type OriginalFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<HubCountClassificationReceipt, SafeError>>
            + Send
            + 'a,
    >,
>;
pub(in crate::hub) fn rebuild_on<'a, 'reader, 'connection>(
    tx: &'a mut OriginalReader<'reader, 'connection>,
    hub: HubId,
    op: OperationRecord,
) -> OriginalFuture<'a> {
    Box::pin(async move {
        if op.kind != "correction_count_classify" {
            return Err(stock::conflict());
        }
        let mode = super::count_mode::original_on(tx, hub, &op).await?;
        let root = super::count_account::root_on(tx, hub, op).await?;
        let super::count_payload::Command::Classification(command) = &root.command else {
            return Err(corrupt_storage());
        };
        let own = crate::hub::actuals::common::operation_id(&root.op.id)
            .map_err(|_| corrupt_storage())?;
        let facts = Box::pin(super::count_classification::facts_on(tx, hub, command))
            .await
            .map_err(|_| corrupt_storage())?;
        let context = super::count_classification::Context {
            facts,
            uncertain_submissions: false,
        };
        let plan = command
            .plan(&context.domain(), own, root.op.actor, root.op.recorded_at)
            .map_err(|_| corrupt_storage())?;
        if plan.amendment.stock_revision != root.op.revision {
            return Err(corrupt_storage());
        }
        let material = [HubSubmittedMaterial::CountClassification(Box::new(
            context.material(own)?,
        ))];
        let admission = crate::hub::submission::verify_recorded_capture_on(
            tx,
            hub,
            &root.op,
            HubSubmittedRequest::CountClassification(command),
            command.expected_stock_revision,
            command.expected_catalog_revision,
            &material,
        )
        .await?;
        if admission != root.admission
            || admission != mode.capture
            || result_digest(hub, &root.op, command, &plan, admission)? != root.result_hash
        {
            return Err(corrupt_storage());
        }
        let history =
            super::count_account::history_on(tx, hub, &plan.count, root.op.revision).await?;
        if history.amendments.iter().find(|row| row.operation == own) != Some(&plan.amendment) {
            return Err(corrupt_storage());
        }
        let returns =
            super::count_returns::for_history_on(tx, hub, &history, root.op.revision).await?;
        if returns.unresolved != context.facts.unresolved_returns
            || history.with_returns(returns.totals)? != plan.after
        {
            return Err(corrupt_storage());
        }
        Ok(HubCountClassificationReceipt {
            operation: root.op.id,
            original: plan,
        })
    })
}
impl Database {
    pub async fn hub_count_classification_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountClassificationReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let Some(op) = stock::read::operation_on(tx, hub.id(), &operation).await?
                    else {
                        return Ok(None);
                    };
                    if op.kind != "correction_count_classify" {
                        return Err(stock::conflict());
                    }
                    let mut reader = OriginalReader::new(tx);
                    Box::pin(reader.count_classification(hub.id(), op))
                        .await
                        .map(|receipt| Some(receipt.as_ref().clone()))
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
