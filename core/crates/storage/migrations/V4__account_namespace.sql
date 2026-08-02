CREATE TABLE account_namespace (
    owner_pubkey TEXT NOT NULL REFERENCES accounts(pubkey) ON DELETE CASCADE,
    preference_key TEXT NOT NULL CHECK (preference_key IN ('namespace_probe')),
    preference_value TEXT NOT NULL CHECK (length(preference_value) <= 4096),
    PRIMARY KEY (owner_pubkey, preference_key)
) STRICT;
