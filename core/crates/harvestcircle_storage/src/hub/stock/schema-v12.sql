-- Preserve the physical v6 table and its foreign keys. SQLite RENAME opens a
-- temp database, which the protected host excludes from migrations.
-- Legacy receiving columns hold the population's initial location; origin_kind
-- distinguishes counted stock from a receipt, never an inferred delivery.
-- The Rust population model supplies the canonical names. Keep SQL spelling
-- stable because both RENAME and schema views exceed the pinned host contract.
ALTER TABLE hub_receipts ADD COLUMN origin_kind TEXT NOT NULL DEFAULT 'receipt' CHECK (length(CAST(origin_kind AS BLOB)) BETWEEN 1 AND 32);
CREATE UNIQUE INDEX hub_population_origin_identity ON hub_receipts(trace_id, origin_kind);
CREATE TABLE hub_counted_openings (
    population_id BLOB NOT NULL PRIMARY KEY CHECK (length(population_id) = 16 AND population_id <> zeroblob(16)),
    origin_kind TEXT NOT NULL DEFAULT 'opening' CHECK (origin_kind = 'opening'),
    history_kind TEXT NOT NULL CHECK (history_kind IN ('unknown', 'external_reference')),
    history_reference TEXT CHECK (history_reference IS NULL OR length(CAST(history_reference AS BLOB)) BETWEEN 1 AND 512),
    FOREIGN KEY (population_id, origin_kind) REFERENCES hub_receipts(trace_id, origin_kind),
    CHECK ((history_kind = 'unknown' AND history_reference IS NULL) OR (history_kind = 'external_reference' AND history_reference IS NOT NULL))
) STRICT;
CREATE TRIGGER hub_counted_opening_no_update BEFORE UPDATE ON hub_counted_openings BEGIN SELECT RAISE(ABORT, 'counted opening history is immutable'); END;
CREATE TRIGGER hub_counted_opening_no_delete BEFORE DELETE ON hub_counted_openings BEGIN SELECT RAISE(ABORT, 'counted opening history is immutable'); END;
