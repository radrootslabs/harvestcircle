INSERT INTO account_identities (
    public_key,
    npub,
    label,
    created_at,
    last_used_at
)
SELECT pubkey, npub, label, created_at, last_used_at
FROM accounts;

INSERT INTO local_signer_bindings (
    account_public_key,
    binding_public_key,
    binding_kind,
    availability
)
SELECT pubkey, pubkey, 'local_secret', key_availability
FROM accounts;

UPDATE runtime_state
SET selected_public_key = (
    SELECT selected_pubkey FROM app_state WHERE singleton = 1
)
WHERE singleton = 1;

INSERT INTO profile_cache_v6 (
    subject_public_key,
    event_id,
    event_created_at,
    name,
    display_name,
    nip05,
    about,
    picture,
    refreshed_at,
    refresh_status
)
SELECT
    subject_pubkey,
    event_id,
    event_created_at,
    name,
    display_name,
    nip05,
    about,
    picture,
    refreshed_at,
    refresh_status
FROM profile_cache;

INSERT INTO durable_operations (
    request_id,
    operation_kind,
    account_public_key,
    binding_public_key,
    phase,
    updated_at,
    diagnostic_code
)
SELECT
    'legacy-v5-' || operation_id,
    CASE operation_kind WHEN 'add' THEN 'create' ELSE operation_kind END,
    subject_pubkey,
    subject_pubkey,
    phase,
    updated_at,
    diagnostic_code
FROM operation_journal;
