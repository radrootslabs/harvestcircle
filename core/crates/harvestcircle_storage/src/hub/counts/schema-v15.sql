CREATE UNIQUE INDEX hub_count_observation_population ON hub_count_observations(count_id,trace_id);
CREATE TABLE hub_count_reconciliations (
    reconciliation_id BLOB NOT NULL PRIMARY KEY CHECK (length(reconciliation_id)=16 AND reconciliation_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    count_id BLOB NOT NULL UNIQUE REFERENCES hub_count_observations(count_id),
    policy_version INTEGER NOT NULL CHECK (policy_version=1),
    effect_kind TEXT NOT NULL CHECK (effect_kind IN ('none','shortage','surplus')),
    changed_packs INTEGER NOT NULL CHECK (changed_packs BETWEEN 0 AND 1000000),
    after_on_hand_packs INTEGER NOT NULL CHECK (after_on_hand_packs BETWEEN 0 AND 1000000),
    after_group_revision INTEGER NOT NULL CHECK (after_group_revision>=1),
    before_causal_floor_unix_s INTEGER CHECK (before_causal_floor_unix_s IS NULL OR before_causal_floor_unix_s>=0),
    confirmed_on_hand_at_unix_s INTEGER NOT NULL CHECK (confirmed_on_hand_at_unix_s>=0),
    scope_work_count INTEGER NOT NULL CHECK (scope_work_count BETWEEN 0 AND 64),
    changed_work_count INTEGER NOT NULL CHECK (changed_work_count BETWEEN 0 AND scope_work_count),
    surplus_population_id BLOB REFERENCES hub_receipts(trace_id) DEFERRABLE INITIALLY DEFERRED,
    reason TEXT NOT NULL CHECK (length(CAST(reason AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    UNIQUE (reconciliation_id,surplus_population_id,count_id),
    CHECK ((effect_kind='none' AND changed_packs=0 AND surplus_population_id IS NULL AND changed_work_count=0) OR (effect_kind='shortage' AND changed_packs>0 AND surplus_population_id IS NULL) OR (effect_kind='surplus' AND changed_packs>0 AND surplus_population_id IS NOT NULL AND length(surplus_population_id)=16 AND surplus_population_id<>zeroblob(16) AND changed_work_count=0)),
    CHECK (before_causal_floor_unix_s IS NULL OR before_causal_floor_unix_s<=confirmed_on_hand_at_unix_s)
) STRICT;
CREATE TABLE hub_count_reconciliation_work (
    reconciliation_id BLOB NOT NULL REFERENCES hub_count_reconciliations(reconciliation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    before_progress TEXT NOT NULL CHECK (before_progress IN ('pending','completed','no_longer_required')),
    before_scoped_packs INTEGER NOT NULL CHECK (before_scoped_packs BETWEEN 1 AND 1000000),
    on_hand_after INTEGER NOT NULL CHECK (on_hand_after BETWEEN 0 AND 1000000),
    removed_packs INTEGER NOT NULL CHECK (removed_packs BETWEEN 0 AND 1000000),
    removed_progress TEXT CHECK (removed_progress IS NULL OR removed_progress IN ('completed','no_longer_required')),
    required INTEGER NOT NULL CHECK (required BETWEEN 1 AND 1000000),
    before_completed INTEGER NOT NULL CHECK (before_completed BETWEEN 0 AND 1000000),
    before_no_longer_required INTEGER NOT NULL CHECK (before_no_longer_required BETWEEN 0 AND 1000000),
    before_remaining INTEGER NOT NULL CHECK (before_remaining BETWEEN 0 AND 1000000),
    before_revision INTEGER NOT NULL CHECK (before_revision>=1),
    after_completed INTEGER NOT NULL CHECK (after_completed BETWEEN 0 AND 1000000),
    after_no_longer_required INTEGER NOT NULL CHECK (after_no_longer_required BETWEEN 0 AND 1000000),
    after_remaining INTEGER NOT NULL CHECK (after_remaining BETWEEN 0 AND 1000000),
    after_revision INTEGER NOT NULL CHECK (after_revision>=1),
    PRIMARY KEY (reconciliation_id,work_id),
    UNIQUE (reconciliation_id,ordinal),
    CHECK (before_scoped_packs=on_hand_after+removed_packs),
    CHECK (required=before_completed+before_no_longer_required+before_remaining AND required=after_completed+after_no_longer_required+after_remaining),
    CHECK (after_completed=before_completed),
    CHECK ((removed_packs=0 AND removed_progress IS NULL) OR (removed_packs>0 AND removed_progress IS NOT NULL AND ((before_progress='pending' AND removed_progress='no_longer_required') OR (before_progress IN ('completed','no_longer_required') AND removed_progress=before_progress)))),
    CHECK (after_no_longer_required=before_no_longer_required+CASE WHEN before_progress='pending' THEN removed_packs ELSE 0 END),
    CHECK (after_remaining=before_remaining-CASE WHEN before_progress='pending' THEN removed_packs ELSE 0 END),
    CHECK (after_revision=before_revision+CASE WHEN before_progress='pending' AND removed_packs>0 THEN 1 ELSE 0 END)
) STRICT;
CREATE INDEX hub_count_reconciliation_work_requirement ON hub_count_reconciliation_work(work_id,reconciliation_id);
CREATE TABLE hub_counted_surplus (
    population_id BLOB NOT NULL PRIMARY KEY CHECK (length(population_id)=16 AND population_id<>zeroblob(16)),
    origin_kind TEXT NOT NULL DEFAULT 'surplus' CHECK (origin_kind='surplus'),
    reconciliation_id BLOB NOT NULL UNIQUE,
    count_id BLOB NOT NULL UNIQUE,
    related_population_id BLOB NOT NULL CHECK (related_population_id<>population_id),
    FOREIGN KEY (population_id,origin_kind) REFERENCES hub_receipts(trace_id,origin_kind),
    FOREIGN KEY (reconciliation_id,population_id,count_id) REFERENCES hub_count_reconciliations(reconciliation_id,surplus_population_id,count_id),
    FOREIGN KEY (count_id,related_population_id) REFERENCES hub_count_observations(count_id,trace_id)
) STRICT;
CREATE TRIGGER hub_count_reconciliation_guard BEFORE INSERT ON hub_count_reconciliations
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o
    JOIN hub_count_observations c ON c.count_id=NEW.count_id
    JOIN hub_stock_operations observed ON observed.operation_id=c.operation_id
    JOIN hub_stock_groups g ON g.group_id=c.group_id AND g.trace_id=c.trace_id
    WHERE o.operation_id=NEW.operation_id AND o.kind='count_post' AND o.result_id=NEW.reconciliation_id
    AND observed.kind='count_observe' AND observed.result_id=c.count_id AND o.hub_id=observed.hub_id
    AND o.stock_revision>observed.stock_revision AND o.catalog_revision>=observed.catalog_revision
    AND o.stock_revision=(SELECT state.revision+1 FROM hub_stock_state state WHERE state.hub_id=o.hub_id)
    AND o.recorded_at_unix_s>=observed.recorded_at_unix_s AND o.recorded_at_unix_s>=NEW.confirmed_on_hand_at_unix_s
    AND c.occurrence_kind='known' AND c.occurrence_start_unix_s=NEW.confirmed_on_hand_at_unix_s
    AND c.latest_physical_recorded_at_unix_s<=NEW.confirmed_on_hand_at_unix_s
    AND g.revision=c.group_revision
    AND (SELECT COALESCE(sum(p.packs),0) FROM hub_stock_positions p WHERE p.group_id=g.group_id)=c.recorded_packs
    AND (SELECT count(*) FROM hub_stock_positions p WHERE p.group_id=g.group_id AND p.packs>0)=1
    AND EXISTS (SELECT 1 FROM hub_stock_positions p WHERE p.group_id=g.group_id AND p.packs>0 AND p.location_id=c.location_id)
    AND NOT EXISTS (SELECT 1 FROM hub_count_observations newer JOIN hub_stock_operations no ON no.operation_id=newer.operation_id WHERE newer.group_id=c.group_id AND no.stock_revision>observed.stock_revision)
    AND NEW.scope_work_count=(SELECT count(*) FROM hub_group_work a WHERE a.group_id=c.group_id)
    AND NEW.changed_work_count=CASE WHEN NEW.effect_kind='shortage' THEN (SELECT count(*) FROM hub_group_work a WHERE a.group_id=c.group_id AND a.progress='pending') ELSE 0 END
    AND ((NEW.effect_kind='none' AND c.counted_packs=c.recorded_packs AND NEW.after_on_hand_packs=c.recorded_packs AND NEW.after_group_revision=c.group_revision)
        OR (NEW.effect_kind='shortage' AND NEW.changed_packs=c.recorded_packs-c.counted_packs AND NEW.after_on_hand_packs=c.counted_packs AND NEW.after_group_revision=c.group_revision+1)
        OR (NEW.effect_kind='surplus' AND NEW.changed_packs=c.counted_packs-c.recorded_packs AND NEW.after_on_hand_packs=c.recorded_packs AND NEW.after_group_revision=c.group_revision))
)
BEGIN SELECT RAISE(ABORT,'count reconciliation must retain its original observed scope'); END;
CREATE TRIGGER hub_count_reconciliation_work_guard BEFORE INSERT ON hub_count_reconciliation_work
WHEN NOT EXISTS (
    SELECT 1 FROM hub_count_reconciliations r
    JOIN hub_count_observations c ON c.count_id=r.count_id
    JOIN hub_stock_operations posted ON posted.operation_id=r.operation_id
    JOIN hub_group_work a ON a.group_id=c.group_id AND a.work_id=NEW.work_id
    JOIN hub_work_requirements w ON w.work_id=a.work_id
    JOIN hub_stock_operations created ON created.operation_id=w.created_operation_id
    WHERE r.reconciliation_id=NEW.reconciliation_id AND NEW.ordinal<r.scope_work_count
    AND w.trace_id=c.trace_id AND w.variant_id=c.variant_id AND created.stock_revision<posted.stock_revision
    AND a.progress=NEW.before_progress AND a.scoped_packs=NEW.before_scoped_packs AND NEW.before_scoped_packs=c.recorded_packs
    AND w.required=NEW.required AND w.completed=NEW.before_completed AND w.no_longer_required=NEW.before_no_longer_required
    AND w.remaining=NEW.before_remaining AND w.revision=NEW.before_revision
    AND NEW.on_hand_after=r.after_on_hand_packs
    AND NEW.removed_packs=CASE WHEN r.effect_kind='shortage' THEN r.changed_packs ELSE 0 END
)
BEGIN SELECT RAISE(ABORT,'count work history must retain its original requirement scope'); END;
CREATE TRIGGER hub_count_reconciliation_no_update BEFORE UPDATE ON hub_count_reconciliations BEGIN SELECT RAISE(ABORT,'count reconciliation is immutable'); END;
CREATE TRIGGER hub_count_reconciliation_no_delete BEFORE DELETE ON hub_count_reconciliations BEGIN SELECT RAISE(ABORT,'count reconciliation is immutable'); END;
CREATE TRIGGER hub_count_reconciliation_work_no_update BEFORE UPDATE ON hub_count_reconciliation_work BEGIN SELECT RAISE(ABORT,'count work history is immutable'); END;
CREATE TRIGGER hub_count_reconciliation_work_no_delete BEFORE DELETE ON hub_count_reconciliation_work BEGIN SELECT RAISE(ABORT,'count work history is immutable'); END;
CREATE TRIGGER hub_counted_surplus_no_update BEFORE UPDATE ON hub_counted_surplus BEGIN SELECT RAISE(ABORT,'counted surplus origin is immutable'); END;
CREATE TRIGGER hub_counted_surplus_no_delete BEFORE DELETE ON hub_counted_surplus BEGIN SELECT RAISE(ABORT,'counted surplus origin is immutable'); END;
DROP TRIGGER hub_group_birth_guard;
CREATE TRIGGER hub_group_birth_guard BEFORE INSERT ON hub_group_identification
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_groups g
    JOIN hub_stock_operations o ON o.operation_id=NEW.created_operation_id
    JOIN hub_receipts r ON r.trace_id=g.trace_id
    JOIN hub_stock_operations original ON original.operation_id=r.operation_id
    WHERE g.group_id=NEW.group_id AND o.hub_id=original.hub_id AND (
        (NEW.parent_group_id IS NULL AND r.root_group_id=g.group_id AND r.operation_id=o.operation_id AND (
            (o.result_id=r.trace_id AND (
                (o.kind='receipt' AND r.origin_kind='receipt' AND NEW.physical_label IS NULL)
                OR (o.kind='opening' AND r.origin_kind='opening' AND NEW.physical_label IS NOT NULL AND EXISTS (
                    SELECT 1 FROM hub_counted_openings entry WHERE entry.population_id=r.trace_id
                ))
            ))
            OR (o.kind='count_post' AND r.origin_kind='surplus' AND NEW.physical_label IS NOT NULL AND EXISTS (
                SELECT 1 FROM hub_counted_surplus entry
                JOIN hub_count_reconciliations counted ON counted.reconciliation_id=entry.reconciliation_id
                JOIN hub_count_observations observed ON observed.count_id=counted.count_id
                WHERE entry.population_id=r.trace_id AND counted.operation_id=o.operation_id AND o.result_id=counted.reconciliation_id
                AND counted.effect_kind='surplus' AND counted.surplus_population_id=r.trace_id AND counted.changed_packs=r.original_packs
                AND r.variant_id=observed.variant_id AND r.receiving_id=observed.location_id
                AND r.supplier_id IS NULL AND r.producer_id IS NULL AND r.source_id IS NULL AND r.source_certainty IS NULL AND r.source_issuer_revision IS NULL
                AND r.planned_location_id IS NULL AND r.occurrence_kind='known' AND r.occurrence_start_unix_s=counted.confirmed_on_hand_at_unix_s
            ))
        ))
        OR
        (NEW.parent_group_id IS NOT NULL AND o.kind IN ('move','split') AND o.result_id=g.group_id AND EXISTS (
            SELECT 1 FROM hub_group_identification parent
            JOIN hub_stock_groups pg ON pg.group_id=parent.group_id
            JOIN hub_stock_operations po ON po.operation_id=parent.created_operation_id
            WHERE parent.group_id=NEW.parent_group_id AND pg.trace_id=g.trace_id AND po.hub_id=o.hub_id AND po.stock_revision<o.stock_revision
        ))
    )
)
BEGIN SELECT RAISE(ABORT,'physical group birth must retain its original population'); END;
