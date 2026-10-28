CREATE TABLE hub_treatment_records (
    record_id BLOB NOT NULL PRIMARY KEY CHECK (length(record_id)=16 AND record_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    kind TEXT NOT NULL CHECK (kind IN ('acceptance','hold_place','hold_release')),
    acceptance TEXT CHECK (acceptance IN ('review','accepted','rejected')),
    hold_id BLOB REFERENCES hub_treatment_records(record_id),
    expected_hold_revision INTEGER,
    basis_id TEXT NOT NULL CHECK (length(CAST(basis_id AS BLOB)) BETWEEN 1 AND 64),
    basis_version INTEGER NOT NULL CHECK (basis_version BETWEEN 1 AND 4294967295),
    site_timezone TEXT NOT NULL CHECK (length(CAST(site_timezone AS BLOB)) BETWEEN 1 AND 64),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    scope_count INTEGER NOT NULL CHECK (scope_count BETWEEN 0 AND 64),
    affected_packs INTEGER NOT NULL CHECK (affected_packs BETWEEN 0 AND 1000000),
    on_hand INTEGER NOT NULL CHECK (on_hand BETWEEN 0 AND 1000000),
    accepted INTEGER NOT NULL CHECK (accepted BETWEEN 0 AND 1000000),
    review_required INTEGER NOT NULL CHECK (review_required BETWEEN 0 AND 1000000),
    rejected INTEGER NOT NULL CHECK (rejected BETWEEN 0 AND 1000000),
    restricted INTEGER NOT NULL CHECK (restricted BETWEEN 0 AND 1000000),
    departure_eligible INTEGER NOT NULL CHECK (departure_eligible BETWEEN 0 AND 1000000),
    history_capacity_reached INTEGER NOT NULL CHECK (history_capacity_reached IN (0,1)),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    CHECK (on_hand=accepted+review_required+rejected AND restricted<=on_hand AND departure_eligible<=accepted AND departure_eligible<=on_hand-restricted AND departure_eligible>=max(accepted-restricted,0)),
    CHECK ((kind='acceptance' AND acceptance IS NOT NULL AND hold_id IS NULL AND expected_hold_revision IS NULL AND scope_count>=1) OR (kind='hold_place' AND acceptance IS NULL AND hold_id IS NULL AND expected_hold_revision IS NULL AND scope_count>=1) OR (kind='hold_release' AND acceptance IS NULL AND hold_id IS NOT NULL AND hold_id<>record_id AND expected_hold_revision IS NOT NULL AND expected_hold_revision=1 AND scope_count=0))
) STRICT;
CREATE TABLE hub_treatment_scope_anchors (
    record_id BLOB NOT NULL REFERENCES hub_treatment_records(record_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    created_stock_revision INTEGER NOT NULL CHECK (created_stock_revision>=2),
    captured_packs INTEGER NOT NULL CHECK (captured_packs BETWEEN 1 AND 1000000),
    PRIMARY KEY (record_id,group_id)
) STRICT;
CREATE INDEX hub_treatment_trace ON hub_treatment_records(trace_id,record_id);
CREATE UNIQUE INDEX hub_treatment_one_release ON hub_treatment_records(hold_id) WHERE kind='hold_release';
CREATE TRIGGER hub_treatment_record_guard BEFORE INSERT ON hub_treatment_records WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_receipts r ON r.trace_id=NEW.trace_id JOIN hub_stock_operations original ON original.operation_id=r.operation_id WHERE o.operation_id=NEW.operation_id AND o.kind=NEW.kind AND o.result_id=NEW.record_id AND o.hub_id=original.hub_id AND (NEW.kind<>'hold_release' OR EXISTS (SELECT 1 FROM hub_treatment_records held JOIN hub_stock_operations placed ON placed.operation_id=held.operation_id WHERE held.record_id=NEW.hold_id AND held.kind='hold_place' AND held.trace_id=NEW.trace_id AND placed.hub_id=o.hub_id AND placed.stock_revision<o.stock_revision))) BEGIN SELECT RAISE(ABORT,'treatment must retain its original population and operation'); END;
CREATE TRIGGER hub_treatment_scope_guard BEFORE INSERT ON hub_treatment_scope_anchors WHEN NOT EXISTS (SELECT 1 FROM hub_treatment_records t JOIN hub_stock_operations o ON o.operation_id=t.operation_id JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_group_identification i ON i.group_id=g.group_id JOIN hub_stock_operations born ON born.operation_id=i.created_operation_id WHERE t.record_id=NEW.record_id AND t.kind IN ('acceptance','hold_place') AND g.trace_id=t.trace_id AND born.hub_id=o.hub_id AND born.stock_revision=NEW.created_stock_revision AND born.stock_revision<o.stock_revision AND NEW.captured_packs=(SELECT COALESCE(sum(p.packs),0) FROM hub_stock_positions p WHERE p.group_id=g.group_id) AND (SELECT count(*) FROM hub_treatment_scope_anchors a WHERE a.record_id=t.record_id)<t.scope_count) BEGIN SELECT RAISE(ABORT,'treatment scope must retain original group birth'); END;
CREATE TRIGGER hub_treatment_records_no_update BEFORE UPDATE ON hub_treatment_records BEGIN SELECT RAISE(ABORT,'treatment original is immutable'); END;
CREATE TRIGGER hub_treatment_records_no_delete BEFORE DELETE ON hub_treatment_records BEGIN SELECT RAISE(ABORT,'treatment original is retained'); END;
CREATE TRIGGER hub_treatment_scope_no_update BEFORE UPDATE ON hub_treatment_scope_anchors BEGIN SELECT RAISE(ABORT,'treatment scope is immutable'); END;
CREATE TRIGGER hub_treatment_scope_no_delete BEFORE DELETE ON hub_treatment_scope_anchors BEGIN SELECT RAISE(ABORT,'treatment scope is retained'); END;
