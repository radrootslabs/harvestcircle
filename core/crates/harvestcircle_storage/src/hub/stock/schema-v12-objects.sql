CREATE TRIGGER hub_counted_opening_no_delete BEFORE DELETE ON hub_counted_openings BEGIN SELECT RAISE(ABORT, 'counted opening history is immutable'); END;
CREATE TRIGGER hub_counted_opening_no_update BEFORE UPDATE ON hub_counted_openings BEGIN SELECT RAISE(ABORT, 'counted opening history is immutable'); END;
CREATE TABLE hub_counted_openings (
    population_id BLOB NOT NULL PRIMARY KEY CHECK (length(population_id) = 16 AND population_id <> zeroblob(16)),
    origin_kind TEXT NOT NULL DEFAULT 'opening' CHECK (origin_kind = 'opening'),
    history_kind TEXT NOT NULL CHECK (history_kind IN ('unknown', 'external_reference')),
    history_reference TEXT CHECK (history_reference IS NULL OR length(CAST(history_reference AS BLOB)) BETWEEN 1 AND 512),
    FOREIGN KEY (population_id, origin_kind) REFERENCES hub_receipts(trace_id, origin_kind),
    CHECK ((history_kind = 'unknown' AND history_reference IS NULL) OR (history_kind = 'external_reference' AND history_reference IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX hub_population_origin_identity ON hub_receipts(trace_id, origin_kind);
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
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512), origin_kind TEXT NOT NULL DEFAULT 'receipt' CHECK (length(CAST(origin_kind AS BLOB)) BETWEEN 1 AND 32),
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
