use radroots_studio_application::{
    AccountOperationKind, AccountOperationPhase, OperationDiagnostic, OperationId,
    OperationJournal, PendingAccountOperation,
};
use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};
use rusqlite::{Row, params};

use crate::Database;

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
    }
}

fn decode_diagnostic(value: &str) -> rusqlite::Result<OperationDiagnostic> {
    match value {
        "storage_unavailable" => Ok(OperationDiagnostic::StorageUnavailable),
        "keyring_unavailable" => Ok(OperationDiagnostic::KeyringUnavailable),
        "credential_missing" => Ok(OperationDiagnostic::CredentialMissing),
        "compensation_failed" => Ok(OperationDiagnostic::CompensationFailed),
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

#[cfg(test)]
mod tests {
    use radroots_studio_application::{
        AccountOperationKind, AccountOperationPhase, OperationDiagnostic, OperationJournal,
    };
    use radroots_studio_domain::{PublicKey, UnixTimestamp};

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
}
