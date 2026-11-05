//! Canonical count explanation originals. The immutable admission comparison
//! authenticates captured uncertainty; saved input never supplies execution.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmittedMaterial, HubSubmittedRequest};
use harvestcircle_domain::hub::{
    actuals::CountExplanationSourceWitness,
    corrections::{CountExplanationCommand, CountExplanationPlan},
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountExplanationReceipt {
    pub operation: DurableRequestId,
    pub original: CountExplanationPlan,
    pub source: CountExplanationSourceWitness,
}
pub(super) fn result_digest(
    hub: HubId,
    op: &OperationRecord,
    command: &CountExplanationCommand,
    plan: &CountExplanationPlan,
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
    hash.update(b"harvestcircle.hub.correction.count.explanation.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}
// Erase the recursive future at the full-proof boundary. Count and actual
// originals share decreasing native dependencies; their compiler Send proof
// must not recursively expand every cross-family future type.
pub(in crate::hub) fn rebuild_on<'a, 'reader, 'connection>(
    tx: &'a mut OriginalReader<'reader, 'connection>,
    hub: HubId,
    op: OperationRecord,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<HubCountExplanationReceipt, SafeError>> + Send + 'a,
    >,
> {
    Box::pin(async move {
        if op.kind != "correction_count_explain" {
            return Err(stock::conflict());
        }
        let mode = super::count_mode::original_on(tx, hub, &op).await?;
        let root = super::count_account::root_on(tx, hub, op).await?;
        let super::count_payload::Command::Explanation(command) = &root.command else {
            return Err(corrupt_storage());
        };
        let own = crate::hub::actuals::common::operation_id(&root.op.id)
            .map_err(|_| corrupt_storage())?;
        let facts = Box::pin(super::count_context::facts_on(tx, hub, command))
            .await
            .map_err(|_| corrupt_storage())?;
        // This false value is accepted only after the complete material matches
        // the original immutable capture below. No mutable Pending state is read.
        let context = super::count_context::Context {
            facts,
            uncertain_submissions: false,
        };
        let plan = command
            .plan(&context.domain(), own, root.op.actor, root.op.recorded_at)
            .map_err(|_| corrupt_storage())?;
        if plan.explanation.stock_revision != root.op.revision {
            return Err(corrupt_storage());
        }
        let material = [HubSubmittedMaterial::CountExplanation(Box::new(
            super::count_context::material(&context, command, own)?,
        ))];
        let admission = crate::hub::submission::verify_recorded_capture_on(
            tx,
            hub,
            &root.op,
            HubSubmittedRequest::CountExplanation(command),
            command.facts.expected_stock_revision,
            command.facts.expected_catalog_revision,
            &material,
        )
        .await?;
        if admission != root.admission
            || admission != mode.capture
            || result_digest(hub, &root.op, command, &plan, admission)? != root.result_hash
        {
            return Err(corrupt_storage());
        }
        // Verify this row's scalar projection independently of the full context.
        // Its account contains the original positive event even after later zero.
        let history =
            super::count_account::history_on(tx, hub, &plan.count, root.op.revision).await?;
        if history.explanations.iter().find(|e| e.operation == own) != Some(&plan.explanation) {
            return Err(corrupt_storage());
        }
        let source = CountExplanationSourceWitness::from_explanation(&plan)
            .map_err(|_| corrupt_storage())?;
        Ok(HubCountExplanationReceipt {
            operation: root.op.id,
            original: plan,
            source,
        })
    })
}
impl Database {
    pub async fn hub_count_explanation_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountExplanationReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let Some(op) = stock::read::operation_on(tx, hub.id(), &operation).await?
                    else {
                        return Ok(None);
                    };
                    if op.kind != "correction_count_explain" {
                        return Err(stock::conflict());
                    }
                    let mut reader = OriginalReader::new(tx);
                    Box::pin(reader.count_explanation(hub.id(), op))
                        .await
                        .map(|receipt| Some(receipt.as_ref().clone()))
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
