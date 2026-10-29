CREATE TABLE hub_departures (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    departure_id BLOB NOT NULL UNIQUE CHECK (length(departure_id)=16 AND departure_id<>zeroblob(16)),
    preparation_id BLOB NOT NULL UNIQUE CHECK (length(preparation_id)=16 AND preparation_id<>zeroblob(16)),
    preparation_revision INTEGER NOT NULL CHECK (preparation_revision>=2),
    identification_id BLOB NOT NULL UNIQUE REFERENCES hub_dispatch_identifications(identification_id),
    line_count INTEGER NOT NULL CHECK (line_count BETWEEN 1 AND 64),
    work_change_count INTEGER NOT NULL CHECK (work_change_count BETWEEN 0 AND 4096),
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('unknown','known','interval')),
    occurrence_start_unix_s INTEGER,
    occurrence_end_unix_s INTEGER,
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    CHECK ((occurrence_kind='unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL) OR (occurrence_kind='known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_start_unix_s=occurrence_end_unix_s) OR (occurrence_kind='interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_start_unix_s<=occurrence_end_unix_s))
) STRICT;
CREATE TABLE hub_departure_lines (
    operation_id TEXT NOT NULL REFERENCES hub_departures(operation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    group_id BLOB NOT NULL UNIQUE REFERENCES hub_group_identification(group_id),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    before_group_revision INTEGER NOT NULL CHECK (before_group_revision>=1),
    after_group_revision INTEGER NOT NULL CHECK (after_group_revision=before_group_revision+1),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    location_id BLOB NOT NULL,
    location_revision INTEGER NOT NULL CHECK (location_revision>=1),
    before_causal_floor_unix_s INTEGER CHECK (before_causal_floor_unix_s IS NULL OR before_causal_floor_unix_s>=0),
    after_causal_floor_unix_s INTEGER CHECK (after_causal_floor_unix_s IS NULL OR after_causal_floor_unix_s>=0),
    scope_count INTEGER NOT NULL CHECK (scope_count BETWEEN 0 AND 64),
    PRIMARY KEY (operation_id,ordinal),
    FOREIGN KEY (location_id,location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK (before_causal_floor_unix_s IS NULL OR (after_causal_floor_unix_s IS NOT NULL AND after_causal_floor_unix_s>=before_causal_floor_unix_s))
) STRICT;
CREATE TABLE hub_departure_work_scope (
    operation_id TEXT NOT NULL,
    line_ordinal INTEGER NOT NULL CHECK (line_ordinal BETWEEN 0 AND 63),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    before_progress TEXT NOT NULL CHECK (before_progress IN ('pending','completed','no_longer_required')),
    before_work_revision INTEGER NOT NULL CHECK (before_work_revision>=1),
    before_completed INTEGER NOT NULL CHECK (before_completed BETWEEN 0 AND 1000000),
    before_no_longer_required INTEGER NOT NULL CHECK (before_no_longer_required BETWEEN 0 AND 1000000),
    before_remaining INTEGER NOT NULL CHECK (before_remaining BETWEEN 0 AND 1000000),
    PRIMARY KEY (operation_id,line_ordinal,work_id),
    FOREIGN KEY (operation_id,line_ordinal) REFERENCES hub_departure_lines(operation_id,ordinal)
) STRICT;
CREATE INDEX hub_departure_trace_lines ON hub_departure_lines(trace_id,operation_id);
CREATE TRIGGER hub_departure_guard BEFORE INSERT ON hub_departures WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_dispatch_changes c ON c.preparation_id=NEW.preparation_id AND c.record_revision=NEW.preparation_revision JOIN hub_dispatch_identifications i ON i.identification_id=NEW.identification_id JOIN hub_stock_operations issued ON issued.operation_id=i.issued_operation_id JOIN hub_dispatch_materials m ON m.operation_id=i.material_operation_id WHERE o.operation_id=NEW.operation_id AND o.kind='departure' AND o.result_id=NEW.departure_id AND c.kind IN ('dispatch_issue','dispatch_replace') AND c.identification_id=NEW.identification_id AND i.preparation_id=NEW.preparation_id AND i.material_operation_id=c.material_operation_id AND m.preparation_id=NEW.preparation_id AND m.line_count=NEW.line_count AND issued.hub_id=o.hub_id AND o.stock_revision>issued.stock_revision AND o.catalog_revision>=m.catalog_revision AND (NEW.occurrence_kind='unknown' OR NEW.occurrence_end_unix_s<=o.recorded_at_unix_s) AND NOT EXISTS (SELECT 1 FROM hub_dispatch_changes later WHERE later.preparation_id=c.preparation_id AND later.record_revision>c.record_revision)) BEGIN SELECT RAISE(ABORT,'departure requires its exact active issued preparation'); END;
CREATE TRIGGER hub_departure_line_guard BEFORE INSERT ON hub_departure_lines WHEN NOT EXISTS (SELECT 1 FROM hub_departures d JOIN hub_dispatch_identifications i ON i.identification_id=d.identification_id JOIN hub_dispatch_material_lines m ON m.operation_id=i.material_operation_id AND m.ordinal=NEW.ordinal JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_stock_positions p ON p.group_id=g.group_id AND p.location_id=NEW.location_id WHERE d.operation_id=NEW.operation_id AND NEW.ordinal<d.line_count AND m.group_id=NEW.group_id AND m.trace_id=NEW.trace_id AND m.group_revision=NEW.before_group_revision AND m.packs=NEW.packs AND m.location_id=NEW.location_id AND m.location_revision=NEW.location_revision AND g.trace_id=NEW.trace_id AND g.revision=NEW.before_group_revision AND p.packs=NEW.packs AND (SELECT count(*) FROM hub_stock_positions all_positions WHERE all_positions.group_id=g.group_id AND all_positions.packs>0)=1 AND NEW.scope_count=(SELECT count(*) FROM hub_group_work a WHERE a.group_id=g.group_id)) BEGIN SELECT RAISE(ABORT,'departure must retain its complete current whole group'); END;
CREATE TRIGGER hub_departure_scope_guard BEFORE INSERT ON hub_departure_work_scope WHEN NOT EXISTS (SELECT 1 FROM hub_departure_lines line JOIN hub_group_work a ON a.group_id=line.group_id AND a.work_id=NEW.work_id JOIN hub_work_requirements w ON w.work_id=a.work_id JOIN hub_receipts r ON r.trace_id=line.trace_id WHERE line.operation_id=NEW.operation_id AND line.ordinal=NEW.line_ordinal AND a.scoped_packs=line.packs AND a.progress=NEW.before_progress AND w.trace_id=line.trace_id AND w.variant_id=r.variant_id AND w.revision=NEW.before_work_revision AND w.completed=NEW.before_completed AND w.no_longer_required=NEW.before_no_longer_required AND w.remaining=NEW.before_remaining AND w.required=NEW.before_completed+NEW.before_no_longer_required+NEW.before_remaining) BEGIN SELECT RAISE(ABORT,'departure work must retain its original complete allocation'); END;
CREATE TRIGGER hub_departure_no_update BEFORE UPDATE ON hub_departures BEGIN SELECT RAISE(ABORT,'actual departure is immutable'); END;
CREATE TRIGGER hub_departure_no_delete BEFORE DELETE ON hub_departures BEGIN SELECT RAISE(ABORT,'actual departure is retained'); END;
CREATE TRIGGER hub_departure_line_no_update BEFORE UPDATE ON hub_departure_lines BEGIN SELECT RAISE(ABORT,'departure line is immutable'); END;
CREATE TRIGGER hub_departure_line_no_delete BEFORE DELETE ON hub_departure_lines BEGIN SELECT RAISE(ABORT,'departure line is retained'); END;
CREATE TRIGGER hub_departure_scope_no_update BEFORE UPDATE ON hub_departure_work_scope BEGIN SELECT RAISE(ABORT,'departure work original is immutable'); END;
CREATE TRIGGER hub_departure_scope_no_delete BEFORE DELETE ON hub_departure_work_scope BEGIN SELECT RAISE(ABORT,'departure work original is retained'); END;
CREATE TRIGGER hub_dispatch_used_guard BEFORE INSERT ON hub_dispatch_changes WHEN EXISTS (SELECT 1 FROM hub_departures used WHERE used.preparation_id=NEW.preparation_id) BEGIN SELECT RAISE(ABORT,'used identification cannot be revised or marked unused'); END;
