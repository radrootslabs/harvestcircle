use super::*;
use crate::hub::corrections::return_allowances;

pub(super) async fn load(
    world: &World,
    cut: HubRevision,
) -> Result<return_allowances::CapturedReturnContext, SafeError> {
    let hub = world.hub;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &target).await?.unwrap();
                let departure = handling::departure::read::original_on(tx, hub, op)
                    .await?
                    .record;
                return_allowances::load_on(tx, hub, &departure, cut).await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn native_allowance_data_keeps_legacy_none_then_seals_exact_found_credit_and_review() {
    let world = seed(false, false).await;
    insert_projection(&world, false).await;
    let old = load(&world, world.before.revision()).await.unwrap();
    assert!(old.allowances.is_none());
    assert_eq!(old.totals.len(), 1);
    assert_eq!(old.totals[0].packs.value(), 0);
    let current = load(&world, world.op.revision).await.unwrap();
    let capture = current.allowances.unwrap();
    assert_eq!(capture.stock_revision, world.op.revision);
    assert_eq!(capture.lines.len(), 1);
    let line = &capture.lines[0];
    assert_eq!(line.effective_departure.value(), 4);
    assert_eq!(line.original_debit_return_bound.value(), 4);
    assert_eq!(
        line.quantity_head.unwrap().operation,
        world.projection.birth.operation
    );
    assert!(!line.unresolved_returns);
    assert_ne!(line.funding_hash, [0; 32]);
    assert_eq!(current.totals[0].packs.value(), 0);
}

#[tokio::test]
async fn missing_initial_review_cannot_be_used_as_a_native_return_funding_credit() {
    let world = seed(false, true).await;
    insert_projection(&world, false).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        load(&world, world.op.revision).await.err().unwrap().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
}
