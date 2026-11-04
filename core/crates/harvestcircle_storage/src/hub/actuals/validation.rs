//! Independent physical and work conservation for committed actual reviews.
use super::{model::*, projection};
use crate::hub::actuals::proofs::OriginalReader;
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::stock,
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    SafeError,
    hub::{
        HubId, HubRevision,
        movement::WorkProgress,
        reference::{HubRecordId, WholePacks},
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;

pub(in crate::hub) async fn headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT ",
        "(SELECT count(*) FROM hub_stock_operations o WHERE o.hub_id=? AND (",
        "(o.kind='actual_report' AND ((SELECT count(*) FROM hub_actual_reports r WHERE r.operation_id=o.operation_id AND r.report_id=o.result_id AND r.policy_version=1)<>1 OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=o.operation_id)<>0)) OR ",
        "(o.kind='actual_review' AND ((SELECT count(*) FROM hub_actual_reviews r WHERE r.operation_id=o.operation_id AND r.report_id=o.result_id AND r.policy_version=1)<>1 OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=o.operation_id)<>COALESCE((SELECT CASE r.review_kind WHEN 'departure' THEN 1 WHEN 'link' THEN 0 ELSE -1 END FROM hub_actual_reviews r WHERE r.operation_id=o.operation_id),-1))))) + ",
        "(SELECT count(*) FROM hub_actual_reports r LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "LEFT JOIN hub_actual_report_reviews marker ON marker.report_id=r.report_id WHERE o.operation_id IS NULL OR o.kind<>'actual_report' OR o.result_id<>r.report_id OR marker.operation_id IS NULL OR marker.operation_id<>r.operation_id OR r.source_count<>(SELECT count(*) FROM hub_actual_report_sources s WHERE s.report_id=r.report_id)) + ",
        "(SELECT count(*) FROM hub_actual_reviews r LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id LEFT JOIN hub_actual_reports report ON report.report_id=r.report_id LEFT JOIN hub_actual_report_reviews marker ON marker.report_id=r.report_id ",
        "WHERE o.operation_id IS NULL OR o.kind<>'actual_review' OR o.result_id<>r.report_id OR report.report_id IS NULL OR marker.review_operation_id IS NULL OR marker.review_operation_id<>r.operation_id OR ",
        "r.lineage_count<>(SELECT count(*) FROM hub_actual_review_lineage l WHERE l.operation_id=r.operation_id) OR r.scope_count<>(SELECT count(*) FROM hub_actual_review_work_scope w WHERE w.operation_id=r.operation_id)) + ",
        "(SELECT count(*) FROM hub_actual_report_reviews marker LEFT JOIN hub_actual_reports report ON report.report_id=marker.report_id ",
        "LEFT JOIN hub_actual_reviews review ON review.operation_id=marker.review_operation_id ",
        "LEFT JOIN hub_corrections correction ON correction.operation_id=marker.review_operation_id ",
        "LEFT JOIN hub_stock_operations current ON current.operation_id=correction.operation_id LEFT JOIN hub_stock_operations original ON original.operation_id=report.operation_id ",
        "WHERE report.report_id IS NULL OR report.operation_id<>marker.operation_id OR (marker.review_operation_id IS NOT NULL AND NOT (",
        "(review.operation_id IS NOT NULL AND review.report_id=marker.report_id) OR ",
        "(correction.operation_id IS NOT NULL AND correction.report_id IS NOT NULL AND current.operation_id IS NOT NULL AND original.operation_id IS NOT NULL AND correction.report_id=marker.report_id AND current.hub_id=original.hub_id AND current.stock_revision>original.stock_revision AND current.result_id=correction.correction_id AND ",
        "((correction.family='additional' AND current.kind='correction_additional') OR (correction.family='count_explain' AND current.kind='correction_count_explain') OR (correction.family='count_classify' AND current.kind='correction_count_classify')))))) + ",
        "(SELECT count(*) FROM hub_actual_report_sources s LEFT JOIN hub_actual_reports r ON r.report_id=s.report_id WHERE r.report_id IS NULL OR s.ordinal<0 OR s.ordinal>=r.source_count) + ",
        "(SELECT count(*) FROM hub_actual_review_lineage l LEFT JOIN hub_actual_reviews r ON r.operation_id=l.operation_id WHERE r.operation_id IS NULL OR r.review_kind<>'departure') + ",
        "(SELECT count(*) FROM hub_actual_review_work_scope w LEFT JOIN hub_actual_reviews r ON r.operation_id=w.operation_id WHERE r.operation_id IS NULL OR r.review_kind<>'departure')"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_reports")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if invalid != 0 || !(0..=4096).contains(&count) {
        return Err(corrupt_storage());
    }
    Ok(())
}

