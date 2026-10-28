CREATE TABLE hub_group_identification (
    group_id BLOB NOT NULL PRIMARY KEY REFERENCES hub_stock_groups(group_id),
    parent_group_id BLOB REFERENCES hub_group_identification(group_id),
    physical_label TEXT CHECK (physical_label IS NULL OR length(CAST(physical_label AS BLOB)) BETWEEN 1 AND 120),
    observation TEXT CHECK (observation IS NULL OR length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    created_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    CHECK (parent_group_id IS NULL OR parent_group_id <> group_id),
    CHECK ((physical_label IS NULL AND observation IS NULL AND parent_group_id IS NULL) OR (physical_label IS NOT NULL AND observation IS NOT NULL))
) STRICT;
CREATE TABLE hub_work_requirements (
    work_id BLOB NOT NULL PRIMARY KEY CHECK (length(work_id) = 16 AND work_id <> zeroblob(16)),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    variant_id BLOB NOT NULL CHECK (length(variant_id) = 16),
    target_location_id BLOB,
    target_location_revision INTEGER,
    required INTEGER NOT NULL CHECK (required BETWEEN 1 AND 1000000),
    completed INTEGER NOT NULL CHECK (completed BETWEEN 0 AND 1000000),
    no_longer_required INTEGER NOT NULL CHECK (no_longer_required BETWEEN 0 AND 1000000),
    remaining INTEGER NOT NULL CHECK (remaining BETWEEN 0 AND 1000000),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    FOREIGN KEY (target_location_id,target_location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK (required = completed + no_longer_required + remaining),
    CHECK ((target_location_id IS NULL AND target_location_revision IS NULL AND completed = 0) OR (target_location_id IS NOT NULL AND target_location_revision IS NOT NULL AND target_location_revision >= 1))
) STRICT;
CREATE TABLE hub_group_work (
    group_id BLOB NOT NULL REFERENCES hub_group_identification(group_id),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    progress TEXT NOT NULL CHECK (progress IN ('pending','completed','no_longer_required')),
    scoped_packs INTEGER NOT NULL CHECK (scoped_packs BETWEEN 0 AND 1000000),
    PRIMARY KEY (group_id,work_id)
) STRICT;
CREATE TABLE hub_movement_details (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    selected_group_id BLOB NOT NULL REFERENCES hub_group_identification(group_id),
    expected_group_revision INTEGER NOT NULL CHECK (expected_group_revision >= 1),
    source_before_packs INTEGER NOT NULL CHECK (source_before_packs BETWEEN 1 AND 1000000),
    moved_group_id BLOB NOT NULL REFERENCES hub_group_identification(group_id),
    work_change_count INTEGER NOT NULL CHECK (work_change_count BETWEEN 0 AND 64),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512)
) STRICT;
CREATE TABLE hub_work_changes (
    operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    before_completed INTEGER CHECK (before_completed IS NULL OR before_completed BETWEEN 0 AND 1000000),
    before_no_longer_required INTEGER CHECK (before_no_longer_required IS NULL OR before_no_longer_required BETWEEN 0 AND 1000000),
    before_remaining INTEGER CHECK (before_remaining IS NULL OR before_remaining BETWEEN 0 AND 1000000),
    before_revision INTEGER CHECK (before_revision IS NULL OR before_revision >= 1),
    after_completed INTEGER NOT NULL CHECK (after_completed BETWEEN 0 AND 1000000),
    after_no_longer_required INTEGER NOT NULL CHECK (after_no_longer_required BETWEEN 0 AND 1000000),
    after_remaining INTEGER NOT NULL CHECK (after_remaining BETWEEN 0 AND 1000000),
    after_revision INTEGER NOT NULL CHECK (after_revision >= 1),
    PRIMARY KEY (operation_id,work_id),
    CHECK ((before_revision IS NULL AND before_completed IS NULL AND before_no_longer_required IS NULL AND before_remaining IS NULL) OR (before_revision IS NOT NULL AND before_completed IS NOT NULL AND before_no_longer_required IS NOT NULL AND before_remaining IS NOT NULL))
) STRICT;
CREATE TABLE hub_work_actions (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    previous_work_id BLOB REFERENCES hub_work_requirements(work_id),
    expected_work_revision INTEGER,
    replacement_work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    CHECK ((previous_work_id IS NULL AND expected_work_revision IS NULL) OR (previous_work_id IS NOT NULL AND expected_work_revision IS NOT NULL AND expected_work_revision >= 1 AND previous_work_id <> replacement_work_id))
) STRICT;
CREATE TABLE hub_action_groups (
    operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    group_id BLOB NOT NULL REFERENCES hub_group_identification(group_id),
    before_revision INTEGER NOT NULL CHECK (before_revision >= 1),
    scoped_packs INTEGER NOT NULL CHECK (scoped_packs BETWEEN 1 AND 1000000),
    location_id BLOB NOT NULL,
    location_revision INTEGER NOT NULL CHECK (location_revision >= 1),
    PRIMARY KEY (operation_id,group_id),
    FOREIGN KEY (location_id,location_revision) REFERENCES hub_reference_versions(record_id,record_revision)
) STRICT;
CREATE TABLE hub_discrepancies (
    discrepancy_id BLOB NOT NULL PRIMARY KEY CHECK (length(discrepancy_id) = 16 AND discrepancy_id <> zeroblob(16)),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    group_id BLOB REFERENCES hub_group_identification(group_id),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    revision INTEGER NOT NULL CHECK (revision IN (1,2)),
    resolution TEXT CHECK (resolution IS NULL OR length(CAST(resolution AS BLOB)) BETWEEN 1 AND 512),
    created_operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    resolved_operation_id TEXT UNIQUE REFERENCES hub_stock_operations(operation_id),
    CHECK ((revision = 1 AND resolution IS NULL AND resolved_operation_id IS NULL) OR (revision = 2 AND resolution IS NOT NULL AND resolved_operation_id IS NOT NULL))
) STRICT;
CREATE INDEX hub_group_physical_label ON hub_group_identification(physical_label) WHERE physical_label IS NOT NULL;
CREATE UNIQUE INDEX hub_group_single_active_position ON hub_stock_positions(group_id) WHERE packs > 0;
CREATE INDEX hub_work_trace ON hub_work_requirements(trace_id,work_id);
CREATE INDEX hub_group_work_requirement ON hub_group_work(work_id,group_id);
CREATE INDEX hub_stock_event_operation ON hub_stock_events(operation_id);
CREATE INDEX hub_stock_event_group_sequence ON hub_stock_events(group_id,sequence);
CREATE TRIGGER hub_group_identification_no_update BEFORE UPDATE ON hub_group_identification BEGIN SELECT RAISE(ABORT, 'physical identification is immutable'); END;
CREATE TRIGGER hub_group_identification_no_delete BEFORE DELETE ON hub_group_identification BEGIN SELECT RAISE(ABORT, 'physical identification is immutable'); END;
CREATE TRIGGER hub_work_requirement_identity_immutable BEFORE UPDATE ON hub_work_requirements WHEN NEW.work_id IS NOT OLD.work_id OR NEW.trace_id IS NOT OLD.trace_id OR NEW.variant_id IS NOT OLD.variant_id OR NEW.target_location_id IS NOT OLD.target_location_id OR NEW.target_location_revision IS NOT OLD.target_location_revision OR NEW.required IS NOT OLD.required OR NEW.created_operation_id IS NOT OLD.created_operation_id BEGIN SELECT RAISE(ABORT, 'work requirement identity is immutable'); END;
CREATE TRIGGER hub_work_requirements_no_delete BEFORE DELETE ON hub_work_requirements BEGIN SELECT RAISE(ABORT, 'work history is retained'); END;
CREATE TRIGGER hub_movement_details_no_update BEFORE UPDATE ON hub_movement_details BEGIN SELECT RAISE(ABORT, 'movement result is immutable'); END;
CREATE TRIGGER hub_movement_details_no_delete BEFORE DELETE ON hub_movement_details BEGIN SELECT RAISE(ABORT, 'movement result is immutable'); END;
CREATE TRIGGER hub_work_changes_no_update BEFORE UPDATE ON hub_work_changes BEGIN SELECT RAISE(ABORT, 'work result is immutable'); END;
CREATE TRIGGER hub_work_changes_no_delete BEFORE DELETE ON hub_work_changes BEGIN SELECT RAISE(ABORT, 'work result is immutable'); END;
CREATE TRIGGER hub_work_actions_no_update BEFORE UPDATE ON hub_work_actions BEGIN SELECT RAISE(ABORT, 'work action is immutable'); END;
CREATE TRIGGER hub_work_actions_no_delete BEFORE DELETE ON hub_work_actions BEGIN SELECT RAISE(ABORT, 'work action is immutable'); END;
CREATE TRIGGER hub_action_groups_no_update BEFORE UPDATE ON hub_action_groups BEGIN SELECT RAISE(ABORT, 'original scope is immutable'); END;
CREATE TRIGGER hub_action_groups_no_delete BEFORE DELETE ON hub_action_groups BEGIN SELECT RAISE(ABORT, 'original scope is immutable'); END;
CREATE TRIGGER hub_discrepancy_identity_immutable BEFORE UPDATE ON hub_discrepancies WHEN NEW.discrepancy_id IS NOT OLD.discrepancy_id OR NEW.trace_id IS NOT OLD.trace_id OR NEW.group_id IS NOT OLD.group_id OR NEW.observation IS NOT OLD.observation OR NEW.created_operation_id IS NOT OLD.created_operation_id OR OLD.revision <> 1 OR NEW.revision <> 2 BEGIN SELECT RAISE(ABORT, 'discrepancy history is immutable'); END;
CREATE TRIGGER hub_discrepancies_no_delete BEFORE DELETE ON hub_discrepancies BEGIN SELECT RAISE(ABORT, 'discrepancy history is retained'); END;
CREATE TRIGGER hub_group_identity_immutable BEFORE UPDATE ON hub_stock_groups WHEN NEW.group_id IS NOT OLD.group_id OR NEW.trace_id IS NOT OLD.trace_id OR NEW.revision <> OLD.revision + 1 BEGIN SELECT RAISE(ABORT, 'group origin is immutable'); END;
CREATE TRIGGER hub_work_progress_monotonic BEFORE UPDATE ON hub_work_requirements WHEN NEW.revision <> OLD.revision + 1 OR NEW.completed < OLD.completed OR NEW.no_longer_required < OLD.no_longer_required OR NEW.remaining > OLD.remaining OR (NEW.completed = OLD.completed AND NEW.no_longer_required = OLD.no_longer_required) BEGIN SELECT RAISE(ABORT, 'work progress cannot be reset'); END;
CREATE TRIGGER hub_group_work_identity_immutable BEFORE UPDATE ON hub_group_work WHEN NEW.group_id IS NOT OLD.group_id OR NEW.work_id IS NOT OLD.work_id OR NEW.scoped_packs > OLD.scoped_packs OR (OLD.progress <> 'pending' AND NEW.progress <> OLD.progress) BEGIN SELECT RAISE(ABORT, 'group work attribution cannot be reset'); END;
CREATE TRIGGER hub_group_work_no_delete BEFORE DELETE ON hub_group_work BEGIN SELECT RAISE(ABORT, 'group work history is retained'); END;
INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) SELECT g.group_id,NULL,NULL,NULL,r.operation_id FROM hub_stock_groups g JOIN hub_receipts r ON r.root_group_id=g.group_id AND r.trace_id=g.trace_id;
INSERT INTO hub_work_requirements (work_id,trace_id,variant_id,target_location_id,target_location_revision,required,completed,no_longer_required,remaining,revision,created_operation_id) SELECT w.trace_id,w.trace_id,r.variant_id,w.target_location_id,w.target_location_revision,w.required,w.completed,w.no_longer_required,w.remaining,w.revision,r.operation_id FROM hub_putaway_work w JOIN hub_receipts r ON r.trace_id=w.trace_id;
INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) SELECT g.group_id,r.trace_id,'pending',CASE WHEN g.group_id=r.root_group_id AND g.revision=1 AND p.revision=1 AND w.revision=1 AND p.packs=r.original_packs AND w.required=r.original_packs AND w.remaining=w.required AND w.completed=0 AND w.no_longer_required=0 AND w.target_location_id IS r.planned_location_id AND w.target_location_revision IS r.planned_location_revision THEN p.packs ELSE -1 END FROM hub_receipts r LEFT JOIN hub_stock_groups g ON g.trace_id=r.trace_id LEFT JOIN hub_stock_positions p ON p.group_id=g.group_id LEFT JOIN hub_putaway_work w ON w.trace_id=r.trace_id;
DROP TABLE hub_putaway_work;
