use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::{
        HubId, HubRevision, actuals::ActualReportError, reference::HubRecordId, stock::Occurrence,
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;

pub(super) fn integer(value: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(value.value()).map_err(|_| corrupt_storage())
}
pub(in crate::hub) fn operation_id(value: &DurableRequestId) -> Result<HubRecordId, SafeError> {
    HubRecordId::from_hex(&value.as_str().replace('-', "")).map_err(|_| stock::invalid_input())
}
pub(in crate::hub) fn operation(value: HubRecordId) -> Result<DurableRequestId, SafeError> {
    let hex = value.to_hex();
    DurableRequestId::parse(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
    .map_err(|_| stock::invalid_input())
}
pub(in crate::hub) fn error(value: ActualReportError) -> SafeError {
    match value {
        ActualReportError::Capacity | ActualReportError::RevisionExhausted => stock::capacity(),
        ActualReportError::InvalidInput | ActualReportError::InvalidIdentity => {
            stock::invalid_input()
        }
        ActualReportError::InvalidNativeContext | ActualReportError::InvalidHistory => {
            corrupt_storage()
        }
        ActualReportError::StaleRevision
        | ActualReportError::ConflictingEvidence
        | ActualReportError::InsufficientBalance
        | ActualReportError::UncertainSubmission
        | ActualReportError::HistoricalSourceUnresolved
        | ActualReportError::DuplicateReviewRequired
        | ActualReportError::AlreadyReviewed => stock::conflict(),
    }
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
/// The caller still needs its one-use in-memory FreshAdmission. A matching saved
/// row is never a reconstructed execution grant or permission to replay input.
pub(in crate::hub) async fn require_admission_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    kind: &'static str,
    input: &[u8; 32],
    now: UnixTimestamp,
) -> Result<(), SafeError> {
    let matches:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_submissions s JOIN hub_runtime_session r ON r.singleton=1 ",
        "WHERE s.operation_id=? AND s.hub_id=? AND s.actor_public_key=? AND s.kind=? AND s.domain_intent_hash=? ",
        "AND s.submitted_at_unix_s<=? AND r.runtime_id=s.runtime_id AND r.generation=s.generation AND r.actor_public_key=s.actor_public_key ",
        "AND NOT EXISTS(SELECT 1 FROM hub_submission_rejections rejected WHERE rejected.operation_id=s.operation_id)"
    )).bind(operation.as_str()).bind(hub.as_bytes().as_slice()).bind(actor.as_bytes().as_slice()).bind(kind).bind(input.as_slice())
        .bind(now.as_seconds()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if matches != 1 {
        return Err(reference::hub_permission_denied());
    }
    Ok(())
}
/// Only exact completed report/link headers advance an operation without a
/// physical event. Ordinary multi-event advancement retains its nonzero guard.
pub(super) async fn advance_journal_only_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    state: &stock::read::StockState,
    operation: &DurableRequestId,
    revision: HubRevision,
) -> Result<(), SafeError> {
    if state.revision.value().checked_add(1) != Some(revision.value())
        || revision.value() > harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY as u64 + 1
    {
        return Err(corrupt_storage());
    }
    let completed:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o WHERE o.operation_id=? AND o.hub_id=? AND o.stock_revision=? ",
        "AND NOT EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id) AND (",
        "(o.kind='actual_report' AND EXISTS(SELECT 1 FROM hub_actual_reports report JOIN hub_actual_report_reviews marker ON marker.report_id=report.report_id AND marker.operation_id=report.operation_id WHERE report.operation_id=o.operation_id AND report.report_id=o.result_id AND marker.review_operation_id IS NULL)) OR ",
        "(o.kind='actual_review' AND EXISTS(SELECT 1 FROM hub_actual_reviews review JOIN hub_actual_report_reviews marker ON marker.report_id=review.report_id AND marker.review_operation_id=review.operation_id WHERE review.operation_id=o.operation_id AND review.review_kind='link' AND review.report_id=o.result_id)))"
    )).bind(operation.as_str()).bind(hub.as_bytes().as_slice()).bind(integer(revision)?)
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if completed != 1 {
        return Err(corrupt_storage());
    }
    let changed=sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(integer(revision)?).bind(hub.as_bytes().as_slice()).bind(integer(state.revision)?).bind(i64::from(state.event_count))
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    Ok(())
}
