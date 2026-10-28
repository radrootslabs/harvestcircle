use super::*;
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::reference;
use harvestcircle_domain::hub::HubOwnership;
use harvestcircle_domain::hub::reference::{
    SourceAttribution, SourceCertainty, SourceReferenceKind,
};
use harvestcircle_domain::hub::stock::{
    HUB_STOCK_EVENT_CAPACITY, Occurrence, ReceiptCommand, ReceiptIdentities, StockError,
    validate_source_reuse,
};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;

impl Database {
    pub async fn apply_hub_receipt(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ReceiptCommand,
        now: UnixTimestamp,
    ) -> Result<HubStockReceipt, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move { apply_on(transaction, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ReceiptCommand,
    now: UnixTimestamp,
) -> Result<HubStockReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    let ids = receipt_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.trace, ids.group);
    if let Some(original) = super::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.hash != hash || original.actor != actor {
            return Err(conflict());
        }
        return super::read::receipt_on(transaction, hub.id(), original).await;
    }
    let state = super::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    let plan = command
        .plan(&catalog, state.revision, ids.trace, ids.group, actor, now)
        .map_err(stock_error)?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_receipts")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    if count > HUB_TRACE_CAPACITY as i64 {
        return Err(corrupt_storage());
    }
    if count == HUB_TRACE_CAPACITY as i64 || state.event_count as usize >= HUB_STOCK_EVENT_CAPACITY
    {
        return Err(capacity());
    }
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,'receipt',?,?,?,?,?)")
        .bind(operation.as_str()).bind(hub.id().as_bytes().as_slice()).bind(actor.as_bytes().as_slice())
        .bind(hash.as_slice()).bind(integer(command.expected_catalog_revision)?).bind(integer(plan.stock_revision)?)
        .bind(ids.trace.as_bytes().as_slice()).bind(now.as_seconds())
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    persist_source_on(transaction, hub, &operation, &plan, ids).await?;
    insert_receipt_on(transaction, &operation, &plan).await?;
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(ids.group.as_bytes().as_slice())
        .bind(ids.trace.as_bytes().as_slice())
        .execute(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1)")
        .bind(ids.group.as_bytes().as_slice()).bind(plan.position.location.as_bytes().as_slice())
        .bind(integer(plan.trace.receiving.revision)?).bind(i64::from(plan.position.packs.value()))
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    crate::hub::handling::seed_receipt_on(transaction, &operation, &plan).await?;
    let (kind, start, end) = occurrence_fields(plan.trace.occurred);
    sqlx::query("INSERT INTO hub_stock_events (sequence,operation_id,kind,trace_id,group_id,packs,from_location_id,from_location_revision,to_location_id,to_location_revision,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s) VALUES (?,?,'receipt',?,?,?,NULL,NULL,?,?,?,?,?)")
        .bind(i64::from(state.event_count)+1).bind(operation.as_str()).bind(ids.trace.as_bytes().as_slice())
        .bind(ids.group.as_bytes().as_slice()).bind(i64::from(plan.trace.original_packs.value()))
        .bind(plan.trace.receiving.id.as_bytes().as_slice()).bind(integer(plan.trace.receiving.revision)?)
        .bind(kind).bind(start).bind(end).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let changed = sqlx::query("UPDATE hub_stock_state SET revision=?,event_count=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(integer(plan.stock_revision)?).bind(i64::from(state.event_count)+1).bind(hub.id().as_bytes().as_slice())
        .bind(integer(state.revision)?).bind(i64::from(state.event_count))
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(HubStockReceipt {
        operation,
        original: plan,
    })
}

