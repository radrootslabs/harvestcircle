use radroots_studio_application::{
    AccountOperationKind, AccountOperationPhase, DurableAccountOperation, DurableOperationKind,
    DurableOperationPhase, DurableOperationReceipt, DurableOperationRepository,
    DurableOperationStart, DurableRequestId, DurableTerminalOutcome, OperationDiagnostic,
    OperationId, OperationJournal, OperationPriorState, PendingAccountOperation,
};
use radroots_studio_domain::{
    BindingAvailability, PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::Database;

impl DurableOperationRepository for Database {
    fn begin_durable_operation(
        &self,
        request_id: &DurableRequestId,
        kind: DurableOperationKind,
        account: PublicKey,
        expected_revision: Option<u64>,
        prior: OperationPriorState,
        updated_at: UnixTimestamp,
    ) -> Result<DurableOperationStart, SafeError> {
        let encoded_expected_revision = expected_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| operation_conflict())?;
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO durable_operations (request_id, operation_kind, \
                 account_public_key, binding_public_key, expected_revision, phase, \
                 prior_selected_public_key, updated_at, prior_binding_availability) \
                 VALUES (?1, ?2, ?3, ?3, ?4, 'intent_recorded', ?5, ?6, ?7)",
                params![
                    request_id.as_str(),
                    encode_durable_kind(kind),
                    account.to_hex(),
                    encoded_expected_revision,
                    prior.selected_account().map(PublicKey::to_hex),
                    updated_at.as_seconds(),
                    prior
                        .binding_availability()
                        .map(encode_binding_availability),
                ],
            )
            .map_err(|_| storage_error())?;
        let operation =
            query_durable_operation(&transaction, request_id)?.ok_or_else(corrupt_storage_error)?;
        if operation.kind() != kind
            || operation.account() != account
            || operation.expected_revision() != expected_revision
            || operation.prior() != prior
        {
            return Err(operation_conflict());
        }
        transaction.commit().map_err(|_| storage_error())?;
        Ok(if inserted == 1 {
            DurableOperationStart::Started(operation)
        } else {
            DurableOperationStart::Existing(operation)
        })
    }

    fn load_durable_operation(
        &self,
        request_id: &DurableRequestId,
    ) -> Result<Option<DurableAccountOperation>, SafeError> {
        query_durable_operation(&self.connection(), request_id)
    }

    fn advance_durable_operation(
        &self,
        request_id: &DurableRequestId,
        expected_phase: DurableOperationPhase,
        next_phase: DurableOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Result<DurableAccountOperation, SafeError> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        let rows = transaction
            .execute(
                "UPDATE durable_operations SET phase = ?3, updated_at = ?4, diagnostic_code = ?5 \
                 WHERE request_id = ?1 AND phase = ?2 AND terminal_outcome IS NULL",
                params![
                    request_id.as_str(),
                    encode_durable_phase(expected_phase),
                    encode_durable_phase(next_phase),
                    updated_at.as_seconds(),
                    diagnostic.map(encode_diagnostic),
                ],
            )
            .map_err(|_| storage_error())?;
        if rows != 1 {
            return Err(operation_conflict());
        }
        let operation =
            query_durable_operation(&transaction, request_id)?.ok_or_else(corrupt_storage_error)?;
        transaction.commit().map_err(|_| storage_error())?;
        Ok(operation)
    }

    fn finalize_durable_operation(
        &self,
        request_id: &DurableRequestId,
        expected_phase: DurableOperationPhase,
        outcome: DurableTerminalOutcome,
        resulting_revision: Option<u64>,
        updated_at: UnixTimestamp,
    ) -> Result<DurableOperationReceipt, SafeError> {
        if let Some(existing) = self.load_durable_operation(request_id)?
            && let Some(receipt) = existing.terminal()
        {
            return if receipt.outcome() == outcome
                && receipt.resulting_revision() == resulting_revision
            {
                Ok(receipt.clone())
            } else {
                Err(operation_conflict())
            };
        }
        let resulting_revision = resulting_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| operation_conflict())?;
        let rows = self
            .connection()
            .execute(
                "UPDATE durable_operations SET phase = 'finalized', terminal_outcome = ?3, \
                 resulting_revision = ?4, updated_at = ?5 \
                 WHERE request_id = ?1 AND phase = ?2 AND terminal_outcome IS NULL",
                params![
                    request_id.as_str(),
                    encode_durable_phase(expected_phase),
                    encode_terminal_outcome(outcome),
                    resulting_revision,
                    updated_at.as_seconds(),
                ],
            )
            .map_err(|_| storage_error())?;
        if rows != 1 {
            return Err(operation_conflict());
        }
        self.load_durable_operation(request_id)?
            .and_then(|operation| operation.terminal().cloned())
            .ok_or_else(corrupt_storage_error)
    }

    fn list_unfinished_durable_operations(
        &self,
    ) -> Result<Vec<DurableAccountOperation>, SafeError> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "{DURABLE_OPERATION_SELECT} WHERE terminal_outcome IS NULL ORDER BY request_id ASC"
            ))
            .map_err(|_| storage_error())?;
        let rows = statement
            .query_map([], decode_durable_operation)
            .map_err(|_| storage_error())?;
        rows.map(|row| row.map_err(|_| corrupt_storage_error()))
            .collect()
    }
}

