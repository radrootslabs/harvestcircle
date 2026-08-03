CREATE TABLE account_identities (
    public_key TEXT PRIMARY KEY NOT NULL CHECK (
        length(public_key) = 64 AND public_key = lower(public_key)
    ),
    npub TEXT NOT NULL UNIQUE CHECK (length(npub) = 63),
    label TEXT CHECK (label IS NULL OR length(label) BETWEEN 1 AND 80),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    last_used_at INTEGER CHECK (last_used_at IS NULL OR last_used_at >= 0)
) STRICT;

CREATE TABLE local_signer_bindings (
    account_public_key TEXT NOT NULL,
    binding_public_key TEXT NOT NULL,
    binding_kind TEXT NOT NULL CHECK (binding_kind = 'local_secret'),
    availability TEXT NOT NULL CHECK (
        availability IN ('available', 'credential_missing', 'store_unavailable')
    ),
    PRIMARY KEY (account_public_key, binding_public_key),
    UNIQUE (account_public_key, binding_kind),
    FOREIGN KEY (account_public_key) REFERENCES account_identities(public_key) ON DELETE CASCADE,
    CHECK (account_public_key = binding_public_key)
) STRICT;

CREATE TABLE runtime_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    selected_public_key TEXT REFERENCES account_identities(public_key) ON DELETE SET NULL,
    active_account_public_key TEXT,
    active_binding_public_key TEXT,
    session_generation INTEGER NOT NULL DEFAULT 0 CHECK (session_generation >= 0),
    FOREIGN KEY (active_account_public_key, active_binding_public_key)
        REFERENCES local_signer_bindings(account_public_key, binding_public_key)
        ON DELETE SET NULL,
    CHECK (
        (active_account_public_key IS NULL AND active_binding_public_key IS NULL)
        OR
        (active_account_public_key IS NOT NULL AND active_binding_public_key IS NOT NULL)
    )
) STRICT;

INSERT INTO runtime_state (singleton) VALUES (1);

CREATE TABLE profile_cache_v6 (
    subject_public_key TEXT PRIMARY KEY NOT NULL
        REFERENCES account_identities(public_key) ON DELETE CASCADE,
    event_id TEXT NOT NULL CHECK (length(event_id) = 64 AND event_id = lower(event_id)),
    event_created_at INTEGER NOT NULL CHECK (event_created_at >= 0),
    name TEXT,
    display_name TEXT,
    nip05 TEXT,
    about TEXT,
    picture TEXT,
    refreshed_at INTEGER NOT NULL CHECK (refreshed_at >= 0),
    refresh_status TEXT NOT NULL CHECK (
        refresh_status IN ('success', 'offline', 'invalid_data')
    )
) STRICT;

CREATE TABLE durable_operations (
    request_id TEXT PRIMARY KEY NOT NULL CHECK (length(request_id) BETWEEN 1 AND 128),
    operation_kind TEXT NOT NULL CHECK (
        operation_kind IN ('create', 'import', 'repair', 'remove')
    ),
    account_public_key TEXT NOT NULL CHECK (
        length(account_public_key) = 64 AND account_public_key = lower(account_public_key)
    ),
    binding_public_key TEXT NOT NULL CHECK (binding_public_key = account_public_key),
    expected_revision INTEGER CHECK (expected_revision IS NULL OR expected_revision >= 0),
    phase TEXT NOT NULL CHECK (
        phase IN (
            'intent_recorded',
            'credential_written',
            'metadata_committed',
            'selection_committed',
            'compensation_pending',
            'credential_deleted',
            'metadata_deleted',
            'finalized'
        )
    ),
    terminal_outcome TEXT CHECK (
        terminal_outcome IS NULL OR terminal_outcome IN ('completed', 'cancelled', 'failed')
    ),
    prior_selected_public_key TEXT,
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    diagnostic_code TEXT CHECK (
        diagnostic_code IS NULL OR diagnostic_code IN (
            'storage_unavailable',
            'keyring_unavailable',
            'credential_missing',
            'compensation_failed',
            'conflict',
            'expired'
        )
    ),
    CHECK (
        (phase = 'finalized' AND terminal_outcome IS NOT NULL)
        OR
        (phase <> 'finalized' AND terminal_outcome IS NULL)
    )
) STRICT;