pub(in crate::hub) async fn trace_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<WholePacks, SafeError> {
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o WHERE o.hub_id=? AND o.kind='actual_review' ",
        "AND o.stock_revision<=? AND EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.trace_id=?) ORDER BY o.stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?).bind(trace.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > 4096 {
        return Err(corrupt_storage());
    }
    let mut total = WholePacks::from_value(0).expect("zero");
    for id in ids {
        let operation = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let known = projection::known_on(tx, hub, &op).await?;
        if known.trace != trace {
            return Err(corrupt_storage());
        }
        for line in known.lines {
            total = total
                .checked_add(
                    Box::pin(
                        crate::hub::corrections::physical_balance::logical_line_net_on(
                            tx, hub, &operation, line.group, line.packs, cut,
                        ),
                    )
                    .await?,
                )
                .map_err(|_| corrupt_storage())?;
        }
    }
    Ok(total)
}

pub(in crate::hub) struct RemovedWork {
    pub total: WholePacks,
    pub completed: WholePacks,
    pub retired: WholePacks,
}
pub(in crate::hub) async fn removed_work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    work: HubRecordId,
) -> Result<RemovedWork, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(removed_work_with_proofs_on(&mut reader, hub, work)).await
}

pub(in crate::hub) async fn removed_work_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    work: HubRecordId,
) -> Result<RemovedWork, SafeError> {
    // No actual-review header exists before forward18. This predecessor-compatible
    // fast path consults only the existing operation journal.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_stock_operations WHERE hub_id=? AND kind IN ('actual_review','correction_additional')",
    )
    .bind(hub.as_bytes().as_slice())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let zero = WholePacks::from_value(0).expect("zero");
    let mut removed = RemovedWork {
        total: zero,
        completed: zero,
        retired: zero,
    };
    if count == 0 {
        return Ok(removed);
    }
    let combined_limit = harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY
        + harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS;
    if count < 0 || usize::try_from(count).map_err(|_| corrupt_storage())? > combined_limit {
        return Err(corrupt_storage());
    }
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(review.operation_id,1,37) FROM hub_actual_reviews review ",
        "JOIN hub_stock_operations o ON o.operation_id=review.operation_id ",
        "JOIN hub_actual_review_work_scope scope ON scope.operation_id=review.operation_id ",
        "WHERE o.hub_id=? AND review.review_kind='departure' AND scope.work_id=? ORDER BY o.stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(work.as_bytes().as_slice()).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > 4096 {
        return Err(corrupt_storage());
    }
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = tx.actual_review(hub, op).await?;
        let HubActualReviewOriginal::Departure(plan) = &original.original else {
            return Err(corrupt_storage());
        };
        let scope = plan
            .reduction
            .removed_work_scope
            .iter()
            .find(|scope| scope.work == work)
            .ok_or_else(corrupt_storage)?;
        removed.total = removed
            .total
            .checked_add(scope.removed)
            .map_err(|_| corrupt_storage())?;
        match scope.removed_progress {
            WorkProgress::Completed => {
                removed.completed = removed
                    .completed
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::NoLongerRequired => {
                removed.retired = removed
                    .retired
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::Pending => return Err(corrupt_storage()),
        }
    }
    let additional =
        Box::pin(crate::hub::corrections::additional_projection::removed_work_on(tx, hub, work))
            .await?;
    removed.total = removed
        .total
        .checked_add(additional.total)
        .map_err(|_| corrupt_storage())?;
    removed.completed = removed
        .completed
        .checked_add(additional.completed)
        .map_err(|_| corrupt_storage())?;
    removed.retired = removed
        .retired
        .checked_add(additional.retired)
        .map_err(|_| corrupt_storage())?;
    Ok(removed)
}