const DURABLE_OPERATION_SELECT: &str = "SELECT request_id, operation_kind, account_public_key, \
    expected_revision, phase, prior_selected_public_key, updated_at, diagnostic_code, \
    terminal_outcome, prior_binding_availability, resulting_revision FROM durable_operations";

fn query_durable_operation(
    connection: &rusqlite::Connection,
    request_id: &DurableRequestId,
) -> Result<Option<DurableAccountOperation>, SafeError> {
    connection
        .query_row(
            &format!("{DURABLE_OPERATION_SELECT} WHERE request_id = ?1"),
            [request_id.as_str()],
            decode_durable_operation,
        )
        .optional()
        .map_err(|_| corrupt_storage_error())
}

fn decode_durable_operation(row: &Row<'_>) -> rusqlite::Result<DurableAccountOperation> {
    let request_id =
        DurableRequestId::parse(row.get::<_, String>(0)?).map_err(|_| invalid_column(0))?;
    let kind = decode_durable_kind(row.get::<_, String>(1)?.as_str())?;
    let account =
        PublicKey::from_hex(row.get::<_, String>(2)?.as_str()).map_err(|_| invalid_column(2))?;
    let expected_revision = row
        .get::<_, Option<i64>>(3)?
        .map(|value| u64::try_from(value).map_err(|_| invalid_column(3)))
        .transpose()?;
    let phase = decode_durable_phase(row.get::<_, String>(4)?.as_str())?;
    let prior_selected = row
        .get::<_, Option<String>>(5)?
        .map(|value| PublicKey::from_hex(&value).map_err(|_| invalid_column(5)))
        .transpose()?;
    let updated_at = UnixTimestamp::from_seconds(row.get(6)?).ok_or_else(|| invalid_column(6))?;
    let diagnostic = row
        .get::<_, Option<String>>(7)?
        .map(|value| decode_diagnostic(&value))
        .transpose()?;
    let outcome = row
        .get::<_, Option<String>>(8)?
        .map(|value| decode_terminal_outcome(&value))
        .transpose()?;
    let prior_availability = row
        .get::<_, Option<String>>(9)?
        .map(|value| decode_binding_availability(&value))
        .transpose()?;
    let resulting_revision = row
        .get::<_, Option<i64>>(10)?
        .map(|value| u64::try_from(value).map_err(|_| invalid_column(10)))
        .transpose()?;
    let terminal = outcome.map(|outcome| {
        DurableOperationReceipt::new(request_id.clone(), account, outcome, resulting_revision)
    });
    Ok(DurableAccountOperation::new(
        request_id,
        kind,
        account,
        expected_revision,
        phase,
        OperationPriorState::new(prior_selected, prior_availability),
        updated_at,
        diagnostic,
        terminal,
    ))
}

