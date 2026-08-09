CREATE TABLE installation_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    installation_id TEXT NOT NULL CHECK (
        length(installation_id) = 32
        AND installation_id NOT GLOB '*[^0-9a-f]*'
    )
) STRICT;
