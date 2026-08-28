use harvestcircle_application::{
    BoxFuture, DurableIdentityOperation, DurableOperationKind, DurableOperationPhase,
    DurableOperationReceipt, DurableOperationRepository, DurableOperationStart, DurableRequestId,
    DurableTerminalOutcome, OperationDiagnostic, OperationPriorState,
};
use harvestcircle_domain::{
    PublicKey, SafeError, SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;

use crate::contract::{
    HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY, HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH,
    HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS,
};
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::{Database, HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY};

const DURABLE_OPERATION_PROJECTION: &str = "SELECT \
    substr(CAST(request_id AS BLOB), 1, 37) AS request_id, length(CAST(request_id AS BLOB)) AS request_id_bytes, \
    substr(CAST(operation_kind AS BLOB), 1, 7) AS operation_kind, length(CAST(operation_kind AS BLOB)) AS operation_kind_bytes, \
    substr(account_public_key, 1, 33) AS account_public_key, length(account_public_key) AS account_public_key_bytes, \
    expected_revision, substr(CAST(phase AS BLOB), 1, 21) AS phase, length(CAST(phase AS BLOB)) AS phase_bytes, \
    CASE WHEN prior_selected_public_key IS NULL THEN NULL ELSE substr(prior_selected_public_key, 1, 33) END AS prior_selected_public_key, \
    CASE WHEN prior_selected_public_key IS NULL THEN NULL ELSE length(prior_selected_public_key) END AS prior_selected_public_key_bytes, \
    updated_at_unix_s, \
    CASE WHEN diagnostic_code IS NULL THEN NULL ELSE substr(CAST(diagnostic_code AS BLOB), 1, 21) END AS diagnostic_code, \
    CASE WHEN diagnostic_code IS NULL THEN NULL ELSE length(CAST(diagnostic_code AS BLOB)) END AS diagnostic_code_bytes, \
    CASE WHEN terminal_outcome IS NULL THEN NULL ELSE substr(CAST(terminal_outcome AS BLOB), 1, 10) END AS terminal_outcome, \
    CASE WHEN terminal_outcome IS NULL THEN NULL ELSE length(CAST(terminal_outcome AS BLOB)) END AS terminal_outcome_bytes, \
    CASE WHEN prior_binding_availability IS NULL THEN NULL ELSE substr(CAST(prior_binding_availability AS BLOB), 1, 19) END AS prior_binding_availability, \
    CASE WHEN prior_binding_availability IS NULL THEN NULL ELSE length(CAST(prior_binding_availability AS BLOB)) END AS prior_binding_availability_bytes, \
    resulting_revision, completed_at_unix_s FROM durable_operations";

impl DurableOperationRepository for Database {
    #[allow(clippy::too_many_arguments)]
    fn begin_durable_operation<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        kind: DurableOperationKind,
        identity: PublicKey,
        expected_revision: Option<u64>,
        prior: OperationPriorState,
        updated_at: UnixTimestamp,
    ) -> BoxFuture<'a, Result<DurableOperationStart, SafeError>> {
        Box::pin(async move {
            let expected_revision = encode_revision(expected_revision)?;
            let request_id = request_id.as_str().to_owned();
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        if let Some(operation) = query_operation(transaction, &request_id).await? {
                            if operation.kind() != kind
                                || operation.identity() != identity
                                || operation.expected_revision()
                                    != expected_revision.map(|value| value as u64)
                                || operation.prior() != prior
                            {
                                return Err(operation_conflict());
                            }
                            return Ok(DurableOperationStart::Existing(operation));
                        }
                        cleanup_expired_terminal_receipts(transaction, updated_at).await?;
                        let unfinished: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM (SELECT 1 FROM durable_operations \
                             WHERE terminal_outcome IS NULL LIMIT 1025)",
                        )
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if usize::try_from(unfinished).ok().is_none_or(|count| {
                            count >= HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY
                        }) {
                            return Err(operation_capacity_exhausted());
                        }
                        let total: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM (SELECT 1 FROM durable_operations LIMIT 4097)",
                        )
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if usize::try_from(total)
                            .ok()
                            .is_none_or(|count| count >= HARVESTCIRCLE_DURABLE_OPERATION_CAPACITY)
                        {
                            return Err(operation_capacity_exhausted());
                        }
                        let result = sqlx::query(
                            "INSERT INTO durable_operations (request_id, operation_kind, \
                             account_public_key, binding_public_key, expected_revision, phase, \
                             prior_selected_public_key, updated_at_unix_s, prior_binding_availability) \
                             VALUES (?, ?, ?, ?, ?, 'intent_recorded', ?, ?, ?)",
                        )
                        .bind(&request_id)
                        .bind(encode_kind(kind))
                        .bind(identity.as_bytes().as_slice())
                        .bind(identity.as_bytes().as_slice())
                        .bind(expected_revision)
                        .bind(prior.selected_identity().map(|value| value.as_bytes().to_vec()))
                        .bind(updated_at.as_seconds())
                        .bind(prior.binding_availability().map(encode_availability))
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(corrupt_storage());
                        }
                        let operation = query_operation(transaction, &request_id)
                            .await?
                            .ok_or_else(corrupt_storage)?;
                        Ok(DurableOperationStart::Started(operation))
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn load_durable_operation<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
    ) -> BoxFuture<'a, Result<Option<DurableIdentityOperation>, SafeError>> {
        Box::pin(async move {
            let request_id = request_id.as_str().to_owned();
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move { query_operation(transaction, &request_id).await })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn advance_durable_operation<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        expected_phase: DurableOperationPhase,
        next_phase: DurableOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> BoxFuture<'a, Result<DurableIdentityOperation, SafeError>> {
        Box::pin(async move {
            let request_id = request_id.as_str().to_owned();
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let result = sqlx::query(
                            "UPDATE durable_operations SET phase = ?, updated_at_unix_s = ?, \
                             diagnostic_code = ? WHERE request_id = ? AND phase = ? \
                             AND terminal_outcome IS NULL",
                        )
                        .bind(encode_phase(next_phase))
                        .bind(updated_at.as_seconds())
                        .bind(diagnostic.map(encode_diagnostic))
                        .bind(&request_id)
                        .bind(encode_phase(expected_phase))
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(operation_conflict());
                        }
                        query_operation(transaction, &request_id)
                            .await?
                            .ok_or_else(corrupt_storage)
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn finalize_durable_operation<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        expected_phase: DurableOperationPhase,
        outcome: DurableTerminalOutcome,
        resulting_revision: Option<u64>,
        updated_at: UnixTimestamp,
    ) -> BoxFuture<'a, Result<DurableOperationReceipt, SafeError>> {
        Box::pin(async move {
            let resulting_revision = encode_revision(resulting_revision)?;
            let request_id = request_id.as_str().to_owned();
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        if let Some(existing) = query_operation(transaction, &request_id).await?
                            && let Some(receipt) = existing.terminal()
                        {
                            return if receipt.outcome() == outcome
                                && receipt.resulting_revision()
                                    == resulting_revision.map(|value| value as u64)
                            {
                                Ok(receipt.clone())
                            } else {
                                Err(operation_conflict())
                            };
                        }
                        let result = sqlx::query(
                            "UPDATE durable_operations SET phase = 'finalized', \
                             terminal_outcome = ?, resulting_revision = ?, updated_at_unix_s = ?, \
                             completed_at_unix_s = ? \
                             WHERE request_id = ? AND phase = ? AND terminal_outcome IS NULL",
                        )
                        .bind(encode_outcome(outcome))
                        .bind(resulting_revision)
                        .bind(updated_at.as_seconds())
                        .bind(updated_at.as_seconds())
                        .bind(&request_id)
                        .bind(encode_phase(expected_phase))
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(operation_conflict());
                        }
                        query_operation(transaction, &request_id)
                            .await?
                            .and_then(|operation| operation.terminal().cloned())
                            .ok_or_else(corrupt_storage)
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn list_unfinished_durable_operations(
        &self,
    ) -> BoxFuture<'_, Result<Vec<DurableIdentityOperation>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let sql = format!(
                            "{DURABLE_OPERATION_PROJECTION} WHERE terminal_outcome IS NULL \
                             ORDER BY request_id LIMIT {}",
                            HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY + 1
                        );
                        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                            .fetch_all(&mut *transaction)
                            .await
                            .map_err(|_| corrupt_storage())?;
                        if rows.len() > HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY {
                            return Err(corrupt_storage());
                        }
                        rows.iter().map(decode_operation).collect()
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }
}