impl OperationJournal for Database {
    fn begin_operation(
        &self,
        kind: AccountOperationKind,
        subject: PublicKey,
        updated_at: UnixTimestamp,
    ) -> Result<OperationId, SafeError> {
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO operation_journal (operation_kind, subject_pubkey, phase, \
                 updated_at) VALUES (?1, ?2, 'intent_recorded', ?3)",
                params![encode_kind(kind), subject.to_hex(), updated_at.as_seconds()],
            )
            .map_err(|_| storage_error())?;
        let id =
            u64::try_from(connection.last_insert_rowid()).map_err(|_| corrupt_storage_error())?;
        Ok(OperationId::from_raw(id))
    }

    fn update_operation(
        &self,
        id: OperationId,
        phase: AccountOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Result<(), SafeError> {
        let encoded_id = i64::try_from(id.as_raw()).map_err(|_| corrupt_storage_error())?;
        match self.connection().execute(
            "UPDATE operation_journal SET phase = ?2, updated_at = ?3, diagnostic_code = ?4 \
             WHERE operation_id = ?1",
            params![
                encoded_id,
                encode_phase(phase),
                updated_at.as_seconds(),
                diagnostic.map(encode_diagnostic)
            ],
        ) {
            Ok(1) => Ok(()),
            Ok(0) => Err(operation_not_found()),
            Ok(_) | Err(_) => Err(storage_error()),
        }
    }

    fn list_pending_operations(&self) -> Result<Vec<PendingAccountOperation>, SafeError> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT operation_id, operation_kind, subject_pubkey, phase, updated_at, \
                 diagnostic_code FROM operation_journal ORDER BY operation_id ASC",
            )
            .map_err(|_| storage_error())?;
        let rows = statement
            .query_map([], decode_operation)
            .map_err(|_| storage_error())?;
        rows.map(|row| row.map_err(|_| corrupt_storage_error()))
            .collect()
    }

    fn finalize_operation(&self, id: OperationId) -> Result<(), SafeError> {
        let encoded_id = i64::try_from(id.as_raw()).map_err(|_| corrupt_storage_error())?;
        self.connection()
            .execute(
                "DELETE FROM operation_journal WHERE operation_id = ?1",
                [encoded_id],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }
}

fn decode_operation(row: &Row<'_>) -> rusqlite::Result<PendingAccountOperation> {
    let id = u64::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_column(0))?;
    let kind = decode_kind(row.get::<_, String>(1)?.as_str())?;
    let subject =
        PublicKey::from_hex(row.get::<_, String>(2)?.as_str()).map_err(|_| invalid_column(2))?;
    let phase = decode_phase(row.get::<_, String>(3)?.as_str())?;
    let updated_at = UnixTimestamp::from_seconds(row.get(4)?).ok_or_else(|| invalid_column(4))?;
    let diagnostic = row
        .get::<_, Option<String>>(5)?
        .map(|value| decode_diagnostic(&value))
        .transpose()?;
    Ok(PendingAccountOperation::new(
        OperationId::from_raw(id),
        kind,
        subject,
        phase,
        updated_at,
        diagnostic,
    ))
}

const fn encode_durable_kind(value: DurableOperationKind) -> &'static str {
    match value {
        DurableOperationKind::Create => "create",
        DurableOperationKind::Import => "import",
        DurableOperationKind::Repair => "repair",
        DurableOperationKind::Remove => "remove",
    }
}

fn decode_durable_kind(value: &str) -> rusqlite::Result<DurableOperationKind> {
    match value {
        "create" => Ok(DurableOperationKind::Create),
        "import" => Ok(DurableOperationKind::Import),
        "repair" => Ok(DurableOperationKind::Repair),
        "remove" => Ok(DurableOperationKind::Remove),
        _ => Err(invalid_column(1)),
    }
}

const fn encode_durable_phase(value: DurableOperationPhase) -> &'static str {
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

fn decode_durable_phase(value: &str) -> rusqlite::Result<DurableOperationPhase> {
    match value {
        "intent_recorded" => Ok(DurableOperationPhase::IntentRecorded),
        "credential_written" => Ok(DurableOperationPhase::CredentialWritten),
        "metadata_committed" => Ok(DurableOperationPhase::MetadataCommitted),
        "selection_committed" => Ok(DurableOperationPhase::SelectionCommitted),
        "compensation_pending" => Ok(DurableOperationPhase::CompensationPending),
        "credential_deleted" => Ok(DurableOperationPhase::CredentialDeleted),
        "metadata_deleted" => Ok(DurableOperationPhase::MetadataDeleted),
        "finalized" => Ok(DurableOperationPhase::Finalized),
        _ => Err(invalid_column(4)),
    }
}

