use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::movement::HUB_DISCREPANCY_CAPACITY;
use harvestcircle_domain::hub::reference::{ReferenceKind, SourceProvenance};

impl Database {
    pub async fn hub_group(
        &self,
        actor: PublicKey,
        id: HubRecordId,
    ) -> Result<HubGroupView, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = reference::authorize_on(transaction, actor).await?;
                    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
                    let state = stock::read::state_on(transaction, hub.id()).await?;
                    let group = read::group_on(transaction, hub.id(), id, &catalog).await?;
                    let trace = read::trace_on(transaction, hub.id(), group.trace).await?;
                    Ok(HubGroupView {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        location: catalog
                            .require_kind(group.location, ReferenceKind::Location)
                            .map_err(|_| corrupt_storage())?
                            .clone(),
                        product: catalog
                            .require_kind(trace.product.id, ReferenceKind::Product)
                            .map_err(|_| corrupt_storage())?
                            .clone(),
                        variant: catalog
                            .require_kind(group.variant, ReferenceKind::Pack)
                            .map_err(|_| corrupt_storage())?
                            .clone(),
                        group,
                        trace,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_work_page(
        &self,
        actor: PublicKey,
        query: HubHandlingQuery,
    ) -> Result<HubHandlingPage<HubWorkRow>, SafeError> {
        query.validate()?;
        self.host().transaction(|transaction| Box::pin(async move {
            let hub = reference::authorize_on(transaction,actor).await?;
            let catalog = reference::load_catalog_on(transaction,hub.id()).await?;
            let state = stock::read::state_on(transaction,hub.id()).await?;
            validate_versions(&query,state.revision,catalog.revision())?;
            let pattern = escaped_pattern(&query.text);
            let count: i64 = sqlx::query_scalar(concat!(
                "SELECT count(*) FROM hub_work_requirements w JOIN hub_receipts r ON r.trace_id=w.trace_id ",
                "JOIN hub_reference_versions v ON v.record_id=r.product_id AND NOT EXISTS(SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=v.record_id AND newer.record_revision>v.record_revision) ",
                "WHERE (? IS NULL OR w.trace_id=?) AND (?=0 OR w.remaining>0) AND (?='' OR v.label LIKE ? ESCAPE '\\')"
            )).bind(query.trace.as_ref().map(|id| id.as_bytes().as_slice())).bind(query.trace.as_ref().map(|id| id.as_bytes().as_slice()))
                .bind(query.open_only).bind(&query.text).bind(&pattern).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            let total = total(count,HUB_WORK_CAPACITY,query.offset)?;
            let selected = sqlx::query(concat!(
                "SELECT substr(w.work_id,1,17) AS work_id FROM hub_work_requirements w JOIN hub_receipts r ON r.trace_id=w.trace_id ",
                "JOIN hub_reference_versions v ON v.record_id=r.product_id AND NOT EXISTS(SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=v.record_id AND newer.record_revision>v.record_revision) ",
                "WHERE (? IS NULL OR w.trace_id=?) AND (?=0 OR w.remaining>0) AND (?='' OR v.label LIKE ? ESCAPE '\\') ORDER BY w.work_id LIMIT ? OFFSET ?"
            )).bind(query.trace.as_ref().map(|id| id.as_bytes().as_slice())).bind(query.trace.as_ref().map(|id| id.as_bytes().as_slice()))
                .bind(query.open_only).bind(&query.text).bind(&pattern).bind(i64::from(query.limit)).bind(i64::from(query.offset))
                .fetch_all(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            let mut rows=Vec::with_capacity(selected.len());
            for row in selected {
                let id = stock::read::record_id(&row,"work_id")?;
                let work = read::work_on(transaction,hub.id(),id).await?;
                let definition = read::definition_on(transaction,hub.id(),id).await?;
                let operation = stock::read::operation_on(transaction,hub.id(),&definition.created_operation).await?.ok_or_else(corrupt_storage)?;
                let origin = match operation.kind.as_str() { "receipt" if id==work.trace && operation.result==work.trace => HubWorkOrigin::Intake,
                    "work_replan" if operation.result==id => HubWorkOrigin::Replan,
                    "work_new" if operation.result==id => HubWorkOrigin::NewHandling, _ => return Err(corrupt_storage()) };
                let trace = read::trace_on(transaction,hub.id(),work.trace).await?;
                let observation = if origin == HubWorkOrigin::Intake { trace.observation.clone() } else {
                    let body: Vec<u8> = sqlx::query_scalar("SELECT substr(CAST(observation AS BLOB),1,513) FROM hub_work_actions WHERE operation_id=? AND replacement_work_id=? LIMIT 1")
                        .bind(operation.id.as_str()).bind(id.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
                    SourceProvenance::new(String::from_utf8(body).map_err(|_| corrupt_storage())?).map_err(|_| corrupt_storage())?
                };
                let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_group_work WHERE work_id=? AND progress='pending' AND scoped_packs>0")
                    .bind(id.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
                rows.push(HubWorkRow { product: catalog.require_kind(trace.product.id,ReferenceKind::Product).map_err(|_| corrupt_storage())?.clone(),
                    variant: catalog.require_kind(work.variant,ReferenceKind::Pack).map_err(|_| corrupt_storage())?.clone(),
                    source_needs_review: trace.source_needs_review(), work, pending_groups: u32::try_from(pending).map_err(|_| corrupt_storage())?, origin, observation,
                    created_by: operation.actor, created_at: operation.recorded_at, created_operation: operation.id });
            }
            Ok(HubHandlingPage { revision: state.revision, catalog_revision: catalog.revision(), offset: query.offset, total,
                more: query.offset + u32::try_from(rows.len()).map_err(|_| corrupt_storage())? < total, rows })
        })).await.map_err(map_transaction_error)
    }

    pub async fn hub_discrepancy_page(
        &self,
        actor: PublicKey,
        query: HubHandlingQuery,
    ) -> Result<HubHandlingPage<HubDiscrepancyRow>, SafeError> {
        query.validate()?;
        self.host().transaction(|transaction|Box::pin(async move {
            let hub=reference::authorize_on(transaction,actor).await?;let catalog=reference::load_catalog_on(transaction,hub.id()).await?;
            let state=stock::read::state_on(transaction,hub.id()).await?;validate_versions(&query,state.revision,catalog.revision())?;
            let pattern=escaped_pattern(&query.text);
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM hub_discrepancies WHERE (? IS NULL OR trace_id=?) AND (?=0 OR resolution IS NULL) AND (?='' OR observation LIKE ? ESCAPE '\\')")
                .bind(query.trace.as_ref().map(|id|id.as_bytes().as_slice())).bind(query.trace.as_ref().map(|id|id.as_bytes().as_slice())).bind(query.open_only).bind(&query.text).bind(&pattern)
                .fetch_one(&mut *transaction).await.map_err(|_|storage_unavailable())?;
            let total=total(count,HUB_DISCREPANCY_CAPACITY,query.offset)?;
            let selected=sqlx::query("SELECT substr(discrepancy_id,1,17) AS discrepancy_id FROM hub_discrepancies WHERE (? IS NULL OR trace_id=?) AND (?=0 OR resolution IS NULL) AND (?='' OR observation LIKE ? ESCAPE '\\') ORDER BY discrepancy_id LIMIT ? OFFSET ?")
                .bind(query.trace.as_ref().map(|id|id.as_bytes().as_slice())).bind(query.trace.as_ref().map(|id|id.as_bytes().as_slice())).bind(query.open_only).bind(&query.text).bind(&pattern).bind(i64::from(query.limit)).bind(i64::from(query.offset))
                .fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
            let mut rows=Vec::with_capacity(selected.len());
            for row in selected {
                let current=discrepancy::value_on(transaction,hub.id(),stock::read::record_id(&row,"discrepancy_id")?).await?;
                let trace=read::trace_on(transaction,hub.id(),current.value.trace).await?;
                rows.push(HubDiscrepancyRow{product:catalog.require_kind(trace.product.id,ReferenceKind::Product).map_err(|_|corrupt_storage())?.clone(),variant:catalog.require_kind(trace.variant.id,ReferenceKind::Pack).map_err(|_|corrupt_storage())?.clone(),
                    created_by:current.created.actor,created_at:current.created.recorded_at,created_operation:current.created.id,
                    resolved_by:current.resolved.as_ref().map(|operation|operation.actor),resolved_at:current.resolved.as_ref().map(|operation|operation.recorded_at),resolved_operation:current.resolved.map(|operation|operation.id),value:current.value});
            }
            Ok(HubHandlingPage{revision:state.revision,catalog_revision:catalog.revision(),offset:query.offset,total,more:query.offset+u32::try_from(rows.len()).map_err(|_|corrupt_storage())?<total,rows})
        })).await.map_err(map_transaction_error)
    }
}

pub(super) fn validate_versions(
    query: &HubHandlingQuery,
    stock: HubRevision,
    catalog: HubRevision,
) -> Result<(), SafeError> {
    if query
        .expected_revision
        .is_some_and(|expected| expected != stock)
        || query
            .expected_catalog_revision
            .is_some_and(|expected| expected != catalog)
    {
        return Err(conflict());
    }
    Ok(())
}

pub(super) fn total(count: i64, maximum: usize, offset: u32) -> Result<u32, SafeError> {
    if count < 0 || count > maximum as i64 {
        return Err(corrupt_storage());
    }
    let count = u32::try_from(count).map_err(|_| corrupt_storage())?;
    if offset > count {
        return Err(invalid_input());
    }
    Ok(count)
}

pub(super) fn escaped_pattern(text: &str) -> String {
    format!(
        "%{}%",
        text.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}
