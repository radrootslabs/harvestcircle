//! Original status is derived from authoritative immutable receipts, not a phase guess.
use super::*;
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubRuntimeIncarnation, inspect_hub_submission, known_hub_submission_kind},
};
use harvestcircle_domain::{SafeErrorCode, UnixTimestamp, hub::HubId};
use sqlx::Row;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubSubmissionStatus {
    Pending,
    Recorded,
    Rejected(SafeErrorCode),
}

#[derive(Clone, Eq, PartialEq)]
pub struct HubSubmissionRecord {
    pub operation: DurableRequestId,
    pub hub: HubId,
    pub actor: PublicKey,
    pub kind: String,
    pub scope: HubSubmissionScope,
    pub submitted_at: UnixTimestamp,
    pub status: HubSubmissionStatus,
    pub(super) input_hash: [u8; 32],
    pub(super) domain_hash: Option<[u8; 32]>,
    capture: Vec<u8>,
    capture_hash: [u8; 32],
}
impl HubSubmissionRecord {
    #[must_use]
    pub fn captured_semantics(&self) -> &[u8] {
        &self.capture
    }
    #[must_use]
    pub const fn capture_hash(&self) -> &[u8; 32] {
        &self.capture_hash
    }
}
impl std::fmt::Debug for HubSubmissionRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubSubmissionRecord")
            .field("operation", &self.operation)
            .field("kind", &self.kind)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

pub(super) struct Original {
    pub hub: HubId,
    pub actor: PublicKey,
    pub kind: String,
    pub hash: Option<[u8; 32]>,
    pub recorded_at: UnixTimestamp,
}

pub(super) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
) -> Result<Option<Original>, SafeError> {
    let rows=sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub_id,substr(actor_public_key,1,33) AS actor,",
        "CASE event_kind WHEN 'initialized' THEN 'initialize' ELSE 'ownership_transfer' END AS kind,NULL AS hash,recorded_at_unix_s AS recorded_at FROM hub_history WHERE operation_id=? ",
        "UNION ALL SELECT substr(hub_id,1,17),substr(actor_public_key,1,33),'reference',substr(intent_hash,1,33),recorded_at_unix_s FROM hub_reference_versions WHERE operation_id=? ",
        "UNION ALL SELECT substr(hub_id,1,17),substr(actor_public_key,1,33),substr(kind,1,33),substr(intent_hash,1,33),recorded_at_unix_s FROM hub_stock_operations WHERE operation_id=? LIMIT 4"
    )).bind(operation.as_str()).bind(operation.as_str()).bind(operation.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > 1 {
        return Err(stock::conflict());
    }
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let hub: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
    let actor: Vec<u8> = row.try_get("actor").map_err(|_| corrupt_storage())?;
    let hash: Option<Vec<u8>> = row.try_get("hash").map_err(|_| corrupt_storage())?;
    Ok(Some(Original {
        hub: HubId::from_bytes(hub.try_into().map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())?,
        actor: PublicKey::from_bytes(actor.try_into().map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())?,
        kind: row.try_get("kind").map_err(|_| corrupt_storage())?,
        hash: hash
            .map(|h| h.try_into().map_err(|_| corrupt_storage()))
            .transpose()?,
        recorded_at: UnixTimestamp::from_seconds(
            row.try_get("recorded_at").map_err(|_| corrupt_storage())?,
        )
        .ok_or_else(corrupt_storage)?,
    }))
}

