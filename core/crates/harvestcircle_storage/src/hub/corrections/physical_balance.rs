//! Cut-specific net physical outflow. Gross originals/events remain unchanged;
//! found credits are deducted exactly once from their own original physical debit.
//! No current lineage or full correction replanner is called from this layer.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{DepartureQuantityDelta, PhysicalDebitKey},
    reference::WholePacks,
};

pub(in crate::hub) async fn original_net_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
    group: HubRecordId,
    gross: WholePacks,
    cut: HubRevision,
) -> Result<WholePacks, SafeError> {
    // Full immutable proofs compose through several ordinary readers. Keep
    // their future storage on the heap while retaining every proof/check.
    let line = Box::pin(super::quantity::original_on(tx, hub, operation, group, cut)).await?;
    if line.original.packs != gross {
        return Err(corrupt_storage());
    }
    let history = Box::pin(super::quantity::history_from_original_on(
        tx, hub, operation, group, cut, &line,
    ))
    .await?;
    // Scalar additional history cannot qualify a separate physical debit.
    if history
        .amendments
        .iter()
        .any(|a| matches!(a.delta, DepartureQuantityDelta::AdditionalDeparture(_)))
    {
        return Err(corrupt_storage());
    }
    let key = PhysicalDebitKey {
        operation: line.original.operation,
        group,
    };
    let mut recovered = WholePacks::from_value(0).expect("zero");
    for credit in Box::pin(super::return_allowances::credits_on(
        tx,
        hub,
        &line,
        &history.amendments,
    ))
    .await?
    {
        if credit.debit != key || credit.stock_revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        recovered = recovered
            .checked_add(credit.packs)
            .map_err(|_| corrupt_storage())?;
    }
    // Returned populations do not deduct this original outflow again.
    gross.checked_sub(recovered).map_err(|_| corrupt_storage())
}
