CREATE TABLE account_preferences (
    owner_public_key TEXT NOT NULL REFERENCES account_identities(public_key) ON DELETE CASCADE,
    preference_key TEXT NOT NULL CHECK (preference_key = 'namespace_probe'),
    preference_value TEXT NOT NULL CHECK (length(preference_value) <= 4096),
    PRIMARY KEY (owner_public_key, preference_key)
) STRICT;

INSERT INTO account_preferences (owner_public_key, preference_key, preference_value)
SELECT owner_pubkey, preference_key, preference_value
FROM account_namespace;