async fn query_operation(
    transaction: &mut ServiceSqliteTransaction<'_>,
    request_id: &str,
) -> Result<Option<DurableIdentityOperation>, SafeError> {
    let sql = format!("{DURABLE_OPERATION_PROJECTION} WHERE request_id = ? LIMIT 2");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(request_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| corrupt_storage())?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => decode_operation(row).map(Some),
        _ => Err(corrupt_storage()),
    }
}

fn decode_operation(row: &sqlx::sqlite::SqliteRow) -> Result<DurableIdentityOperation, SafeError> {
    let request_id =
        DurableRequestId::parse(required_text(row, "request_id", "request_id_bytes", 36)?)?;
    let kind = decode_kind(&required_text(
        row,
        "operation_kind",
        "operation_kind_bytes",
        6,
    )?)?;
    let identity = required_key(row, "account_public_key", "account_public_key_bytes")?;
    let expected_revision = decode_revision(
        row.try_get("expected_revision")
            .map_err(|_| corrupt_storage())?,
    )?;
    let phase = decode_phase(&required_text(row, "phase", "phase_bytes", 20)?)?;
    let prior_selected = optional_key(
        row,
        "prior_selected_public_key",
        "prior_selected_public_key_bytes",
    )?;
    let updated_at = UnixTimestamp::from_seconds(
        row.try_get("updated_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    let diagnostic = optional_text(row, "diagnostic_code", "diagnostic_code_bytes", 20)?
        .map(|value| decode_diagnostic(&value))
        .transpose()?;
    let outcome = optional_text(row, "terminal_outcome", "terminal_outcome_bytes", 9)?
        .map(|value| decode_outcome(&value))
        .transpose()?;
    let prior_availability = optional_text(
        row,
        "prior_binding_availability",
        "prior_binding_availability_bytes",
        18,
    )?
    .map(|value| decode_availability(&value))
    .transpose()?;
    let resulting_revision = decode_revision(
        row.try_get("resulting_revision")
            .map_err(|_| corrupt_storage())?,
    )?;
    let completed_at = row
        .try_get::<Option<i64>, _>("completed_at_unix_s")
        .map_err(|_| corrupt_storage())?
        .map(|value| UnixTimestamp::from_seconds(value).ok_or_else(corrupt_storage))
        .transpose()?;
    let terminal = match (outcome, completed_at) {
        (Some(outcome), Some(completed_at)) => Some(DurableOperationReceipt::new(
            request_id.clone(),
            identity,
            outcome,
            resulting_revision,
            completed_at,
        )),
        (None, None) if resulting_revision.is_none() => None,
        _ => return Err(corrupt_storage()),
    };
    Ok(DurableIdentityOperation::new(
        request_id,
        kind,
        identity,
        expected_revision,
        phase,
        OperationPriorState::new(prior_selected, prior_availability),
        updated_at,
        diagnostic,
        terminal,
    ))
}

async fn cleanup_expired_terminal_receipts(
    transaction: &mut ServiceSqliteTransaction<'_>,
    now: UnixTimestamp,
) -> Result<(), SafeError> {
    let cutoff = now
        .as_seconds()
        .saturating_sub(HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS);
    let result = sqlx::query(
        "DELETE FROM durable_operations WHERE request_id IN (\
             SELECT request_id FROM durable_operations \
             WHERE completed_at_unix_s IS NOT NULL AND completed_at_unix_s < ? \
             ORDER BY completed_at_unix_s, request_id LIMIT 256\
         )",
    )
    .bind(cutoff)
    .execute(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?;
    if result.rows_affected()
        > u64::try_from(HARVESTCIRCLE_DURABLE_OPERATION_CLEANUP_BATCH)
            .expect("cleanup bound fits in u64")
    {
        return Err(corrupt_storage());
    }
    Ok(())
}

fn required_key(
    row: &sqlx::sqlite::SqliteRow,
    value: &str,
    length: &str,
) -> Result<PublicKey, SafeError> {
    optional_key(row, value, length)?.ok_or_else(corrupt_storage)
}

fn optional_key(
    row: &sqlx::sqlite::SqliteRow,
    value: &str,
    length: &str,
) -> Result<Option<PublicKey>, SafeError> {
    optional_blob(row, value, length, 32)?
        .map(|bytes| {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| corrupt_storage())?;
            PublicKey::from_bytes(bytes).map_err(|_| corrupt_storage())
        })
        .transpose()
}

fn required_text(
    row: &sqlx::sqlite::SqliteRow,
    value: &str,
    length: &str,
    maximum: usize,
) -> Result<String, SafeError> {
    optional_text(row, value, length, maximum)?.ok_or_else(corrupt_storage)
}

fn optional_text(
    row: &sqlx::sqlite::SqliteRow,
    value: &str,
    length: &str,
    maximum: usize,
) -> Result<Option<String>, SafeError> {
    optional_blob(row, value, length, maximum)?
        .map(String::from_utf8)
        .transpose()
        .map_err(|_| corrupt_storage())
}

fn optional_blob(
    row: &sqlx::sqlite::SqliteRow,
    value_column: &str,
    length_column: &str,
    maximum: usize,
) -> Result<Option<Vec<u8>>, SafeError> {
    let value = row
        .try_get::<Option<Vec<u8>>, _>(value_column)
        .map_err(|_| corrupt_storage())?;
    let length = row
        .try_get::<Option<i64>, _>(length_column)
        .map_err(|_| corrupt_storage())?;
    match (value, length) {
        (None, None) => Ok(None),
        (Some(value), Some(length))
            if usize::try_from(length)
                .ok()
                .is_some_and(|length| length <= maximum && length == value.len()) =>
        {
            Ok(Some(value))
        }
        _ => Err(corrupt_storage()),
    }
}

fn encode_revision(value: Option<u64>) -> Result<Option<i64>, SafeError> {
    value
        .map(i64::try_from)
        .transpose()
        .map_err(|_| operation_conflict())
}

fn decode_revision(value: Option<i64>) -> Result<Option<u64>, SafeError> {
    value
        .map(u64::try_from)
        .transpose()
        .map_err(|_| corrupt_storage())
}

const fn encode_kind(value: DurableOperationKind) -> &'static str {
    match value {
        DurableOperationKind::Create => "create",
        DurableOperationKind::Import => "import",
        DurableOperationKind::Repair => "repair",
        DurableOperationKind::Remove => "remove",
    }
}

fn decode_kind(value: &str) -> Result<DurableOperationKind, SafeError> {
    match value {
        "create" => Ok(DurableOperationKind::Create),
        "import" => Ok(DurableOperationKind::Import),
        "repair" => Ok(DurableOperationKind::Repair),
        "remove" => Ok(DurableOperationKind::Remove),
        _ => Err(corrupt_storage()),
    }
}

const fn encode_phase(value: DurableOperationPhase) -> &'static str {
    match value {
        DurableOperationPhase::IntentRecorded => "intent_recorded",
        DurableOperationPhase::CredentialWritten => "credential_written",
        DurableOperationPhase::MetadataCommitted => "metadata_committed",
        DurableOperationPhase::SelectionCommitted => "selection_committed",
        DurableOperationPhase::CompensationPending => "compensation_pending",
        DurableOperationPhase::CredentialDeleted => "credential_deleted",
        DurableOperationPhase::MetadataDeleted => "metadata_deleted",
        DurableOperationPhase::Finalized => "finalized",
    }
}

