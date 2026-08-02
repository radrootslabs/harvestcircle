CREATE TABLE operation_journal (
    operation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_kind TEXT NOT NULL CHECK (operation_kind IN ('add', 'import', 'remove')),
    subject_pubkey TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('intent_recorded', 'credential_written', 'metadata_committed', 'compensation_pending', 'credential_deleted', 'metadata_deleted')),
    updated_at INTEGER NOT NULL,
    diagnostic_code TEXT CHECK (diagnostic_code IN ('storage_unavailable', 'keyring_unavailable', 'credential_missing', 'compensation_failed'))
) STRICT;
