//! Complete cut-specific return bounds. No original physical debit is expanded
//! by an additional debit elsewhere, and uncertainty never becomes allocation.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{DepartureQuantityAmendment, DepartureQuantityDelta, PhysicalDebitKey},
    departure::DepartureRecord,
    reference::WholePacks,
    returns::{
        EstablishedReturnTotal, ReturnAllowanceCapture, ReturnLineAllowance, ReturnQuantityHead,
    },
    stock::HUB_STOCK_EVENT_CAPACITY,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(in crate::hub) struct CapturedReturnContext {
    pub totals: Vec<EstablishedReturnTotal>,
    pub allowances: Option<ReturnAllowanceCapture>,
}
#[derive(Serialize)]
pub(super) struct Credit {
    pub(super) operation: HubRecordId,
    pub(super) stock_revision: HubRevision,
    pub(super) allocation: u8,
    pub(super) child: HubRecordId,
    pub(super) debit: PhysicalDebitKey,
    pub(super) packs: WholePacks,
    pub(super) projection_hash: [u8; 32],
    pub(super) initial_review_hash: [u8; 32],
}

pub(super) async fn credits_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    line: &quantity::CanonicalLine,
    changes: &[DepartureQuantityAmendment],
) -> Result<Vec<Credit>, SafeError> {
    let mut credits = Vec::new();
    for change in changes {
        let DepartureQuantityDelta::IdentifiedFoundRemainder(expected) = change.delta else {
            continue;
        };
        let id = crate::hub::actuals::common::operation(change.operation)
            .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "correction_found" || op.revision != change.stock_revision {
            return Err(corrupt_storage());
        }
        let graph = Box::pin(handling::lineage::historical_on(
            tx,
            hub,
            line.original.trace,
            op.revision,
        ))
        .await?;
        super::initial_review::original_on(tx, hub, &graph, &op).await?;
        let review_hash:Vec<u8>=sqlx::query_scalar("SELECT substr(result_hash,1,33) FROM hub_correction_initial_reviews WHERE operation_id=?").bind(id.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        let review_hash: [u8; 32] = review_hash.try_into().map_err(|_| corrupt_storage())?;
        let rows=sqlx::query("SELECT allocation,substr(child_group_id,1,17) AS child,substr(projection_hash,1,33) AS projection_hash FROM hub_correction_found_allocations WHERE operation_id=? ORDER BY allocation LIMIT 65")
            .bind(id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if rows.is_empty()
            || rows.len() > 64
            || credits
                .len()
                .checked_add(rows.len())
                .is_none_or(|n| n > HUB_STOCK_EVENT_CAPACITY)
        {
            return Err(corrupt_storage());
        }
        let mut total = WholePacks::from_value(0).expect("zero");
        for (n, row) in rows.into_iter().enumerate() {
            let child = stock::read::record_id(&row, "child")?;
            let birth = Box::pin(super::birth::birth_on(tx, hub, &op, child)).await?;
            if row
                .try_get::<i64, _>("allocation")
                .map_err(|_| corrupt_storage())?
                != i64::try_from(n).map_err(|_| corrupt_storage())?
                || birth.target_operation != line.original.operation
                || birth.target_group != line.original.group
            {
                return Err(corrupt_storage());
            }
            let hash: Vec<u8> = row
                .try_get("projection_hash")
                .map_err(|_| corrupt_storage())?;
            total = total
                .checked_add(birth.group.packs)
                .map_err(|_| corrupt_storage())?;
            credits.push(Credit {
                operation: change.operation,
                stock_revision: op.revision,
                allocation: birth.allocation,
                child,
                debit: PhysicalDebitKey {
                    operation: birth.recovery.debit_operation,
                    group: birth.group.parent.ok_or_else(corrupt_storage)?,
                },
                packs: birth.group.packs,
                projection_hash: hash.try_into().map_err(|_| corrupt_storage())?,
                initial_review_hash: review_hash,
            });
        }
        if total != expected {
            return Err(corrupt_storage());
        }
    }
    Ok(credits)
}

pub(in crate::hub) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    departure: &DepartureRecord,
    cut: HubRevision,
) -> Result<CapturedReturnContext, SafeError> {
    let issued = departure
        .paper
        .versions
        .last()
        .ok_or_else(corrupt_storage)?;
    if departure.hub != hub
        || departure.stock_revision.value() > cut.value()
        || issued.material.lines.is_empty()
        || issued.material.lines.len() > 64
    {
        return Err(corrupt_storage());
    }
    let target = crate::hub::actuals::common::operation(departure.operation)
        .map_err(|_| corrupt_storage())?;
    let mut totals = Vec::with_capacity(issued.material.lines.len());
    let mut allowances = Vec::with_capacity(issued.material.lines.len());
    let mut changed = false;
    for original in &issued.material.lines {
        let line = quantity::original_on(tx, hub, &target, original.group, cut).await?;
        if line.original.packs != original.packs
            || line.original.trace != original.trace.id
            || line.original.variant != original.variant.id
        {
            return Err(corrupt_storage());
        }
        let history =
            quantity::history_from_original_on(tx, hub, &target, original.group, cut, &line)
                .await?;
        let changes = history.amendments.clone();
        let returned = funding::returns_on(tx, hub, &line, cut).await?;
        let account = history.with_verified_returns(returned.total)?;
        let credits = credits_on(tx, hub, &line, &changes).await?;
        let debit = PhysicalDebitKey {
            operation: line.original.operation,
            group: line.original.group,
        };
        let recovered = credits
            .iter()
            .filter(|credit| credit.debit == debit)
            .try_fold(WholePacks::from_value(0).expect("zero"), |sum, credit| {
                sum.checked_add(credit.packs)
            })
            .map_err(|_| corrupt_storage())?;
        let physical = line
            .original
            .packs
            .checked_sub(recovered)
            .map_err(|_| corrupt_storage())?;
        if returned.total.value() > physical.value() {
            return Err(corrupt_storage());
        }
        let mut seals = Vec::with_capacity(changes.len());
        for change in &changes {
            let id = crate::hub::actuals::common::operation(change.operation)
                .map_err(|_| corrupt_storage())?;
            let hash:Vec<u8>=sqlx::query_scalar("SELECT substr(projection_hash,1,33) FROM hub_correction_quantity_changes WHERE operation_id=?").bind(id.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            seals.push(<[u8; 32]>::try_from(hash).map_err(|_| corrupt_storage())?);
        }
        let head = changes
            .last()
            .zip(seals.last())
            .map(|(change, hash)| ReturnQuantityHead {
                operation: change.operation,
                stock_revision: change.stock_revision,
                projection_hash: *hash,
            });
        #[derive(Serialize)]
        struct Basis<'a> {
            hub: HubId,
            cut: HubRevision,
            line: &'a quantity::CanonicalLine,
            changes: &'a [DepartureQuantityAmendment],
            seals: &'a [[u8; 32]],
            credits: &'a [Credit],
            returns: [u8; 32],
            established:
                &'a [harvestcircle_domain::hub::corrections::EstablishedReturnReservation],
            physical: WholePacks,
            effective: WholePacks,
            unresolved: bool,
        }
        let bytes = serde_json::to_vec(&Basis {
            hub,
            cut,
            line: &line,
            changes: &changes,
            seals: &seals,
            credits: &credits,
            returns: returned.evidence_digest,
            established: &returned.established,
            physical,
            effective: account.effective(),
            unresolved: returned.unresolved,
        })
        .map_err(|_| corrupt_storage())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(corrupt_storage());
        }
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.return.allowance.funding.v1\0");
        hash.update(
            u64::try_from(bytes.len())
                .map_err(|_| corrupt_storage())?
                .to_be_bytes(),
        );
        hash.update(bytes);
        changed |= !changes.is_empty() || returned.unresolved;
        totals.push(EstablishedReturnTotal {
            original_group: original.group,
            packs: returned.total,
        });
        allowances.push(ReturnLineAllowance {
            original_group: original.group,
            effective_departure: account.effective(),
            original_debit_return_bound: physical,
            quantity_head: head,
            funding_hash: hash.finalize().into(),
            unresolved_returns: returned.unresolved,
        });
    }
    allowances.sort_by_key(|line| line.original_group);
    Ok(CapturedReturnContext {
        totals,
        allowances: changed.then_some(ReturnAllowanceCapture {
            hub,
            departure: departure.id,
            operation: departure.operation,
            stock_revision: cut,
            lines: allowances,
        }),
    })
}
