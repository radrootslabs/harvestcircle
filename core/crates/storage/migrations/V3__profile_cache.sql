CREATE TABLE profile_cache (
    subject_pubkey TEXT PRIMARY KEY NOT NULL REFERENCES accounts(pubkey) ON DELETE CASCADE,
    event_id TEXT NOT NULL,
    event_created_at INTEGER NOT NULL,
    name TEXT,
    display_name TEXT,
    nip05 TEXT,
    about TEXT,
    picture TEXT,
    refreshed_at INTEGER NOT NULL,
    refresh_status TEXT NOT NULL CHECK (refresh_status IN ('success', 'offline', 'invalid_data'))
) STRICT;
