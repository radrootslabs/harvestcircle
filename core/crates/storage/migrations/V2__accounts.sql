CREATE TABLE accounts (
    pubkey TEXT PRIMARY KEY NOT NULL CHECK (
        length(pubkey) = 64 AND pubkey = lower(pubkey)
    ),
    npub TEXT NOT NULL CHECK (length(npub) = 63),
    signer_kind TEXT NOT NULL CHECK (
        signer_kind IN ('local_secret', 'watch_only', 'remote_nip46')
    ),
    key_availability TEXT NOT NULL CHECK (
        key_availability IN (
            'available',
            'credential_missing',
            'store_unavailable',
            'not_required'
        )
    ),
    label TEXT,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    last_used_at INTEGER CHECK (last_used_at >= 0)
);

CREATE TABLE app_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    selected_pubkey TEXT REFERENCES accounts(pubkey) ON DELETE SET NULL
);

INSERT INTO app_state (singleton, selected_pubkey) VALUES (1, NULL);
UPDATE application_schema SET schema_version = 2 WHERE singleton = 1;
