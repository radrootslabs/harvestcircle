//! Native22 Return mode is immutable even for an empty association scope.
//! Header/result consistency is not complete scope authentication or permission.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::returns::ReturnCommand;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::hub) enum Kind {
    Owned,
    Compatibility,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Compatibility => "legacy_compat",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::hub) struct Mode {
    pub epoch: super::count_return_ledger::Epoch,
    pub kind: Kind,
    pub associations: u32,
    pub scope: [u8; 32],
    pub inner: [u8; 32],
    pub result: [u8; 32],
    pub capture: Option<[u8; 32]>,
}
fn wrapped(hub: HubId, op: &OperationRecord, mode: &Mode) -> Result<[u8; 32], SafeError> {
    if op.kind != "return"
        || mode.associations > 4096
        || (mode.kind == Kind::Owned) != mode.capture.is_some()
        || (mode.kind == Kind::Compatibility && mode.associations != 0)
        || op.revision.value() <= mode.epoch.stock
        || op.catalog_revision.value() < mode.epoch.catalog
    {
        return Err(corrupt_storage());
    }
    let header = serde_json::to_vec(&(
        (
            hub,
            op.id.as_str(),
            &op.kind,
            op.actor,
            op.hash,
            op.revision,
            op.catalog_revision,
            op.result,
            op.recorded_at,
        ),
        (
            mode.epoch.stock,
            mode.epoch.catalog,
            mode.kind.text(),
            mode.associations,
            mode.scope,
            mode.inner,
            mode.capture,
        ),
    ))
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.native-mode.result.v1\0");
    hash.update(
        u32::try_from(header.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(header);
    Ok(hash.finalize().into())
}

#[cfg(test)]
pub(in crate::hub) fn wrapped_for_test(
    hub: HubId,
    op: &OperationRecord,
    mode: &Mode,
) -> Result<[u8; 32], SafeError> {
    wrapped(hub, op, mode)
}

pub(in crate::hub) fn for_write_on<'a, 'c>(
    tx: &'a mut ServiceSqliteTransaction<'c>,
    hub: HubId,
    op: &'a OperationRecord,
    command: &'a ReturnCommand,
    scope: Option<&'a super::count_return_scope::VerifiedScope>,
    inner: [u8; 32],
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Mode>, SafeError>> + Send + 'a>>
{
    Box::pin(async move {
        if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
            return Ok(None);
        }
        let scope = scope.ok_or_else(corrupt_storage)?;
        let digest = scope.validate_for(hub, command.expected_stock_revision)?;
        let material = scope.material(op.result)?;
        let epoch = super::count_return_ledger::epoch_on(tx, hub).await?;
        if material.epoch_stock_revision != epoch.stock
            || material.epoch_catalog_revision != epoch.catalog
            || command.expected_stock_revision.value().checked_add(1) != Some(op.revision.value())
        {
            return Err(corrupt_storage());
        }
        let runtime: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hub_runtime_session)")
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
        let (kind, capture) = if runtime != 0 {
            crate::hub::actuals::common::require_admission_on(
                tx,
                hub,
                op.actor,
                &op.id,
                "return",
                &op.hash,
                op.recorded_at,
            )
            .await?;
            (
                Kind::Owned,
                Some(
                    crate::hub::submission::admission_capture_digest_on(
                        tx,
                        hub,
                        op,
                        HubSubmittedRequest::Return(command),
                    )
                    .await?,
                ),
            )
        } else {
            if material.associations != 0 {
                return Err(corrupt_storage());
            }
            (Kind::Compatibility, None)
        };
        let mut mode = Mode {
            epoch,
            kind,
            associations: material.associations,
            scope: digest,
            inner,
            result: [0; 32],
            capture,
        };
        mode.result = wrapped(hub, op, &mode)?;
        Ok(Some(mode))
    })
}

pub(in crate::hub) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    mode: Option<Mode>,
) -> Result<(), SafeError> {
    let Some(mode) = mode else {
        return if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
            Ok(())
        } else {
            Err(corrupt_storage())
        };
    };
    if wrapped(hub, op, &mode)? != mode.result {
        return Err(corrupt_storage());
    }
    sqlx::query(concat!(
        "INSERT INTO hub_count_return_intake_modes(operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,mode,association_count,scope_hash,inner_result_hash,result_hash,capture_hash) ",
        "VALUES(?,?,1,?,?,?,?,?,?,?,?)"
    )).bind(op.id.as_str()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(mode.epoch.stock).map_err(|_|corrupt_storage())?)
        .bind(i64::try_from(mode.epoch.catalog).map_err(|_|corrupt_storage())?)
        .bind(mode.kind.text()).bind(i64::from(mode.associations)).bind(mode.scope.as_slice())
        .bind(mode.inner.as_slice()).bind(mode.result.as_slice()).bind(mode.capture.map(|v|v.to_vec()))
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

