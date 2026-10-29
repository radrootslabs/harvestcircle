//! A fresh successful admission commit creates the sole in-memory execution grant.
use super::*;
use harvestcircle_domain::{SafeErrorCode, SafeMessage, UnixTimestamp};

const ORDINARY_ADMISSION_CAPACITY: i64 = 65_536;
const CLOSURE_CAPTURE_BYTES: usize = 1024 * 1024;

// Neither serialized, cloned nor reconstructed from a saved pending row.
pub(super) struct FreshAdmission {
    pub(super) operation: DurableRequestId,
    pub(super) scope: HubSubmissionScope,
    pub(super) mutation: HubMutation,
}
pub(super) enum Decision {
    Fresh(FreshAdmission),
    Recorded {
        operation: DurableRequestId,
        mutation: HubMutation,
    },
}

pub(super) async fn prepare_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    scope: HubSubmissionScope,
    operation: DurableRequestId,
    mutation: HubMutation,
    now: UnixTimestamp,
) -> Result<Decision, SafeError> {
    session::require_on(tx, scope).await?;
    let request = mutation.submitted_request();
    let raw = request.archive().map_err(archive_error)?;
    let hub = match &request {
        HubSubmittedRequest::Initialize(candidate) => {
            candidate
                .authorize(Some(scope.actor))
                .map_err(|_| reference::hub_permission_denied())?;
            if let Some((current, _)) = super::super::load_on(tx).await? {
                current
                    .authorize(Some(scope.actor))
                    .map_err(|_| reference::hub_permission_denied())?;
            }
            candidate.id()
        }
        _ => reference::authorize_on(tx, scope.actor).await?.id(),
    };
    let domain_hash = intent::hash(&request, hub, scope.actor, &operation)?;
    if let Some(prior) = read::record_on(tx, &operation).await? {
        if prior.actor != scope.actor
            || prior.hub != hub
            || prior.kind != request.kind()
            || prior.input_hash != *raw.digest()
            || prior.domain_hash != domain_hash
        {
            return Err(stock::conflict());
        }
        return match prior.status {
            HubSubmissionStatus::Pending => Err(outcome_unknown()),
            HubSubmissionStatus::Rejected(code) => Err(known_rejection(code)),
            HubSubmissionStatus::Recorded => Ok(Decision::Recorded {
                operation,
                mutation,
            }),
        };
    }
    // Published pre-admission originals keep their existing exact intent checks.
    // This branch cannot authorize a new effect: a matching immutable root must
    // already exist, and restoration cannot run with this shared Database borrow.
    if let Some(original) = read::original_on(tx, &operation).await? {
        if original.hub != hub
            || original.actor != scope.actor
            || original.kind != request.kind()
            || original.hash != domain_hash
        {
            return Err(stock::conflict());
        }
        return Ok(Decision::Recorded {
            operation,
            mutation,
        });
    }
    if unsafe_planned_kind(request.kind()) && has_unresolved_on(tx, &operation).await? {
        return Err(SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "Review the hub's unresolved submitted operations or actual reports before this action.",
            ),
        ));
    }
    let archive = capture::capture_on(tx, scope.actor, scope, &operation, request).await?;
    let closing = matches!(archive.kind(), "hold_release" | "dispatch_unused");
    if closing {
        // Recovery/closure does not spend the ordinary-admission allocation.
        // Every individual archived input/material remains bounded.
        if archive.as_bytes().len() > CLOSURE_CAPTURE_BYTES {
            return Err(stock::capacity());
        }
    } else {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM hub_submissions WHERE kind NOT IN ('hold_release','dispatch_unused')")
            .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if count < 0 {
            return Err(corrupt_storage());
        }
        if count >= ORDINARY_ADMISSION_CAPACITY {
            return Err(stock::capacity());
        }
    }
    sqlx::query(concat!(
        "INSERT INTO hub_submissions (operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) ",
        "VALUES (?,?,?,?,?,?,?,?,1,?,?,?)"
    )).bind(operation.as_str()).bind(hub.as_bytes().as_slice()).bind(scope.actor.as_bytes().as_slice()).bind(archive.kind())
        .bind(scope.runtime.as_bytes().as_slice()).bind(scope.generation.value().to_be_bytes().as_slice())
        .bind(archive.input_digest().as_slice()).bind(domain_hash.as_ref().map(|h|h.as_slice()))
        .bind(archive.as_bytes()).bind(archive.digest().as_slice()).bind(now.as_seconds())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(Decision::Fresh(FreshAdmission {
        operation,
        scope,
        mutation,
    }))
}