const fn encode_terminal_outcome(value: DurableTerminalOutcome) -> &'static str {
    match value {
        DurableTerminalOutcome::Completed => "completed",
        DurableTerminalOutcome::Cancelled => "cancelled",
        DurableTerminalOutcome::Failed => "failed",
    }
}

fn decode_terminal_outcome(value: &str) -> rusqlite::Result<DurableTerminalOutcome> {
    match value {
        "completed" => Ok(DurableTerminalOutcome::Completed),
        "cancelled" => Ok(DurableTerminalOutcome::Cancelled),
        "failed" => Ok(DurableTerminalOutcome::Failed),
        _ => Err(invalid_column(8)),
    }
}

const fn encode_binding_availability(value: BindingAvailability) -> &'static str {
    match value {
        BindingAvailability::Available => "available",
        BindingAvailability::CredentialMissing => "credential_missing",
        BindingAvailability::StoreUnavailable => "store_unavailable",
    }
}

fn decode_binding_availability(value: &str) -> rusqlite::Result<BindingAvailability> {
    match value {
        "available" => Ok(BindingAvailability::Available),
        "credential_missing" => Ok(BindingAvailability::CredentialMissing),
        "store_unavailable" => Ok(BindingAvailability::StoreUnavailable),
        _ => Err(invalid_column(9)),
    }
}

const fn encode_kind(value: AccountOperationKind) -> &'static str {
    match value {
        AccountOperationKind::Add => "add",
        AccountOperationKind::Import => "import",
        AccountOperationKind::Remove => "remove",
    }
}

fn decode_kind(value: &str) -> rusqlite::Result<AccountOperationKind> {
    match value {
        "add" => Ok(AccountOperationKind::Add),
        "import" => Ok(AccountOperationKind::Import),
        "remove" => Ok(AccountOperationKind::Remove),
        _ => Err(invalid_column(1)),
    }
}

const fn encode_phase(value: AccountOperationPhase) -> &'static str {
    match value {
        AccountOperationPhase::IntentRecorded => "intent_recorded",
        AccountOperationPhase::CredentialWritten => "credential_written",
        AccountOperationPhase::MetadataCommitted => "metadata_committed",
        AccountOperationPhase::CompensationPending => "compensation_pending",
        AccountOperationPhase::CredentialDeleted => "credential_deleted",
        AccountOperationPhase::MetadataDeleted => "metadata_deleted",
    }
}

fn decode_phase(value: &str) -> rusqlite::Result<AccountOperationPhase> {
    match value {
        "intent_recorded" => Ok(AccountOperationPhase::IntentRecorded),
        "credential_written" => Ok(AccountOperationPhase::CredentialWritten),
        "metadata_committed" => Ok(AccountOperationPhase::MetadataCommitted),
        "compensation_pending" => Ok(AccountOperationPhase::CompensationPending),
        "credential_deleted" => Ok(AccountOperationPhase::CredentialDeleted),
        "metadata_deleted" => Ok(AccountOperationPhase::MetadataDeleted),
        _ => Err(invalid_column(3)),
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

fn decode_diagnostic(value: &str) -> rusqlite::Result<OperationDiagnostic> {
    match value {
        "storage_unavailable" => Ok(OperationDiagnostic::StorageUnavailable),
        "keyring_unavailable" => Ok(OperationDiagnostic::KeyringUnavailable),
        "credential_missing" => Ok(OperationDiagnostic::CredentialMissing),
        "compensation_failed" => Ok(OperationDiagnostic::CompensationFailed),
        "conflict" => Ok(OperationDiagnostic::Conflict),
        "expired" => Ok(OperationDiagnostic::Expired),
        _ => Err(invalid_column(5)),
    }
}

fn invalid_column(index: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        index,
        "account operation journal".to_owned(),
        rusqlite::types::Type::Text,
    )
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The account recovery journal is unavailable."),
    )
}

const fn corrupt_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The account recovery journal could not be read."),
    )
}

const fn operation_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::PendingOperationRecoveryRequired,
        SafeMessage::new("The account recovery operation was not found."),
    )
}