/// Structural prefix symmetry only. Full Return owners authenticate the scope,
/// capture and inner/wrapped result. Future roots are excluded before bodies.
pub(in crate::hub) async fn audit_prefix_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<(), SafeError> {
    let epoch = super::count_return_ledger::epoch_on(tx, hub).await?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_count_return_intake_modes m LEFT JOIN hub_stock_operations o ON o.operation_id=m.operation_id ",
        "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) AND ",
        "(o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>'return' OR m.hub_id<>?1 OR m.epoch<>1 OR ",
        "m.legacy_stock_revision<>?3 OR m.legacy_catalog_revision<>?4 OR o.stock_revision<=?3 OR ",
        "NOT EXISTS(SELECT 1 FROM hub_returns r WHERE r.operation_id=m.operation_id))"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?)
        .bind(i64::try_from(epoch.stock).map_err(|_|corrupt_storage())?).bind(i64::try_from(epoch.catalog).map_err(|_|corrupt_storage())?)
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    Ok(())
}

/// Select from native activation and operation evidence, never optional JSON.
/// Owned identity is historical: later runtime/generation/owner changes do not
/// invalidate an original. Full owner separately recomputes scope and capture.
pub(in crate::hub) fn original_on<'a, 'c>(
    tx: &'a mut ServiceSqliteTransaction<'c>,
    hub: HubId,
    op: &'a OperationRecord,
    command: &'a ReturnCommand,
    result: [u8; 32],
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Mode>, SafeError>> + Send + 'a>>
{
    Box::pin(async move {
        if op.kind != "return" {
            return Err(corrupt_storage());
        }
        if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
            return Ok(None);
        }
        let epoch = super::count_return_ledger::epoch_on(tx, hub).await?;
        let row=sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,substr(mode,1,33) AS mode,association_count,",
        "substr(scope_hash,1,33) AS scope_hash,substr(inner_result_hash,1,33) AS inner_result_hash,substr(result_hash,1,33) AS result_hash,substr(capture_hash,1,33) AS capture_hash ",
        "FROM hub_count_return_intake_modes WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if op.revision.value() <= epoch.stock {
            return if row.is_none() {
                Ok(None)
            } else {
                Err(corrupt_storage())
            };
        }
        let row = row.ok_or_else(corrupt_storage)?;
        let number = |name| -> Result<u64, SafeError> {
            u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())
        };
        let bytes = |name| -> Result<[u8; 32], SafeError> {
            row.try_get::<Vec<u8>, _>(name)
                .map_err(|_| corrupt_storage())?
                .try_into()
                .map_err(|_| corrupt_storage())
        };
        let owner: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
        if owner.as_slice() != hub.as_bytes()
            || number("epoch")? != 1
            || number("legacy_stock_revision")? != epoch.stock
            || number("legacy_catalog_revision")? != epoch.catalog
        {
            return Err(corrupt_storage());
        }
        let kind = match row
            .try_get::<String, _>("mode")
            .map_err(|_| corrupt_storage())?
            .as_str()
        {
            "owned" => Kind::Owned,
            "legacy_compat" => Kind::Compatibility,
            _ => return Err(corrupt_storage()),
        };
        let capture: Option<Vec<u8>> =
            row.try_get("capture_hash").map_err(|_| corrupt_storage())?;
        let capture = capture
            .map(|v| v.try_into().map_err(|_| corrupt_storage()))
            .transpose()?;
        let mode = Mode {
            epoch,
            kind,
            associations: u32::try_from(number("association_count")?)
                .map_err(|_| corrupt_storage())?,
            scope: bytes("scope_hash")?,
            inner: bytes("inner_result_hash")?,
            result: bytes("result_hash")?,
            capture,
        };
        if mode.result != result || wrapped(hub, op, &mode)? != result {
            return Err(corrupt_storage());
        }
        match kind {
            Kind::Owned => {
                crate::hub::submission::verify_recorded_input_identity_on(
                    tx,
                    hub,
                    op,
                    HubSubmittedRequest::Return(command),
                )
                .await?;
                let digest: Vec<u8> = sqlx::query_scalar(
                    "SELECT substr(capture_hash,1,33) FROM hub_submissions WHERE operation_id=?",
                )
                .bind(op.id.as_str())
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
                if mode.capture.as_ref().map(|v| v.as_slice()) != Some(digest.as_slice()) {
                    return Err(corrupt_storage());
                }
            }
            Kind::Compatibility => {
                let admissions: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM hub_submissions WHERE operation_id=?")
                        .bind(op.id.as_str())
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|_| storage_unavailable())?;
                if admissions != 0 {
                    return Err(corrupt_storage());
                }
            }
        }
        Ok(Some(mode))
    })
}