pub(in crate::hub) async fn has_unresolved_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    except: &DurableRequestId,
) -> Result<bool, SafeError> {
    let facts = uncertainty_on(tx, except).await?;
    Ok(facts.uncertain_intent || facts.unresolved_actual)
}

pub(in crate::hub) async fn uncertainty_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    except: &DurableRequestId,
) -> Result<HubUncertainty, SafeError> {
    let pending:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_submissions s WHERE s.operation_id<>? ",
        "AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections r WHERE r.operation_id=s.operation_id) ",
        "AND NOT EXISTS (SELECT 1 FROM hub_history h WHERE h.operation_id=s.operation_id AND s.kind='initialize' AND h.event_kind='initialized' AND h.hub_id=s.hub_id AND h.actor_public_key=s.actor_public_key AND h.recorded_at_unix_s>=s.submitted_at_unix_s) ",
        "AND NOT EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.operation_id=s.operation_id AND s.kind='reference' AND v.hub_id=s.hub_id AND v.actor_public_key=s.actor_public_key AND v.intent_hash=s.domain_intent_hash AND v.recorded_at_unix_s>=s.submitted_at_unix_s) ",
        "AND NOT EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=s.operation_id AND o.hub_id=s.hub_id AND o.actor_public_key=s.actor_public_key AND o.kind=s.kind AND o.intent_hash=s.domain_intent_hash AND o.recorded_at_unix_s>=s.submitted_at_unix_s)"
    )).bind(except.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let actual: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_actual_report_reviews WHERE review_operation_id IS NULL",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if pending < 0 || actual < 0 {
        return Err(corrupt_storage());
    }
    Ok(HubUncertainty {
        uncertain_intent: pending != 0,
        unresolved_actual: actual != 0,
    })
}

fn unsafe_planned_kind(kind: &str) -> bool {
    matches!(
        kind,
        "initialize"
            | "receipt"
            | "opening"
            | "move"
            | "split"
            | "work_new"
            | "work_replan"
            | "dispatch_prepare"
            | "dispatch_revise"
            | "dispatch_issue"
            | "dispatch_replace"
            | "departure"
            | "count_post"
    )
}

pub(super) async fn reject_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    error: SafeError,
    now: UnixTimestamp,
) -> Result<(), SafeError> {
    let tag = match error.code() {
        SafeErrorCode::HubConflict => "conflict",
        SafeErrorCode::HubInvalidInput => "invalid_input",
        SafeErrorCode::HubCapacity => "capacity",
        SafeErrorCode::HubPermissionDenied => "permission_denied",
        SafeErrorCode::StorageUnavailable => "storage_unavailable",
        SafeErrorCode::StorageCorrupt => "storage_corrupt",
        SafeErrorCode::InvalidApplicationState => "internal",
        _ => return Err(outcome_unknown()),
    };
    sqlx::query(concat!(
        "INSERT INTO hub_submission_rejections (operation_id,code,recorded_at_unix_s) ",
        "SELECT operation_id,?,MAX(submitted_at_unix_s,?) FROM hub_submissions WHERE operation_id=?"
    ))
    .bind(tag)
    .bind(now.as_seconds())
    .bind(operation.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let record = read::record_on(tx, operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    if record.status != HubSubmissionStatus::Rejected(error.code()) {
        return Err(corrupt_storage());
    }
    Ok(())
}

pub(super) fn known_rejection(code: SafeErrorCode) -> SafeError {
    match code {
        SafeErrorCode::HubConflict => stock::conflict(),
        SafeErrorCode::HubInvalidInput => stock::invalid_input(),
        SafeErrorCode::HubCapacity => stock::capacity(),
        SafeErrorCode::HubPermissionDenied => reference::hub_permission_denied(),
        SafeErrorCode::StorageCorrupt => corrupt_storage(),
        _ => SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "The original submission was rejected without recording a change. Review its details before another submission.",
            ),
        ),
    }
}

pub(super) const fn outcome_unknown() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubOutcomeUnknown,
        SafeMessage::new(
            "The original submitted operation requires outcome review before another attempt.",
        ),
    )
}
