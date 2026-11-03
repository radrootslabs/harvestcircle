CREATE TABLE hub_actual_reports (
    report_id BLOB NOT NULL PRIMARY KEY CHECK (length(report_id)=16 AND report_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    policy_version INTEGER NOT NULL CHECK (policy_version=1),
    variant_id BLOB CHECK (variant_id IS NULL OR (length(variant_id)=16 AND variant_id<>zeroblob(16))),
    recipient_id BLOB CHECK (recipient_id IS NULL OR (length(recipient_id)=16 AND recipient_id<>zeroblob(16))),
    packs INTEGER CHECK (packs IS NULL OR packs BETWEEN 1 AND 1000000),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('unknown','possible','claim')),
    source_count INTEGER NOT NULL CHECK (source_count BETWEEN 0 AND 64),
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('unknown','known','interval')),
    occurrence_start_unix_s INTEGER,
    occurrence_end_unix_s INTEGER,
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    CHECK ((source_kind='unknown' AND source_count=0) OR (source_kind='possible' AND source_count BETWEEN 1 AND 64) OR (source_kind='claim' AND source_count=1)),
    CHECK ((occurrence_kind='unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind='known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_end_unix_s=occurrence_start_unix_s) OR (occurrence_kind='interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_end_unix_s>=occurrence_start_unix_s))
) STRICT;
CREATE TABLE hub_actual_report_sources (
    report_id BLOB NOT NULL REFERENCES hub_actual_reports(report_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    trace_id BLOB NOT NULL CHECK (length(trace_id)=16 AND trace_id<>zeroblob(16)),
    group_id BLOB CHECK (group_id IS NULL OR (length(group_id)=16 AND group_id<>zeroblob(16))),
    location_id BLOB CHECK (location_id IS NULL OR (length(location_id)=16 AND location_id<>zeroblob(16))),
    PRIMARY KEY (report_id,ordinal)
) STRICT;
CREATE INDEX hub_actual_source_claims ON hub_actual_report_sources(trace_id,report_id);
CREATE TABLE hub_actual_reviews (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    report_id BLOB NOT NULL UNIQUE REFERENCES hub_actual_reports(report_id),
    policy_version INTEGER NOT NULL CHECK (policy_version=1),
    review_kind TEXT NOT NULL CHECK (review_kind IN ('departure','link')),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    variant_id BLOB NOT NULL,
    variant_revision INTEGER NOT NULL CHECK (variant_revision>=1),
    recipient_id BLOB NOT NULL,
    recipient_revision INTEGER NOT NULL CHECK (recipient_revision>=1),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    source_operation_id TEXT REFERENCES hub_stock_operations(operation_id),
    source_group_id BLOB REFERENCES hub_stock_groups(group_id),
    known_operation_id TEXT REFERENCES hub_stock_operations(operation_id),
    before_group_revision INTEGER,
    after_group_revision INTEGER,
    before_packs INTEGER,
    before_causal_floor_unix_s INTEGER CHECK (before_causal_floor_unix_s IS NULL OR before_causal_floor_unix_s>=0),
    after_causal_floor_unix_s INTEGER CHECK (after_causal_floor_unix_s IS NULL OR after_causal_floor_unix_s>=0),
    historical_location_id BLOB,
    historical_location_revision INTEGER,
    projection_hash BLOB,
    current_location_id BLOB,
    current_location_revision INTEGER,
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('unknown','known','interval')),
    occurrence_start_unix_s INTEGER,
    occurrence_end_unix_s INTEGER,
    source_evidence TEXT,
    quantity_evidence TEXT,
    recipient_evidence TEXT,
    timing_evidence TEXT,
    verification TEXT NOT NULL CHECK (length(CAST(verification AS BLOB)) BETWEEN 1 AND 512),
    lineage_count INTEGER NOT NULL CHECK (lineage_count BETWEEN 0 AND 256),
    scope_count INTEGER NOT NULL CHECK (scope_count BETWEEN 0 AND 64),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    FOREIGN KEY (variant_id,variant_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (recipient_id,recipient_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (historical_location_id,historical_location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (current_location_id,current_location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK ((review_kind='departure' AND historical_location_id IS NOT NULL AND historical_location_revision IS NOT NULL AND historical_location_revision>=1 AND projection_hash IS NOT NULL AND length(projection_hash)=32 AND source_operation_id IS NOT NULL AND source_group_id IS NOT NULL AND known_operation_id IS NULL AND before_group_revision IS NOT NULL AND after_group_revision IS NOT NULL AND before_packs IS NOT NULL AND before_group_revision>=1 AND after_group_revision=before_group_revision+1 AND before_packs>=packs AND before_packs<=1000000 AND current_location_id IS NOT NULL AND current_location_revision IS NOT NULL AND current_location_revision>=1 AND lineage_count BETWEEN 1 AND 256 AND source_evidence IS NOT NULL AND length(CAST(source_evidence AS BLOB)) BETWEEN 1 AND 512 AND quantity_evidence IS NOT NULL AND length(CAST(quantity_evidence AS BLOB)) BETWEEN 1 AND 512 AND recipient_evidence IS NOT NULL AND length(CAST(recipient_evidence AS BLOB)) BETWEEN 1 AND 512 AND timing_evidence IS NOT NULL AND length(CAST(timing_evidence AS BLOB)) BETWEEN 1 AND 512) OR (review_kind='link' AND historical_location_id IS NULL AND historical_location_revision IS NULL AND projection_hash IS NULL AND source_operation_id IS NULL AND source_group_id IS NULL AND known_operation_id IS NOT NULL AND before_group_revision IS NULL AND after_group_revision IS NULL AND before_packs IS NULL AND before_causal_floor_unix_s IS NULL AND after_causal_floor_unix_s IS NULL AND current_location_id IS NULL AND current_location_revision IS NULL AND lineage_count=0 AND scope_count=0 AND source_evidence IS NULL AND quantity_evidence IS NULL AND recipient_evidence IS NULL AND timing_evidence IS NULL)),
    CHECK ((review_kind='link' AND after_causal_floor_unix_s IS NULL) OR (review_kind='departure' AND (before_causal_floor_unix_s IS NULL OR (after_causal_floor_unix_s IS NOT NULL AND after_causal_floor_unix_s>=before_causal_floor_unix_s)) AND (occurrence_kind='unknown' OR (after_causal_floor_unix_s IS NOT NULL AND after_causal_floor_unix_s>=occurrence_start_unix_s)))),
    CHECK ((occurrence_kind='unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind='known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_end_unix_s=occurrence_start_unix_s) OR (occurrence_kind='interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_end_unix_s>=occurrence_start_unix_s))
) STRICT;
CREATE INDEX hub_actual_review_trace ON hub_actual_reviews(trace_id,review_kind);
CREATE TABLE hub_actual_review_lineage (
    operation_id TEXT NOT NULL REFERENCES hub_actual_reviews(operation_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    parent_group_id BLOB REFERENCES hub_stock_groups(group_id),
    created_revision INTEGER NOT NULL CHECK (created_revision>=2),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 0 AND 1000000),
    PRIMARY KEY (operation_id,group_id)
) STRICT;
CREATE TABLE hub_actual_review_work_scope (
    operation_id TEXT NOT NULL REFERENCES hub_actual_reviews(operation_id),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    before_progress TEXT NOT NULL CHECK (before_progress IN ('pending','completed','no_longer_required')),
    before_work_revision INTEGER NOT NULL CHECK (before_work_revision>=1),
    before_completed INTEGER NOT NULL CHECK (before_completed BETWEEN 0 AND 1000000),
    before_no_longer_required INTEGER NOT NULL CHECK (before_no_longer_required BETWEEN 0 AND 1000000),
    before_remaining INTEGER NOT NULL CHECK (before_remaining BETWEEN 0 AND 1000000),
    PRIMARY KEY (operation_id,work_id)
) STRICT;
CREATE TRIGGER hub_actual_report_guard BEFORE INSERT ON hub_actual_reports WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=NEW.operation_id AND o.kind='actual_report' AND o.result_id=NEW.report_id
) BEGIN SELECT RAISE(ABORT,'actual reports retain original claims without authorizing stock'); END;
CREATE TRIGGER hub_actual_source_guard BEFORE INSERT ON hub_actual_report_sources WHEN NOT EXISTS (
    SELECT 1 FROM hub_actual_reports r WHERE r.report_id=NEW.report_id AND NEW.ordinal<r.source_count
) BEGIN SELECT RAISE(ABORT,'every bounded actual source remains an original claim'); END;
CREATE TRIGGER hub_actual_review_guard BEFORE INSERT ON hub_actual_reviews WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_actual_reports r ON r.report_id=NEW.report_id
    JOIN hub_stock_operations original ON original.operation_id=r.operation_id
    JOIN hub_actual_report_reviews review ON review.report_id=r.report_id AND review.operation_id=r.operation_id
    JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_receipts population ON population.trace_id=g.trace_id
    WHERE o.operation_id=NEW.operation_id AND o.kind='actual_review' AND o.result_id=NEW.report_id
    AND o.hub_id=original.hub_id AND original.stock_revision<o.stock_revision AND review.review_operation_id IS NULL
    AND g.trace_id=NEW.trace_id AND population.variant_id=NEW.variant_id
    AND EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.record_id=NEW.variant_id AND v.record_revision=NEW.variant_revision AND v.hub_id=o.hub_id AND v.kind='pack' AND v.catalog_revision<=o.catalog_revision)
    AND EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.record_id=NEW.recipient_id AND v.record_revision=NEW.recipient_revision AND v.hub_id=o.hub_id AND v.kind='party' AND v.catalog_revision<=o.catalog_revision)
    AND (NEW.occurrence_kind='unknown' OR NEW.occurrence_end_unix_s<=o.recorded_at_unix_s)
    AND ((NEW.review_kind='departure' AND g.revision=NEW.before_group_revision
        AND (NEW.before_causal_floor_unix_s IS NULL OR NEW.before_causal_floor_unix_s<=o.recorded_at_unix_s)
        AND (NEW.after_causal_floor_unix_s IS NULL OR NEW.after_causal_floor_unix_s<=o.recorded_at_unix_s)
        AND NEW.after_causal_floor_unix_s IS CASE WHEN NEW.occurrence_kind='unknown' THEN NEW.before_causal_floor_unix_s WHEN NEW.before_causal_floor_unix_s IS NULL THEN NEW.occurrence_start_unix_s ELSE max(NEW.before_causal_floor_unix_s,NEW.occurrence_start_unix_s) END
        AND NEW.before_packs=COALESCE((SELECT sum(p.packs) FROM hub_stock_positions p WHERE p.group_id=g.group_id),0)
        AND EXISTS (SELECT 1 FROM hub_stock_positions p WHERE p.group_id=g.group_id AND p.location_id=NEW.current_location_id AND p.packs=NEW.before_packs)
        AND EXISTS (SELECT 1 FROM hub_reference_versions location WHERE location.record_id=NEW.current_location_id AND location.record_revision=NEW.current_location_revision AND location.hub_id=o.hub_id AND location.kind='location' AND location.catalog_revision<=o.catalog_revision AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=location.record_id AND newer.catalog_revision<=o.catalog_revision AND newer.record_revision>location.record_revision))
        AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.catalog_revision<=o.catalog_revision AND ((newer.record_id=NEW.variant_id AND newer.record_revision>NEW.variant_revision) OR (newer.record_id=NEW.recipient_id AND newer.record_revision>NEW.recipient_revision)))
        AND NEW.scope_count=(SELECT count(*) FROM hub_group_work w WHERE w.group_id=g.group_id)
        AND NEW.lineage_count=(SELECT count(*) FROM hub_stock_groups all_groups WHERE all_groups.trace_id=g.trace_id)
        AND EXISTS (SELECT 1 FROM hub_stock_operations witness JOIN hub_stock_groups source ON source.group_id=NEW.source_group_id WHERE witness.operation_id=NEW.source_operation_id AND witness.hub_id=o.hub_id AND witness.stock_revision<o.stock_revision AND witness.recorded_at_unix_s<=o.recorded_at_unix_s AND witness.kind IN ('receipt','opening','return','count_post','move','split') AND source.trace_id=g.trace_id AND EXISTS (SELECT 1 FROM hub_reference_versions location WHERE location.record_id=NEW.historical_location_id AND location.record_revision=NEW.historical_location_revision AND location.hub_id=o.hub_id AND location.kind='location' AND location.catalog_revision<=witness.catalog_revision AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=location.record_id AND newer.catalog_revision<=witness.catalog_revision AND newer.record_revision>location.record_revision))))
    OR (NEW.review_kind='link' AND EXISTS (SELECT 1 FROM hub_stock_operations known WHERE known.operation_id=NEW.known_operation_id AND known.hub_id=o.hub_id AND known.stock_revision<o.stock_revision AND ((known.kind='departure' AND EXISTS (SELECT 1 FROM hub_departure_lines line WHERE line.operation_id=known.operation_id AND line.group_id=g.group_id AND line.trace_id=g.trace_id)) OR (known.kind='actual_review' AND EXISTS (SELECT 1 FROM hub_actual_reviews prior WHERE prior.operation_id=known.operation_id AND prior.review_kind='departure' AND prior.trace_id=g.trace_id AND prior.group_id=g.group_id))))))
) BEGIN SELECT RAISE(ABORT,'actual review needs its original unresolved report and bounded explicit native evidence'); END;
CREATE TRIGGER hub_actual_lineage_guard BEFORE INSERT ON hub_actual_review_lineage WHEN NOT EXISTS (
    SELECT 1 FROM hub_actual_reviews review JOIN hub_stock_operations current ON current.operation_id=review.operation_id
    JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_group_identification i ON i.group_id=g.group_id
    JOIN hub_stock_operations birth ON birth.operation_id=i.created_operation_id
    WHERE review.operation_id=NEW.operation_id AND review.review_kind='departure' AND g.trace_id=review.trace_id
    AND i.parent_group_id IS NEW.parent_group_id AND birth.stock_revision=NEW.created_revision AND birth.stock_revision<current.stock_revision
    AND NEW.packs=COALESCE((SELECT sum(p.packs) FROM hub_stock_positions p WHERE p.group_id=g.group_id),0)
    AND (SELECT count(*) FROM hub_actual_review_lineage existing WHERE existing.operation_id=review.operation_id)<review.lineage_count
) BEGIN SELECT RAISE(ABORT,'actual review retains its complete native physical cut before effects'); END;
CREATE TRIGGER hub_actual_work_guard BEFORE INSERT ON hub_actual_review_work_scope WHEN NOT EXISTS (
    SELECT 1 FROM hub_actual_reviews review JOIN hub_group_work allocation ON allocation.group_id=review.group_id AND allocation.work_id=NEW.work_id
    JOIN hub_work_requirements w ON w.work_id=allocation.work_id JOIN hub_receipts population ON population.trace_id=review.trace_id
    WHERE review.operation_id=NEW.operation_id AND review.review_kind='departure' AND allocation.scoped_packs=review.before_packs
    AND allocation.progress=NEW.before_progress AND w.revision=NEW.before_work_revision
    AND w.trace_id=review.trace_id AND w.variant_id=population.variant_id
    AND w.completed=NEW.before_completed AND w.no_longer_required=NEW.before_no_longer_required AND w.remaining=NEW.before_remaining
    AND w.required=NEW.before_completed+NEW.before_no_longer_required+NEW.before_remaining
    AND (SELECT count(*) FROM hub_actual_review_work_scope existing WHERE existing.operation_id=review.operation_id)<review.scope_count
) BEGIN SELECT RAISE(ABORT,'actual review retains exact original work before balanced retirement'); END;
CREATE TRIGGER hub_actual_reports_no_update BEFORE UPDATE ON hub_actual_reports BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_reports_no_delete BEFORE DELETE ON hub_actual_reports BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_report_sources_no_update BEFORE UPDATE ON hub_actual_report_sources BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_report_sources_no_delete BEFORE DELETE ON hub_actual_report_sources BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_reviews_no_update BEFORE UPDATE ON hub_actual_reviews BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_reviews_no_delete BEFORE DELETE ON hub_actual_reviews BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_review_lineage_no_update BEFORE UPDATE ON hub_actual_review_lineage BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_review_lineage_no_delete BEFORE DELETE ON hub_actual_review_lineage BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_review_work_scope_no_update BEFORE UPDATE ON hub_actual_review_work_scope BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
CREATE TRIGGER hub_actual_review_work_scope_no_delete BEFORE DELETE ON hub_actual_review_work_scope BEGIN SELECT RAISE(ABORT,'actual report and review originals are retained immutable evidence'); END;
DROP TRIGGER hub_actual_report_review_insert_guard;
CREATE TRIGGER hub_actual_report_review_insert_guard BEFORE INSERT ON hub_actual_report_reviews
WHEN NEW.review_operation_id IS NOT NULL OR NOT EXISTS (SELECT 1 FROM hub_submissions s JOIN hub_actual_reports r ON r.operation_id=s.operation_id WHERE s.operation_id=NEW.operation_id AND s.kind='actual_report' AND r.report_id=NEW.report_id)
BEGIN SELECT RAISE(ABORT,'unresolved actual markers bind their exact admitted original report'); END;
DROP TRIGGER hub_actual_report_review_update_guard;
CREATE TRIGGER hub_actual_report_review_update_guard BEFORE UPDATE ON hub_actual_report_reviews
WHEN NEW.report_id<>OLD.report_id OR NEW.operation_id<>OLD.operation_id OR OLD.review_operation_id IS NOT NULL OR NEW.review_operation_id IS NULL OR NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_actual_reviews review ON review.operation_id=o.operation_id WHERE o.operation_id=NEW.review_operation_id AND o.kind='actual_review' AND review.report_id=OLD.report_id)
BEGIN SELECT RAISE(ABORT,'actual review closure binds the exact immutable owner review'); END;