async fn persist_source_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubOwnership,
    operation: &DurableRequestId,
    plan: &ReceiptPlan,
    ids: ReceiptIdentities,
) -> Result<(), SafeError> {
    let SourceAttribution::Referenced { source, .. } = &plan.trace.source else {
        return Ok(());
    };
    let row = sqlx::query("SELECT substr(source_id,1,17) AS source_id, substr(issuer_id,1,17) AS issuer_id, substr(kind,1,20) AS source_kind, substr(CAST(source_code AS BLOB),1,129) AS source_code, substr(CAST(provenance AS BLOB),1,513) AS provenance, 'established' AS source_certainty FROM hub_source_references WHERE hub_id=? AND source_id=? LIMIT 1")
        .bind(hub.id().as_bytes().as_slice()).bind(source.id.as_bytes().as_slice())
        .fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if let Some(row) = row {
        let existing = super::read::source_from_row(&row)?;
        return validate_source_reuse(&plan.trace.source, [existing]).map_err(stock_error);
    }
    if source.id != ids.source {
        return Err(invalid_input());
    }
    let kind = match source.kind {
        SourceReferenceKind::ProducerBatch => "producer_batch",
        SourceReferenceKind::SupplierReference => "supplier_reference",
        SourceReferenceKind::Other => "other",
    };
    sqlx::query("INSERT INTO hub_source_references (source_id,hub_id,issuer_id,issuer_revision,kind,source_code,provenance,created_operation_id) VALUES (?,?,?,?,?,?,?,?)")
        .bind(source.id.as_bytes().as_slice()).bind(hub.id().as_bytes().as_slice())
        .bind(source.issuer.as_ref().map(|id| id.as_bytes().as_slice())).bind(optional_revision(plan.trace.source_issuer.as_ref())?)
        .bind(kind).bind(source.code.as_str()).bind(source.provenance.as_str()).bind(operation.as_str())
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    Ok(())
}

pub(crate) async fn insert_receipt_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &ReceiptPlan,
) -> Result<(), SafeError> {
    let trace = &plan.trace;
    let (source, certainty) = match &trace.source {
        SourceAttribution::Unknown => (None, None),
        SourceAttribution::Referenced { source, certainty } => (
            Some(source.id.as_bytes().as_slice()),
            Some(match certainty {
                SourceCertainty::ReportedPossible => "possible",
                SourceCertainty::Established => "established",
            }),
        ),
    };
    let (kind, start, end) = occurrence_fields(trace.occurred);
    sqlx::query("INSERT INTO hub_receipts (trace_id,root_group_id,operation_id,variant_id,variant_revision,product_id,product_revision,supplier_id,supplier_revision,producer_id,producer_revision,source_id,source_certainty,source_issuer_revision,original_packs,receiving_id,receiving_revision,planned_location_id,planned_location_revision,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(trace.id.as_bytes().as_slice()).bind(plan.position.group.as_bytes().as_slice()).bind(operation.as_str())
        .bind(trace.variant.id.as_bytes().as_slice()).bind(integer(trace.variant.revision)?)
        .bind(trace.product.id.as_bytes().as_slice()).bind(integer(trace.product.revision)?)
        .bind(trace.supplier.as_ref().map(|r| r.id.as_bytes().as_slice())).bind(optional_revision(trace.supplier.as_ref())?)
        .bind(trace.producer.as_ref().map(|r| r.id.as_bytes().as_slice())).bind(optional_revision(trace.producer.as_ref())?)
        .bind(source).bind(certainty).bind(optional_revision(trace.source_issuer.as_ref())?)
        .bind(i64::from(trace.original_packs.value())).bind(trace.receiving.id.as_bytes().as_slice()).bind(integer(trace.receiving.revision)?)
        .bind(trace.planned_location.as_ref().map(|r| r.id.as_bytes().as_slice())).bind(optional_revision(trace.planned_location.as_ref())?)
        .bind(kind).bind(start).bind(end).bind(trace.observation.as_str())
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    Ok(())
}

pub(super) fn occurrence_fields(value: Occurrence) -> (&'static str, Option<i64>, Option<i64>) {
    match value {
        Occurrence::Unknown => ("unknown", None, None),
        Occurrence::Known(at) => ("known", Some(at.as_seconds()), Some(at.as_seconds())),
        Occurrence::Interval { earliest, latest } => (
            "interval",
            Some(earliest.as_seconds()),
            Some(latest.as_seconds()),
        ),
    }
}
fn integer(revision: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(revision.value()).map_err(|_| corrupt_storage())
}
fn optional_revision(record: Option<&ReferenceRecord>) -> Result<Option<i64>, SafeError> {
    record.map(|r| integer(r.revision)).transpose()
}
fn stock_error(error: StockError) -> SafeError {
    match error {
        StockError::StaleRevision | StockError::SourceIdentityConflict => conflict(),
        _ => invalid_input(),
    }
}