impl Database {
    /// Current ownership permits reading originals; an archived scope is never a grant.
    pub async fn hub_submission(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubSubmissionRecord>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = super::super::load_on(tx).await?;
                    if let Some((ownership, _)) = &hub {
                        ownership
                            .authorize(Some(actor))
                            .map_err(|_| reference::hub_permission_denied())?;
                    }
                    let record = record_on(tx, &operation).await?;
                    if let Some(record) = &record {
                        if let Some((ownership, _)) = hub {
                            if record.hub != ownership.id() {
                                return Err(corrupt_storage());
                            }
                        } else if record.actor != actor {
                            return Err(reference::hub_permission_denied());
                        }
                    }
                    Ok(record)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn record_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
) -> Result<Option<HubSubmissionRecord>, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub_id,substr(actor_public_key,1,33) AS actor,substr(kind,1,33) AS kind,",
        "substr(runtime_id,1,17) AS runtime,substr(generation,1,9) AS generation,substr(input_hash,1,33) AS input_hash,",
        "substr(domain_intent_hash,1,33) AS domain_hash,capture_schema,substr(capture_body,1,16777217) AS capture,substr(capture_hash,1,33) AS capture_hash,submitted_at_unix_s ",
        "FROM hub_submissions WHERE operation_id=? LIMIT 1"
    )).bind(operation.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let Some(row) = row else { return Ok(None) };
    let bytes = |name: &str| -> Result<Vec<u8>, SafeError> {
        row.try_get(name).map_err(|_| corrupt_storage())
    };
    let hub = HubId::from_bytes(bytes("hub_id")?.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let actor = PublicKey::from_bytes(bytes("actor")?.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let kind: String = row.try_get("kind").map_err(|_| corrupt_storage())?;
    if !known_hub_submission_kind(&kind)
        || row
            .try_get::<i64, _>("capture_schema")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let scope = HubSubmissionScope {
        runtime: HubRuntimeIncarnation::from_bytes(
            bytes("runtime")?
                .try_into()
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?,
        generation: SessionGeneration::from_value(u64::from_be_bytes(
            bytes("generation")?
                .try_into()
                .map_err(|_| corrupt_storage())?,
        )),
        actor,
    };
    let input_hash: [u8; 32] = bytes("input_hash")?
        .try_into()
        .map_err(|_| corrupt_storage())?;
    let domain_hash: Option<Vec<u8>> = row.try_get("domain_hash").map_err(|_| corrupt_storage())?;
    let domain_hash: Option<[u8; 32]> = domain_hash
        .map(|h| h.try_into().map_err(|_| corrupt_storage()))
        .transpose()?;
    if domain_hash.is_none() != (kind == "initialize") {
        return Err(corrupt_storage());
    }
    let capture = bytes("capture")?;
    let capture_hash: [u8; 32] = bytes("capture_hash")?
        .try_into()
        .map_err(|_| corrupt_storage())?;
    let metadata = inspect_hub_submission(&capture).map_err(|_| corrupt_storage())?;
    if metadata.operation != *operation
        || metadata.kind != kind
        || metadata.hub.id() != hub
        || metadata.scope != scope
        || metadata.input_digest != input_hash
        || metadata.capture_digest != capture_hash
    {
        return Err(corrupt_storage());
    }
    let submitted_at = UnixTimestamp::from_seconds(
        row.try_get("submitted_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    let rejected=sqlx::query("SELECT substr(code,1,33) AS code,recorded_at_unix_s FROM hub_submission_rejections WHERE operation_id=? LIMIT 1")
        .bind(operation.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let original = original_on(tx, operation).await?;
    let status = match (original, rejected) {
        (Some(_), Some(_)) => return Err(corrupt_storage()),
        (Some(original), None) => {
            if original.hub != hub
                || original.actor != actor
                || original.kind != kind
                || original.hash != domain_hash
                || original.recorded_at < submitted_at
            {
                return Err(corrupt_storage());
            }
            HubSubmissionStatus::Recorded
        }
        (None, Some(row)) => {
            let recorded: i64 = row
                .try_get("recorded_at_unix_s")
                .map_err(|_| corrupt_storage())?;
            if recorded < submitted_at.as_seconds() {
                return Err(corrupt_storage());
            }
            HubSubmissionStatus::Rejected(rejection_code(
                &row.try_get::<String, _>("code")
                    .map_err(|_| corrupt_storage())?,
            )?)
        }
        (None, None) => HubSubmissionStatus::Pending,
    };
    Ok(Some(HubSubmissionRecord {
        operation: operation.clone(),
        hub,
        actor,
        kind,
        scope,
        submitted_at,
        status,
        input_hash,
        domain_hash,
        capture,
        capture_hash,
    }))
}

fn rejection_code(code: &str) -> Result<SafeErrorCode, SafeError> {
    Ok(match code {
        "conflict" => SafeErrorCode::HubConflict,
        "invalid_input" => SafeErrorCode::HubInvalidInput,
        "capacity" => SafeErrorCode::HubCapacity,
        "permission_denied" => SafeErrorCode::HubPermissionDenied,
        "storage_unavailable" => SafeErrorCode::StorageUnavailable,
        "storage_corrupt" => SafeErrorCode::StorageCorrupt,
        "internal" => SafeErrorCode::InvalidApplicationState,
        _ => return Err(corrupt_storage()),
    })
}
