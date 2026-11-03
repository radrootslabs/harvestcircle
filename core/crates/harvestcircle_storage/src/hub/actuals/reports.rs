//! Preserved unverified claims, independent of normal planned-action eligibility.
use super::{common::*, model::*};
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error, storage_unavailable},
    hub::{handling, reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::{
        HubId, HubRevision,
        actuals::{
            ActualReportCommand, ActualReportSources, HUB_ACTUAL_REPORT_CAPACITY,
            ReportedActualSource, actual_report_result_digest,
        },
        reference::{HubRecordId, SourceProvenance},
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use stock::read::{OperationRecord, optional_id, record_id, whole_packs};

pub(in crate::hub) async fn by_id_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<HubActualReportReceipt, SafeError> {
    let operation: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_actual_reports WHERE report_id=? LIMIT 1",
    )
    .bind(id.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let operation = DurableRequestId::parse(operation.ok_or_else(stock::invalid_input)?)
        .map_err(|_| corrupt_storage())?;
    let op = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    original_on(tx, hub, op).await
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubActualReportReceipt, SafeError> {
    if op.kind != "actual_report" {
        return Err(stock::conflict());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(report_id,1,17) AS report_id,policy_version,substr(variant_id,1,17) AS variant_id,substr(recipient_id,1,17) AS recipient_id,packs,",
        "substr(source_kind,1,33) AS source_kind,source_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,",
        "substr(CAST(observation AS BLOB),1,513) AS observation,substr(result_hash,1,33) AS result_hash FROM hub_actual_reports WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let id = record_id(&row, "report_id")?;
    if id != op.result
        || row
            .try_get::<i64, _>("policy_version")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let count = usize::try_from(
        row.try_get::<i64, _>("source_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if count > 64 {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query("SELECT ordinal,substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,substr(location_id,1,17) AS location_id FROM hub_actual_report_sources WHERE report_id=? ORDER BY ordinal LIMIT 65")
        .bind(id.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != count {
        return Err(corrupt_storage());
    }
    let mut sources = Vec::with_capacity(count);
    for (ordinal, row) in rows.iter().enumerate() {
        if row
            .try_get::<i64, _>("ordinal")
            .map_err(|_| corrupt_storage())?
            != ordinal as i64
        {
            return Err(corrupt_storage());
        }
        sources.push(ReportedActualSource {
            trace: record_id(row, "trace_id")?,
            group: optional_id(row, "group_id")?,
            location: optional_id(row, "location_id")?,
        });
    }
    let sources = match row
        .try_get::<String, _>("source_kind")
        .map_err(|_| corrupt_storage())?
        .as_str()
    {
        "unknown" if sources.is_empty() => ActualReportSources::Unknown,
        "possible" if !sources.is_empty() => ActualReportSources::Possible(sources),
        "claim" if sources.len() == 1 => ActualReportSources::EstablishedClaim(sources.remove(0)),
        _ => return Err(corrupt_storage()),
    };
    let command = ActualReportCommand {
        variant: optional_id(&row, "variant_id")?,
        recipient: optional_id(&row, "recipient_id")?,
        packs: row
            .try_get::<Option<i64>, _>("packs")
            .map_err(|_| corrupt_storage())?
            .map(whole_packs)
            .transpose()?,
        sources,
        occurred: stock::read::occurrence(&row)?,
        observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    let prior = HubRevision::from_value(
        op.revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let record = command
        .preserve(
            hub,
            prior,
            op.catalog_revision,
            operation_id(&op.id).map_err(|_| corrupt_storage())?,
            op.actor,
            op.recorded_at,
        )
        .map_err(|_| corrupt_storage())?;
    if record.id != id
        || record.stock_revision != op.revision
        || command.fingerprint(hub, op.actor, record.operation) != op.hash
    {
        return Err(corrupt_storage());
    }
    let saved: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if actual_report_result_digest(&op.hash, &record)
        .map_err(|_| corrupt_storage())?
        .as_slice()
        != saved.as_slice()
    {
        return Err(corrupt_storage());
    }
    let marker:Option<(String,Option<String>)>=sqlx::query_as("SELECT substr(operation_id,1,37),substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let (original, review) = marker.ok_or_else(corrupt_storage)?;
    if original != op.id.as_str() {
        return Err(corrupt_storage());
    }
    if let Some(review) = review {
        let review = DurableRequestId::parse(review).map_err(|_| corrupt_storage())?;
        let valid:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_reviews review JOIN hub_stock_operations o ON o.operation_id=review.operation_id WHERE review.operation_id=? AND review.report_id=? AND o.hub_id=? AND o.kind='actual_review' AND o.result_id=review.report_id")
            .bind(review.as_str()).bind(id.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if valid != 1 {
            return Err(corrupt_storage());
        }
    }
    Ok(HubActualReportReceipt {
        operation: op.id,
        original: record,
    })
}

/// Only the executor's fresh admission path reaches a new report. The same
/// operation's verified original is returned before capacity/session checks.
pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ActualReportCommand,
    now: UnixTimestamp,
) -> Result<HubActualReportReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let id = operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, id);
    if let Some(prior) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if prior.kind != "actual_report" || prior.actor != actor || prior.hash != hash {
            return Err(stock::conflict());
        }
        return original_on(tx, hub.id(), prior).await;
    }
    require_admission_on(tx, hub.id(), actor, &operation, "actual_report", &hash, now).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_reports")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if count < 0 {
        return Err(corrupt_storage());
    }
    if count >= HUB_ACTUAL_REPORT_CAPACITY as i64 {
        return Err(stock::capacity());
    }
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::ActualReport,
    )
    .await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let record = command
        .preserve(hub.id(), state.revision, catalog.revision(), id, actor, now)
        .map_err(error)?;
    let result_hash = actual_report_result_digest(&hash, &record).map_err(error)?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "actual_report".into(),
        actor,
        hash,
        catalog_revision: record.catalog_revision,
        revision: record.stock_revision,
        result: record.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    let (kind, start, end) = occurrence_fields(command.occurred);
    let source_kind = match &command.sources {
        ActualReportSources::Unknown => "unknown",
        ActualReportSources::Possible(_) => "possible",
        ActualReportSources::EstablishedClaim(_) => "claim",
    };
    sqlx::query("INSERT INTO hub_actual_reports (report_id,operation_id,policy_version,variant_id,recipient_id,packs,source_kind,source_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) VALUES (?,?,1,?,?,?,?,?,?,?,?,?,?)")
        .bind(record.id.as_bytes().as_slice()).bind(operation.as_str()).bind(command.variant.as_ref().map(|id|id.as_bytes().as_slice()))
        .bind(command.recipient.as_ref().map(|id|id.as_bytes().as_slice())).bind(command.packs.map(|p|i64::from(p.value())))
        .bind(source_kind).bind(i64::try_from(command.sources.candidates().len()).map_err(|_|corrupt_storage())?).bind(kind).bind(start).bind(end)
        .bind(command.observation.as_str()).bind(result_hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for (ordinal, source) in command.sources.candidates().iter().enumerate() {
        sqlx::query("INSERT INTO hub_actual_report_sources (report_id,ordinal,trace_id,group_id,location_id) VALUES (?,?,?,?,?)")
            .bind(record.id.as_bytes().as_slice()).bind(i64::try_from(ordinal).map_err(|_|corrupt_storage())?).bind(source.trace.as_bytes().as_slice())
            .bind(source.group.as_ref().map(|id|id.as_bytes().as_slice())).bind(source.location.as_ref().map(|id|id.as_bytes().as_slice()))
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    sqlx::query("INSERT INTO hub_actual_report_reviews (report_id,operation_id,review_operation_id) VALUES (?,?,NULL)")
        .bind(record.id.as_bytes().as_slice()).bind(operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    advance_journal_only_on(tx, hub.id(), &state, &operation, record.stock_revision).await?;
    let restored = original_on(tx, hub.id(), op).await?;
    if restored.original != record {
        return Err(corrupt_storage());
    }
    Ok(restored)
}

impl Database {
    pub async fn hub_actual_report_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubActualReportReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        None => Ok(None),
                        Some(op) => Ok(Some(original_on(tx, hub.id(), op).await?)),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
    pub async fn hub_actual_reports(
        &self,
        actor: PublicKey,
        query: HubActualReportQuery,
    ) -> Result<HubActualReportPage, SafeError> {
        query.validate()?;
        self.host().transaction(|tx|Box::pin(async move {
            let hub=reference::authorize_on(tx,actor).await?;let state=stock::read::state_on(tx,hub.id()).await?;
            let catalog=reference::load_catalog_on(tx,hub.id()).await?;
            if query.expected_revision.is_some_and(|v|v!=state.revision)||query.expected_catalog_revision.is_some_and(|v|v!=catalog.revision()) {return Err(stock::conflict());}
            let all=matches!(query.filter,HubActualReportFilter::All);
            let total:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_reports report JOIN hub_actual_report_reviews marker ON marker.report_id=report.report_id JOIN hub_stock_operations o ON o.operation_id=report.operation_id WHERE o.hub_id=? AND (? OR marker.review_operation_id IS NULL)")
                .bind(hub.id().as_bytes().as_slice()).bind(all).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if !(0..=HUB_ACTUAL_REPORT_CAPACITY as i64).contains(&total) {return Err(corrupt_storage());}
            if i64::from(query.offset)>total {return Err(stock::invalid_input());}
            let rows:Vec<(String,Option<String>)>=sqlx::query_as("SELECT substr(report.operation_id,1,37),substr(marker.review_operation_id,1,37) FROM hub_actual_reports report JOIN hub_actual_report_reviews marker ON marker.report_id=report.report_id JOIN hub_stock_operations o ON o.operation_id=report.operation_id WHERE o.hub_id=? AND (? OR marker.review_operation_id IS NULL) ORDER BY o.stock_revision LIMIT ? OFFSET ?")
                .bind(hub.id().as_bytes().as_slice()).bind(all).bind(i64::from(query.limit)).bind(i64::from(query.offset)).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
            let mut reports=Vec::with_capacity(rows.len());
            for (operation,review) in rows {
                let operation=DurableRequestId::parse(operation).map_err(|_|corrupt_storage())?;
                let op=stock::read::operation_on(tx,hub.id(),&operation).await?.ok_or_else(corrupt_storage)?;
                reports.push(HubActualReportRow {report:original_on(tx,hub.id(),op).await?,review_operation:review.map(DurableRequestId::parse).transpose().map_err(|_|corrupt_storage())?});
            }
            Ok(HubActualReportPage {revision:state.revision,catalog_revision:catalog.revision(),offset:query.offset,total:u32::try_from(total).map_err(|_|corrupt_storage())?,more:u64::from(query.offset)+(reports.len() as u64)<total as u64,rows:reports})
        })).await.map_err(map_transaction_error)
    }
}
