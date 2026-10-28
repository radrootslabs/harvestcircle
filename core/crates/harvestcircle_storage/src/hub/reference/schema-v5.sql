CREATE TABLE hub_reference_versions (
    record_id BLOB NOT NULL CHECK (length(record_id) = 16 AND record_id <> zeroblob(16)),
    record_revision INTEGER NOT NULL CHECK (record_revision >= 1),
    catalog_revision INTEGER NOT NULL UNIQUE CHECK (catalog_revision BETWEEN 2 AND 16385),
    hub_id BLOB NOT NULL REFERENCES hub_context(hub_id) CHECK (length(hub_id) = 16),
    kind TEXT NOT NULL CHECK (kind IN ('product', 'pack', 'party', 'location')),
    label TEXT NOT NULL CHECK (length(CAST(label AS BLOB)) BETWEEN 1 AND 120),
    product_id BLOB CHECK (product_id IS NULL OR length(product_id) = 16),
    nominal_grams INTEGER CHECK (nominal_grams IS NULL OR nominal_grams BETWEEN 1 AND 1000000000),
    location_purpose TEXT CHECK (location_purpose IS NULL OR location_purpose IN ('receiving', 'storage', 'other')),
    actor_public_key BLOB NOT NULL CHECK (length(actor_public_key) = 32),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(CAST(operation_id AS BLOB)) = 36),
    intent_hash BLOB NOT NULL CHECK (length(intent_hash) = 32),
    recorded_at_unix_s INTEGER NOT NULL CHECK (recorded_at_unix_s >= 0),
    PRIMARY KEY (record_id, record_revision),
    CHECK (record_revision < catalog_revision),
    CHECK ((kind = 'pack' AND product_id IS NOT NULL AND location_purpose IS NULL) OR (kind = 'location' AND product_id IS NULL AND nominal_grams IS NULL AND location_purpose IS NOT NULL) OR (kind IN ('product', 'party') AND product_id IS NULL AND nominal_grams IS NULL AND location_purpose IS NULL))
) STRICT;
CREATE TRIGGER hub_reference_no_update BEFORE UPDATE ON hub_reference_versions BEGIN SELECT RAISE(ABORT, 'hub reference history is immutable'); END;
CREATE TRIGGER hub_reference_no_delete BEFORE DELETE ON hub_reference_versions BEGIN SELECT RAISE(ABORT, 'hub reference history is immutable'); END;
CREATE TABLE hub_handling_context (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    hub_id BLOB NOT NULL UNIQUE REFERENCES hub_context(hub_id) CHECK (length(hub_id) = 16),
    basis_id TEXT NOT NULL CHECK (basis_id = 'whole-pack-demo-v1'),
    basis_version INTEGER NOT NULL CHECK (basis_version = 1),
    site_timezone TEXT NOT NULL CHECK (site_timezone = 'UTC')
) STRICT;
CREATE TRIGGER hub_handling_no_update BEFORE UPDATE ON hub_handling_context BEGIN SELECT RAISE(ABORT, 'demo handling context is immutable'); END;
CREATE TRIGGER hub_handling_no_delete BEFORE DELETE ON hub_handling_context BEGIN SELECT RAISE(ABORT, 'demo handling context is immutable'); END;
INSERT INTO hub_handling_context (singleton, hub_id, basis_id, basis_version, site_timezone) SELECT 1, hub_id, 'whole-pack-demo-v1', 1, 'UTC' FROM hub_context;