const fn operation_conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The durable account operation conflicts with existing state."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_application::{
        AccountOperationKind, AccountOperationPhase, DurableOperationKind, DurableOperationPhase,
        DurableOperationRepository, DurableOperationStart, DurableRequestId,
        DurableTerminalOutcome, OperationDiagnostic, OperationJournal, OperationPriorState,
    };
    use radroots_studio_domain::{BindingAvailability, PublicKey, UnixTimestamp};

    use crate::Database;

    #[test]
    fn journal_creates_advances_loads_and_finalizes_pending_operations() {
        let database = Database::in_memory().expect("database");
        let subject = PublicKey::from_bytes([7; 32]);
        let id = database
            .begin_operation(
                AccountOperationKind::Import,
                subject,
                UnixTimestamp::from_seconds(10).expect("time"),
            )
            .expect("begin");
        database
            .update_operation(
                id,
                AccountOperationPhase::CompensationPending,
                UnixTimestamp::from_seconds(11).expect("time"),
                Some(OperationDiagnostic::KeyringUnavailable),
            )
            .expect("advance");

        let pending = database.list_pending_operations().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].subject(), subject);
        assert_eq!(pending[0].kind(), AccountOperationKind::Import);
        assert_eq!(
            pending[0].phase(),
            AccountOperationPhase::CompensationPending
        );
        assert_eq!(
            pending[0].diagnostic(),
            Some(OperationDiagnostic::KeyringUnavailable)
        );

        database.finalize_operation(id).expect("finalize");
        assert!(
            database
                .list_pending_operations()
                .expect("pending")
                .is_empty()
        );
    }

    #[test]
    fn journal_schema_and_rows_exclude_secret_payload_columns() {
        let database = Database::in_memory().expect("database");
        database
            .begin_operation(
                AccountOperationKind::Remove,
                PublicKey::from_bytes([8; 32]),
                UnixTimestamp::from_seconds(12).expect("time"),
            )
            .expect("begin");
        let connection = database.connection();
        let schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'operation_journal'",
                [],
                |row| row.get(0),
            )
            .expect("schema");
        assert!(!schema.contains("secret"));
        assert!(!schema.contains("payload"));
    }

    #[test]
    fn durable_repository_replays_matching_requests_and_retains_terminal_receipts() {
        let database = Database::in_memory().expect("database");
        let request = DurableRequestId::parse("import:test:1").expect("request");
        let account = PublicKey::from_bytes([9; 32]);
        let prior = OperationPriorState::new(
            Some(PublicKey::from_bytes([8; 32])),
            Some(BindingAvailability::CredentialMissing),
        );
        let started = database
            .begin_durable_operation(
                &request,
                DurableOperationKind::Repair,
                account,
                Some(4),
                prior,
                UnixTimestamp::from_seconds(10).expect("time"),
            )
            .expect("begin");
        assert!(matches!(started, DurableOperationStart::Started(_)));
        let replay = database
            .begin_durable_operation(
                &request,
                DurableOperationKind::Repair,
                account,
                Some(4),
                prior,
                UnixTimestamp::from_seconds(11).expect("time"),
            )
            .expect("replay");
        assert!(matches!(replay, DurableOperationStart::Existing(_)));
        assert!(
            database
                .begin_durable_operation(
                    &request,
                    DurableOperationKind::Remove,
                    account,
                    Some(4),
                    prior,
                    UnixTimestamp::from_seconds(11).expect("time"),
                )
                .is_err()
        );
        database
            .advance_durable_operation(
                &request,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialWritten,
                UnixTimestamp::from_seconds(12).expect("time"),
                None,
            )
            .expect("advance");
        let receipt = database
            .finalize_durable_operation(
                &request,
                DurableOperationPhase::CredentialWritten,
                DurableTerminalOutcome::Completed,
                Some(5),
                UnixTimestamp::from_seconds(13).expect("time"),
            )
            .expect("finalize");
        assert_eq!(receipt.resulting_revision(), Some(5));
        assert_eq!(
            database
                .finalize_durable_operation(
                    &request,
                    DurableOperationPhase::CredentialWritten,
                    DurableTerminalOutcome::Completed,
                    Some(5),
                    UnixTimestamp::from_seconds(14).expect("time"),
                )
                .expect("receipt replay"),
            receipt
        );
        assert!(
            database
                .list_unfinished_durable_operations()
                .expect("unfinished")
                .is_empty()
        );
    }
}
