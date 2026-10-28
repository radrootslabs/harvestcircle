CREATE TABLE hub_stock_state (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    hub_id BLOB NOT NULL UNIQUE REFERENCES hub_context(hub_id) CHECK (length(hub_id) = 16),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    event_count INTEGER NOT NULL CHECK (event_count BETWEEN 0 AND 32768)
) STRICT;
CREATE TABLE hub_stock_operations (
    operation_id TEXT NOT NULL PRIMARY KEY CHECK (length(CAST(operation_id AS BLOB)) = 36),
    hub_id BLOB NOT NULL REFERENCES hub_context(hub_id) CHECK (length(hub_id) = 16),
    actor_public_key BLOB NOT NULL CHECK (length(actor_public_key) = 32),
    kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 32),
    intent_hash BLOB NOT NULL CHECK (length(intent_hash) = 32),
    catalog_revision INTEGER NOT NULL CHECK (catalog_revision >= 1),
    stock_revision INTEGER NOT NULL UNIQUE CHECK (stock_revision >= 2),
    result_id BLOB NOT NULL CHECK (length(result_id) = 16 AND result_id <> zeroblob(16)),
    recorded_at_unix_s INTEGER NOT NULL CHECK (recorded_at_unix_s >= 0)
) STRICT;
CREATE TABLE hub_source_references (
    source_id BLOB NOT NULL PRIMARY KEY CHECK (length(source_id) = 16 AND source_id <> zeroblob(16)),
    hub_id BLOB NOT NULL REFERENCES hub_context(hub_id) CHECK (length(hub_id) = 16),
    issuer_id BLOB,
    issuer_revision INTEGER,
    kind TEXT NOT NULL CHECK (kind IN ('producer_batch', 'supplier_reference', 'other')),
    source_code TEXT NOT NULL CHECK (length(CAST(source_code AS BLOB)) BETWEEN 1 AND 128),
    provenance TEXT NOT NULL CHECK (length(CAST(provenance AS BLOB)) BETWEEN 1 AND 512),
    created_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    FOREIGN KEY (issuer_id, issuer_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    CHECK ((issuer_id IS NULL AND issuer_revision IS NULL) OR (issuer_id IS NOT NULL AND issuer_revision IS NOT NULL AND length(issuer_id) = 16 AND issuer_revision >= 1))
) STRICT;
CREATE TABLE hub_receipts (
    trace_id BLOB NOT NULL PRIMARY KEY CHECK (length(trace_id) = 16 AND trace_id <> zeroblob(16)),
    root_group_id BLOB NOT NULL UNIQUE CHECK (length(root_group_id) = 16 AND root_group_id <> zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    variant_id BLOB NOT NULL,
    variant_revision INTEGER NOT NULL CHECK (variant_revision >= 1),
    product_id BLOB NOT NULL,
    product_revision INTEGER NOT NULL CHECK (product_revision >= 1),
    supplier_id BLOB,
    supplier_revision INTEGER,
    producer_id BLOB,
    producer_revision INTEGER,
    source_id BLOB REFERENCES hub_source_references(source_id),
    source_certainty TEXT CHECK (source_certainty IS NULL OR source_certainty IN ('possible', 'established')),
    source_issuer_revision INTEGER CHECK (source_issuer_revision IS NULL OR source_issuer_revision >= 1),
    original_packs INTEGER NOT NULL CHECK (original_packs BETWEEN 1 AND 1000000),
    receiving_id BLOB NOT NULL,
    receiving_revision INTEGER NOT NULL CHECK (receiving_revision >= 1),
    planned_location_id BLOB,
    planned_location_revision INTEGER,
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('known', 'interval', 'unknown')),
    occurrence_start_unix_s INTEGER CHECK (occurrence_start_unix_s IS NULL OR occurrence_start_unix_s >= 0),
    occurrence_end_unix_s INTEGER CHECK (occurrence_end_unix_s IS NULL OR occurrence_end_unix_s >= 0),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    FOREIGN KEY (variant_id, variant_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (product_id, product_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (supplier_id, supplier_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (producer_id, producer_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (receiving_id, receiving_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (planned_location_id, planned_location_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    CHECK ((supplier_id IS NULL AND supplier_revision IS NULL) OR (supplier_id IS NOT NULL AND supplier_revision IS NOT NULL AND supplier_revision >= 1)),
    CHECK ((producer_id IS NULL AND producer_revision IS NULL) OR (producer_id IS NOT NULL AND producer_revision IS NOT NULL AND producer_revision >= 1)),
    CHECK ((planned_location_id IS NULL AND planned_location_revision IS NULL) OR (planned_location_id IS NOT NULL AND planned_location_revision IS NOT NULL AND planned_location_revision >= 1 AND planned_location_id <> receiving_id)),
    CHECK ((source_id IS NULL AND source_certainty IS NULL AND source_issuer_revision IS NULL) OR (source_id IS NOT NULL AND source_certainty IS NOT NULL)),
    CHECK ((occurrence_kind = 'unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind = 'known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s = occurrence_start_unix_s) OR (occurrence_kind = 'interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s >= occurrence_start_unix_s))
) STRICT;
CREATE TABLE hub_stock_groups (
    group_id BLOB NOT NULL PRIMARY KEY CHECK (length(group_id) = 16 AND group_id <> zeroblob(16)),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id) CHECK (length(trace_id) = 16),
    revision INTEGER NOT NULL CHECK (revision >= 1)
) STRICT;
CREATE TABLE hub_stock_positions (
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    location_id BLOB NOT NULL CHECK (length(location_id) = 16),
    location_revision INTEGER NOT NULL CHECK (location_revision >= 1),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 0 AND 1000000),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (group_id, location_id),
    FOREIGN KEY (location_id, location_revision) REFERENCES hub_reference_versions(record_id, record_revision)
) STRICT;
CREATE TABLE hub_putaway_work (
    trace_id BLOB NOT NULL PRIMARY KEY REFERENCES hub_receipts(trace_id),
    required INTEGER NOT NULL CHECK (required BETWEEN 1 AND 1000000),
    completed INTEGER NOT NULL CHECK (completed BETWEEN 0 AND 1000000),
    no_longer_required INTEGER NOT NULL CHECK (no_longer_required BETWEEN 0 AND 1000000),
    remaining INTEGER NOT NULL CHECK (remaining BETWEEN 0 AND 1000000),
    target_location_id BLOB,
    target_location_revision INTEGER,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    FOREIGN KEY (target_location_id, target_location_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    CHECK (required = completed + no_longer_required + remaining),
    CHECK ((target_location_id IS NULL AND target_location_revision IS NULL) OR (target_location_id IS NOT NULL AND target_location_revision IS NOT NULL AND target_location_revision >= 1))
) STRICT;
CREATE TABLE hub_stock_events (
    sequence INTEGER NOT NULL PRIMARY KEY CHECK (sequence BETWEEN 1 AND 32768),
    operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 32),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 0 AND 1000000),
    from_location_id BLOB CHECK (from_location_id IS NULL OR length(from_location_id) = 16),
    from_location_revision INTEGER CHECK (from_location_revision IS NULL OR from_location_revision >= 1),
    to_location_id BLOB CHECK (to_location_id IS NULL OR length(to_location_id) = 16),
    to_location_revision INTEGER CHECK (to_location_revision IS NULL OR to_location_revision >= 1),
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('known', 'interval', 'unknown')),
    occurrence_start_unix_s INTEGER CHECK (occurrence_start_unix_s IS NULL OR occurrence_start_unix_s >= 0),
    occurrence_end_unix_s INTEGER CHECK (occurrence_end_unix_s IS NULL OR occurrence_end_unix_s >= 0),
    FOREIGN KEY (from_location_id, from_location_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    FOREIGN KEY (to_location_id, to_location_revision) REFERENCES hub_reference_versions(record_id, record_revision),
    CHECK ((from_location_id IS NULL AND from_location_revision IS NULL) OR (from_location_id IS NOT NULL AND from_location_revision IS NOT NULL)),
    CHECK ((to_location_id IS NULL AND to_location_revision IS NULL) OR (to_location_id IS NOT NULL AND to_location_revision IS NOT NULL)),
    CHECK ((occurrence_kind = 'unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind = 'known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s = occurrence_start_unix_s) OR (occurrence_kind = 'interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s >= occurrence_start_unix_s))
) STRICT;
CREATE TRIGGER hub_stock_operations_no_update BEFORE UPDATE ON hub_stock_operations BEGIN SELECT RAISE(ABORT, 'stock operation history is immutable'); END;
CREATE TRIGGER hub_stock_operations_no_delete BEFORE DELETE ON hub_stock_operations BEGIN SELECT RAISE(ABORT, 'stock operation history is immutable'); END;
CREATE TRIGGER hub_source_references_no_update BEFORE UPDATE ON hub_source_references BEGIN SELECT RAISE(ABORT, 'source identity is immutable'); END;
CREATE TRIGGER hub_source_references_no_delete BEFORE DELETE ON hub_source_references BEGIN SELECT RAISE(ABORT, 'source identity is immutable'); END;
CREATE TRIGGER hub_receipts_no_update BEFORE UPDATE ON hub_receipts BEGIN SELECT RAISE(ABORT, 'original receipt is immutable'); END;
CREATE TRIGGER hub_receipts_no_delete BEFORE DELETE ON hub_receipts BEGIN SELECT RAISE(ABORT, 'original receipt is immutable'); END;
CREATE TRIGGER hub_stock_events_no_update BEFORE UPDATE ON hub_stock_events BEGIN SELECT RAISE(ABORT, 'stock event history is immutable'); END;
CREATE TRIGGER hub_stock_events_no_delete BEFORE DELETE ON hub_stock_events BEGIN SELECT RAISE(ABORT, 'stock event history is immutable'); END;
INSERT INTO hub_stock_state (singleton, hub_id, revision, event_count) SELECT 1, hub_id, 1, 0 FROM hub_context;