fn decode_phase(value: &str) -> Result<DurableOperationPhase, SafeError> {
    match value {
        "intent_recorded" => Ok(DurableOperationPhase::IntentRecorded),
        "credential_written" => Ok(DurableOperationPhase::CredentialWritten),
        "metadata_committed" => Ok(DurableOperationPhase::MetadataCommitted),
        "selection_committed" => Ok(DurableOperationPhase::SelectionCommitted),
        "compensation_pending" => Ok(DurableOperationPhase::CompensationPending),
        "credential_deleted" => Ok(DurableOperationPhase::CredentialDeleted),
        "metadata_deleted" => Ok(DurableOperationPhase::MetadataDeleted),
        "finalized" => Ok(DurableOperationPhase::Finalized),
        _ => Err(corrupt_storage()),
    }
}

const fn encode_outcome(value: DurableTerminalOutcome) -> &'static str {
    match value {
        DurableTerminalOutcome::Completed => "completed",
        DurableTerminalOutcome::Cancelled => "cancelled",
        DurableTerminalOutcome::Failed => "failed",
    }
}

fn decode_outcome(value: &str) -> Result<DurableTerminalOutcome, SafeError> {
    match value {
        "completed" => Ok(DurableTerminalOutcome::Completed),
        "cancelled" => Ok(DurableTerminalOutcome::Cancelled),
        "failed" => Ok(DurableTerminalOutcome::Failed),
        _ => Err(corrupt_storage()),
    }
}

