CREATE TABLE hub_count_observations (
    count_id BLOB NOT NULL PRIMARY KEY CHECK (length(count_id)=16 AND count_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    group_revision INTEGER NOT NULL CHECK (group_revision>=1),
    product_id BLOB NOT NULL,
    product_revision INTEGER NOT NULL CHECK (product_revision>=1),
    variant_id BLOB NOT NULL,
    variant_revision INTEGER NOT NULL CHECK (variant_revision>=1),
    location_id BLOB NOT NULL,
    location_revision INTEGER NOT NULL CHECK (location_revision>=1),
    recorded_packs INTEGER NOT NULL CHECK (recorded_packs BETWEEN 1 AND 1000000),
    counted_packs INTEGER NOT NULL CHECK (counted_packs BETWEEN 0 AND 1000000),
    latest_physical_recorded_at_unix_s INTEGER NOT NULL CHECK (latest_physical_recorded_at_unix_s>=0),
    previous_reconciliation_id BLOB CHECK (previous_reconciliation_id IS NULL OR (length(previous_reconciliation_id)=16 AND previous_reconciliation_id<>zeroblob(16))),
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('known','interval','unknown')),
    occurrence_start_unix_s INTEGER CHECK (occurrence_start_unix_s IS NULL OR occurrence_start_unix_s>=0),
    occurrence_end_unix_s INTEGER CHECK (occurrence_end_unix_s IS NULL OR occurrence_end_unix_s>=0),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    FOREIGN KEY (product_id,product_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (variant_id,variant_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (location_id,location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK ((occurrence_kind='unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind='known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s=occurrence_start_unix_s) OR (occurrence_kind='interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_end_unix_s>=occurrence_start_unix_s))
) STRICT;
CREATE INDEX hub_count_observation_group ON hub_count_observations(group_id,operation_id);
CREATE TRIGGER hub_count_observation_no_update BEFORE UPDATE ON hub_count_observations BEGIN SELECT RAISE(ABORT,'count observations are immutable'); END;
CREATE TRIGGER hub_count_observation_no_delete BEFORE DELETE ON hub_count_observations BEGIN SELECT RAISE(ABORT,'count observations are immutable'); END;
