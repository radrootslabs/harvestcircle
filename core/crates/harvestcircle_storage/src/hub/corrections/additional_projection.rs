//! Independently sealed physical-debit metadata. No full correction replan or
//! current lineage read belongs here. Current authority still audits full roots.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualPhysicalReduction, HistoricalActualSource},
    corrections::{
        AdditionalQuantityCommand, DepartureQuantityAmendment, DepartureQuantityDelta,
        OriginalPhysicalDebit, PhysicalDebitFunding, PhysicalDebitKey, PhysicalDebitKind,
    },
    stock::Occurrence,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(super) struct Basis<'a> {
    pub command: &'a AdditionalQuantityCommand,
    pub target: &'a super::quantity::CanonicalLine,
    pub source: &'a HistoricalActualSource,
    pub reduction: &'a ActualPhysicalReduction,
    pub quantity: [u8; 32],
    pub frozen: [u8; 32],
    pub admission: [u8; 32],
}

pub(super) fn base_digest(
    hub: HubId,
    op: &OperationRecord,
    basis: &Basis<'_>,
) -> Result<[u8; 32], SafeError> {
    if op.kind != "correction_additional"
        || basis.command.fingerprint(
            hub,
            op.actor,
            crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?,
        ) != op.hash
        || basis.reduction.before.id != basis.command.balancing_group
        || basis.reduction.before.revision != basis.command.expected_balancing_revision
        || basis.reduction.before.trace != basis.target.original.trace
        || basis.reduction.before.variant != basis.target.original.variant
        || basis.target.original.operation != basis.command.facts.target_operation
        || basis.target.original.group != basis.command.facts.target_group
    {
        return Err(corrupt_storage());
    }
    let floor = basis
        .reduction
        .causal_floor_after_reconciliation(basis.command.facts.occurred, op.recorded_at)
        .map_err(|_| corrupt_storage())?;
    #[derive(Serialize)]
    struct Bound<'a> {
        hub: HubId,
        operation: &'a str,
        actor: PublicKey,
        input: [u8; 32],
        result: HubRecordId,
        catalog: HubRevision,
        revision: HubRevision,
        recorded_at: UnixTimestamp,
        command: &'a AdditionalQuantityCommand,
        target: &'a super::quantity::CanonicalLine,
        source: &'a HistoricalActualSource,
        reduction: &'a ActualPhysicalReduction,
        causal_floor: Option<UnixTimestamp>,
        quantity: [u8; 32],
        frozen: [u8; 32],
        admission: [u8; 32],
    }
    let bytes = serde_json::to_vec(&Bound {
        hub,
        operation: op.id.as_str(),
        actor: op.actor,
        input: op.hash,
        result: op.result,
        catalog: op.catalog_revision,
        revision: op.revision,
        recorded_at: op.recorded_at,
        command: basis.command,
        target: basis.target,
        source: basis.source,
        reduction: basis.reduction,
        causal_floor: floor,
        quantity: basis.quantity,
        frozen: basis.frozen,
        admission: basis.admission,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.additional.physical.base.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

/// Acyclic binding: full result commits to base; stored physical seal commits
/// to both base and that full result. Neither hash is a new execution grant.
pub(super) fn seal(base: [u8; 32], result: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.additional.physical.seal.v1\0");
    hash.update(base);
    hash.update(result);
    hash.finalize().into()
}

pub(super) struct Projection {
    pub original: OriginalPhysicalDebit,
    pub reduction: ActualPhysicalReduction,
    pub source: HistoricalActualSource,
    pub quantity: super::quantity::QuantityProjection,
    pub quantity_hash: [u8; 32],
    pub frozen: [u8; 32],
    pub admission: [u8; 32],
    pub base_hash: [u8; 32],
    pub result_hash: [u8; 32],
}

pub(super) async fn read_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Projection, SafeError> {
    let native = super::additional_record::input_on(tx, hub, op).await?;
    let command = &native.command;
    let target = crate::hub::actuals::common::operation(command.facts.target_operation)
        .map_err(|_| corrupt_storage())?;
    let cut = command.facts.expected_stock_revision;
    let canonical = Box::pin(super::quantity::original_on(
        tx,
        hub,
        &target,
        command.facts.target_group,
        cut,
    ))
    .await?;
    let row=sqlx::query(concat!(
        "SELECT substr(target_operation_id,1,37) AS target,substr(group_id,1,17) AS group_id,",
        "before_packs,after_packs,delta_kind,delta_packs,substr(projection_hash,1,33) AS projection_hash ",
        "FROM hub_correction_quantity_changes WHERE operation_id=?"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let packs = |n| stock::read::whole_packs(n);
    let before = packs(row.try_get("before_packs").map_err(|_| corrupt_storage())?)?;
    let after = packs(row.try_get("after_packs").map_err(|_| corrupt_storage())?)?;
    let delta = packs(row.try_get("delta_packs").map_err(|_| corrupt_storage())?)?;
    if row
        .try_get::<String, _>("target")
        .map_err(|_| corrupt_storage())?
        != target.as_str()
        || stock::read::record_id(&row, "group_id")? != canonical.original.group
        || row
            .try_get::<String, _>("delta_kind")
            .map_err(|_| corrupt_storage())?
            != "additional"
        || delta.value() == 0
        || before.checked_add(delta).map_err(|_| corrupt_storage())? != after
        || after != command.facts.corrected_line_packs
    {
        return Err(corrupt_storage());
    }
    // Independently bind the exact preceding scalar head, without recursively
    // replanning the predecessor through a current physical stock context.
    let prior=sqlx::query(concat!(
        "SELECT substr(c.operation_id,1,37) AS operation,q.after_packs FROM hub_corrections c ",
        "JOIN hub_stock_operations o ON o.operation_id=c.operation_id JOIN hub_correction_quantity_changes q ON q.operation_id=c.operation_id ",
        "WHERE c.target_operation_id=? AND c.subject_kind='line' AND c.subject_id=? AND c.family IN ('additional','found') ",
        "AND o.hub_id=? AND o.stock_revision<? ORDER BY o.stock_revision DESC LIMIT 1"
    )).bind(target.as_str()).bind(canonical.original.group.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let (previous, effective) = match prior {
        None => (None, canonical.original.packs),
        Some(row) => {
            let id = DurableRequestId::parse(
                row.try_get::<String, _>("operation")
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?;
            (
                Some(
                    crate::hub::actuals::common::operation_id(&id)
                        .map_err(|_| corrupt_storage())?,
                ),
                packs(row.try_get("after_packs").map_err(|_| corrupt_storage())?)?,
            )
        }
    };
    if previous != command.facts.expected_prior || effective != before {
        return Err(corrupt_storage());
    }
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let quantity = super::quantity::QuantityProjection {
        original: canonical,
        amendment: DepartureQuantityAmendment {
            operation: own,
            stock_revision: op.revision,
            before,
            after,
            delta: DepartureQuantityDelta::AdditionalDeparture(delta),
        },
        previous,
        report: Some(command.facts.report),
        observation: None,
    };
    let quantity_hash = super::quantity::seal(op, hub, &quantity)?;
    let stored_quantity: Vec<u8> = row
        .try_get("projection_hash")
        .map_err(|_| corrupt_storage())?;
    if stored_quantity.as_slice() != quantity_hash.as_slice() {
        return Err(corrupt_storage());
    }
    let captured = Box::pin(super::additional_frozen::physical_on(
        tx,
        hub,
        op,
        command,
        &quantity.original,
    ))
    .await?;
    let reduction =
        ActualPhysicalReduction::plan(&captured.group, delta).map_err(|_| corrupt_storage())?;
    let physical=sqlx::query(concat!(
        "SELECT substr(target_operation_id,1,37) AS target,substr(target_group_id,1,17) AS target_group,",
        "substr(group_id,1,17) AS group_id,substr(trace_id,1,17) AS trace_id,substr(variant_id,1,17) AS variant_id,",
        "before_group_revision,after_group_revision,before_packs,packs,after_packs,substr(projection_hash,1,33) AS projection_hash ",
        "FROM hub_correction_physical_debits WHERE operation_id=?"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if physical
        .try_get::<String, _>("target")
        .map_err(|_| corrupt_storage())?
        != target.as_str()
        || stock::read::record_id(&physical, "target_group")? != quantity.original.original.group
        || stock::read::record_id(&physical, "group_id")? != reduction.before.id
        || stock::read::record_id(&physical, "trace_id")? != reduction.before.trace
        || stock::read::record_id(&physical, "variant_id")? != reduction.before.variant
        || stock::read::revision_field(&physical, "before_group_revision")?
            != reduction.before.revision
        || stock::read::revision_field(&physical, "after_group_revision")?
            != reduction.after_revision
        || packs(
            physical
                .try_get("before_packs")
                .map_err(|_| corrupt_storage())?,
        )? != reduction.before.packs
        || packs(physical.try_get("packs").map_err(|_| corrupt_storage())?)? != delta
        || packs(
            physical
                .try_get("after_packs")
                .map_err(|_| corrupt_storage())?,
        )? != reduction.on_hand_after
    {
        return Err(corrupt_storage());
    }
    let base_hash = base_digest(
        hub,
        op,
        &Basis {
            command,
            target: &quantity.original,
            source: &captured.source,
            reduction: &reduction,
            quantity: quantity_hash,
            frozen: captured.frozen,
            admission: captured.admission,
        },
    )?;
    let stored: Vec<u8> = physical
        .try_get("projection_hash")
        .map_err(|_| corrupt_storage())?;
    if stored.as_slice() != seal(base_hash, native.result_hash).as_slice() {
        return Err(corrupt_storage());
    }
    let (kind, start, end) = match command.facts.occurred {
        Occurrence::Unknown => ("unknown", None, None),
        Occurrence::Known(at) => ("known", Some(at.as_seconds()), Some(at.as_seconds())),
        Occurrence::Interval { earliest, latest } => (
            "interval",
            Some(earliest.as_seconds()),
            Some(latest.as_seconds()),
        ),
    };
    let events:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=? AND e.kind='correction_additional' ",
        "AND e.trace_id=? AND e.group_id=? AND e.packs=? AND e.from_location_id=? AND e.from_location_revision=? ",
        "AND e.to_location_id IS NULL AND e.to_location_revision IS NULL ",
        "AND e.occurrence_kind=? AND e.occurrence_start_unix_s IS ? AND e.occurrence_end_unix_s IS ? ",
        "AND e.sequence=1+(SELECT count(*) FROM hub_stock_events old JOIN hub_stock_operations original ON original.operation_id=old.operation_id WHERE original.hub_id=? AND original.stock_revision<?)"
    )).bind(op.id.as_str()).bind(reduction.before.trace.as_bytes().as_slice()).bind(reduction.before.id.as_bytes().as_slice())
        .bind(i64::from(delta.value())).bind(captured.source.location.id.as_bytes().as_slice())
        .bind(i64::try_from(captured.source.location.revision.value()).map_err(|_|corrupt_storage())?)
        .bind(kind).bind(start).bind(end).bind(hub.as_bytes().as_slice()).bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?)
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
            .bind(op.id.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    let closed:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE report_id=? AND review_operation_id=?")
        .bind(command.facts.report.as_bytes().as_slice()).bind(op.id.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if events != 1 || total != 1 || closed != 1 {
        return Err(corrupt_storage());
    }
    let original = OriginalPhysicalDebit {
        key: PhysicalDebitKey {
            operation: own,
            group: reduction.before.id,
        },
        kind: PhysicalDebitKind::AdditionalQuantity,
        target: quantity.original.original,
        before: reduction.before.clone(),
        packs: delta,
        stock_revision: op.revision,
        recorded_at: op.recorded_at,
    };
    PhysicalDebitFunding::restore(original.clone(), Vec::new(), Vec::new(), false)
        .map_err(|_| corrupt_storage())?;
    Ok(Projection {
        original,
        reduction,
        source: captured.source,
        quantity,
        quantity_hash,
        frozen: captured.frozen,
        admission: captured.admission,
        base_hash,
        result_hash: native.result_hash,
    })
}

pub(in crate::hub) async fn removed_work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    work: HubRecordId,
) -> Result<crate::hub::actuals::validation::RemovedWork, SafeError> {
    use harvestcircle_domain::hub::movement::WorkProgress;
    let zero = harvestcircle_domain::hub::reference::WholePacks::from_value(0).expect("zero");
    let mut result = crate::hub::actuals::validation::RemovedWork {
        total: zero,
        completed: zero,
        retired: zero,
    };
    let ids:Vec<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE hub_id=? AND kind='correction_additional' ORDER BY stock_revision LIMIT 4097")
        .bind(hub.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let projection = Box::pin(read_on(tx, hub, &op)).await?;
        let Some(scope) = projection
            .reduction
            .removed_work_scope
            .iter()
            .find(|s| s.work == work)
        else {
            continue;
        };
        result.total = result
            .total
            .checked_add(scope.removed)
            .map_err(|_| corrupt_storage())?;
        match scope.removed_progress {
            WorkProgress::Completed => {
                result.completed = result
                    .completed
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::NoLongerRequired => {
                result.retired = result
                    .retired
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::Pending => return Err(corrupt_storage()),
        }
    }
    Ok(result)
}

pub(in crate::hub) async fn physical_debit_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<OriginalPhysicalDebit, SafeError> {
    Ok(Box::pin(read_on(tx, hub, op)).await?.original)
}