const fn encode_diagnostic(value: OperationDiagnostic) -> &'static str {
    match value {
        OperationDiagnostic::StorageUnavailable => "storage_unavailable",
        OperationDiagnostic::KeyringUnavailable => "keyring_unavailable",
        OperationDiagnostic::CredentialMissing => "credential_missing",
        OperationDiagnostic::CompensationFailed => "compensation_failed",
        OperationDiagnostic::Conflict => "conflict",
        OperationDiagnostic::Expired => "expired",
    }
}

fn decode_diagnostic(value: &str) -> Result<OperationDiagnostic, SafeError> {
    match value {
        "storage_unavailable" => Ok(OperationDiagnostic::StorageUnavailable),
        "keyring_unavailable" => Ok(OperationDiagnostic::KeyringUnavailable),
        "credential_missing" => Ok(OperationDiagnostic::CredentialMissing),
        "compensation_failed" => Ok(OperationDiagnostic::CompensationFailed),
        "conflict" => Ok(OperationDiagnostic::Conflict),
        "expired" => Ok(OperationDiagnostic::Expired),
        _ => Err(corrupt_storage()),
    }
}

const fn encode_availability(value: SignerAvailability) -> &'static str {
    match value {
        SignerAvailability::Available => "available",
        SignerAvailability::CredentialMissing => "credential_missing",
        SignerAvailability::StoreUnavailable => "store_unavailable",
    }
}

fn decode_availability(value: &str) -> Result<SignerAvailability, SafeError> {
    match value {
        "available" => Ok(SignerAvailability::Available),
        "credential_missing" => Ok(SignerAvailability::CredentialMissing),
        "store_unavailable" => Ok(SignerAvailability::StoreUnavailable),
        _ => Err(corrupt_storage()),
    }
}

const fn operation_conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The durable operation conflicts with existing state."),
    )
}

const fn operation_capacity_exhausted() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The unfinished operation capacity is exhausted."),
    )
}
