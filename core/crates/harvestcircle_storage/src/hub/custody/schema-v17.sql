CREATE TABLE hub_returns (
    return_id BLOB NOT NULL PRIMARY KEY CHECK (length(return_id)=16 AND return_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    population_id BLOB NOT NULL UNIQUE,
    origin_kind TEXT NOT NULL DEFAULT 'return' CHECK (origin_kind='return'),
    policy_version INTEGER NOT NULL CHECK (policy_version=1),
    returned_by_id BLOB,
    returned_by_revision INTEGER,
    claimed_departure_id BLOB CHECK (claimed_departure_id IS NULL OR (length(claimed_departure_id)=16 AND claimed_departure_id<>zeroblob(16))),
    claimed_group_id BLOB CHECK (claimed_group_id IS NULL OR (length(claimed_group_id)=16 AND claimed_group_id<>zeroblob(16))),
    known_departure_id BLOB REFERENCES hub_departures(departure_id),
    established_group_id BLOB REFERENCES hub_departure_lines(group_id),
    resolution TEXT NOT NULL CHECK (resolution IN ('unknown','unrecorded','unresolved','established')),
    unresolved_reason TEXT CHECK (unresolved_reason IS NULL OR unresolved_reason IN ('original_group_unknown','no_matching_variant','claimed_group_missing','variant_conflict','quantity_conflict','time_conflict')),
    timing TEXT NOT NULL CHECK (timing IN ('after_or_same','uncertain','contradictory')),
    source_count INTEGER NOT NULL CHECK (source_count BETWEEN 0 AND 64),
    intake_work_id BLOB REFERENCES hub_work_requirements(work_id) DEFERRABLE INITIALLY DEFERRED,
    population_hash BLOB NOT NULL CHECK (length(population_hash)=32),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    FOREIGN KEY (population_id,origin_kind) REFERENCES hub_receipts(trace_id,origin_kind),
    FOREIGN KEY (returned_by_id,returned_by_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK ((returned_by_id IS NULL AND returned_by_revision IS NULL) OR (returned_by_id IS NOT NULL AND returned_by_revision IS NOT NULL AND length(returned_by_id)=16 AND returned_by_id<>zeroblob(16) AND returned_by_revision>=1)),
    CHECK (claimed_group_id IS NULL OR claimed_departure_id IS NOT NULL),
    CHECK ((known_departure_id IS NULL AND source_count=0) OR (known_departure_id IS NOT NULL AND source_count BETWEEN 1 AND 64 AND known_departure_id IS claimed_departure_id)),
    CHECK ((resolution='unknown' AND claimed_departure_id IS NULL AND known_departure_id IS NULL AND established_group_id IS NULL AND unresolved_reason IS NULL AND timing='uncertain') OR (resolution='unrecorded' AND claimed_departure_id IS NOT NULL AND known_departure_id IS NULL AND established_group_id IS NULL AND unresolved_reason IS NULL AND timing='uncertain') OR (resolution='unresolved' AND known_departure_id IS NOT NULL AND established_group_id IS NULL AND unresolved_reason IS NOT NULL) OR (resolution='established' AND known_departure_id IS NOT NULL AND claimed_group_id IS NOT NULL AND established_group_id IS NOT NULL AND established_group_id=claimed_group_id AND unresolved_reason IS NULL AND timing IN ('after_or_same','uncertain')))
) STRICT;
CREATE INDEX hub_return_departure_scope ON hub_returns(known_departure_id,established_group_id);
CREATE TABLE hub_return_sources (
    return_id BLOB NOT NULL REFERENCES hub_returns(return_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    departure_operation_id TEXT NOT NULL,
    original_group_id BLOB NOT NULL REFERENCES hub_departure_lines(group_id),
    previously_returned_packs INTEGER NOT NULL CHECK (previously_returned_packs BETWEEN 0 AND 1000000),
    PRIMARY KEY (return_id,ordinal),
    UNIQUE (return_id,original_group_id),
    FOREIGN KEY (departure_operation_id,ordinal) REFERENCES hub_departure_lines(operation_id,ordinal)
) STRICT;
CREATE TABLE hub_dispositions (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    disposition_id BLOB NOT NULL UNIQUE CHECK (length(disposition_id)=16 AND disposition_id<>zeroblob(16)),
    policy_version INTEGER NOT NULL CHECK (policy_version=1),
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
    work_change_count INTEGER NOT NULL CHECK (work_change_count BETWEEN 0 AND scope_count),
    disposition_kind TEXT NOT NULL CHECK (disposition_kind IN ('destroyed','other_observed_removal')),
    occurrence_kind TEXT NOT NULL CHECK (occurrence_kind IN ('unknown','known','interval')),
    occurrence_start_unix_s INTEGER,
    occurrence_end_unix_s INTEGER,
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    FOREIGN KEY (location_id,location_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    CHECK (before_causal_floor_unix_s IS NULL OR (after_causal_floor_unix_s IS NOT NULL AND after_causal_floor_unix_s>=before_causal_floor_unix_s)),
    CHECK ((occurrence_kind='unknown' AND occurrence_start_unix_s IS NULL AND occurrence_end_unix_s IS NULL AND after_causal_floor_unix_s IS before_causal_floor_unix_s) OR (occurrence_kind='known' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_start_unix_s=occurrence_end_unix_s) OR (occurrence_kind='interval' AND occurrence_start_unix_s IS NOT NULL AND occurrence_end_unix_s IS NOT NULL AND occurrence_start_unix_s>=0 AND occurrence_start_unix_s<=occurrence_end_unix_s))
) STRICT;
CREATE INDEX hub_disposition_trace ON hub_dispositions(trace_id,operation_id);
CREATE TABLE hub_disposition_work_scope (
    operation_id TEXT NOT NULL REFERENCES hub_dispositions(operation_id),
    work_id BLOB NOT NULL REFERENCES hub_work_requirements(work_id),
    before_progress TEXT NOT NULL CHECK (before_progress IN ('pending','completed','no_longer_required')),
    before_work_revision INTEGER NOT NULL CHECK (before_work_revision>=1),
    before_completed INTEGER NOT NULL CHECK (before_completed BETWEEN 0 AND 1000000),
    before_no_longer_required INTEGER NOT NULL CHECK (before_no_longer_required BETWEEN 0 AND 1000000),
    before_remaining INTEGER NOT NULL CHECK (before_remaining BETWEEN 0 AND 1000000),
    PRIMARY KEY (operation_id,work_id)
) STRICT;
CREATE TRIGGER hub_return_guard BEFORE INSERT ON hub_returns WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_receipts p ON p.operation_id=o.operation_id
    WHERE o.operation_id=NEW.operation_id AND o.kind='return' AND o.result_id=NEW.return_id
    AND p.trace_id=NEW.population_id AND p.origin_kind='return' AND p.original_packs>0 AND p.supplier_id IS NULL AND p.producer_id IS NULL
    AND ((NEW.intake_work_id IS NULL AND p.planned_location_id IS NULL) OR (NEW.intake_work_id IS NOT NULL AND p.planned_location_id IS NOT NULL AND p.planned_location_id<>p.receiving_id))
    AND (NEW.returned_by_id IS NULL OR EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.record_id=NEW.returned_by_id AND v.record_revision=NEW.returned_by_revision AND v.hub_id=o.hub_id AND v.kind='party' AND v.catalog_revision<=o.catalog_revision))
    AND (NEW.resolution<>'unrecorded' OR NOT EXISTS (SELECT 1 FROM hub_departures d WHERE d.departure_id=NEW.claimed_departure_id))
    AND (NEW.known_departure_id IS NULL OR EXISTS (SELECT 1 FROM hub_departures d JOIN hub_stock_operations original ON original.operation_id=d.operation_id WHERE d.departure_id=NEW.known_departure_id AND d.line_count=NEW.source_count AND original.hub_id=o.hub_id AND original.stock_revision<o.stock_revision AND original.catalog_revision<=o.catalog_revision))
    AND (NEW.resolution<>'established' OR EXISTS (
        SELECT 1 FROM hub_departures d JOIN hub_departure_lines line ON line.operation_id=d.operation_id JOIN hub_receipts source ON source.trace_id=line.trace_id
        WHERE d.departure_id=NEW.known_departure_id AND line.group_id=NEW.established_group_id AND source.variant_id=p.variant_id
        AND p.original_packs+COALESCE((SELECT sum(prior_population.original_packs) FROM hub_returns prior JOIN hub_receipts prior_population ON prior_population.trace_id=prior.population_id WHERE prior.known_departure_id=d.departure_id AND prior.established_group_id=line.group_id AND prior.resolution='established'),0)<=line.packs
    ))
) BEGIN SELECT RAISE(ABORT,'returned custody needs its distinct original and bounded explicit association'); END;
CREATE TRIGGER hub_return_source_guard BEFORE INSERT ON hub_return_sources WHEN NOT EXISTS (
    SELECT 1 FROM hub_returns returned JOIN hub_stock_operations current ON current.operation_id=returned.operation_id
    JOIN hub_departures d ON d.departure_id=returned.known_departure_id
    JOIN hub_departure_lines line ON line.operation_id=d.operation_id AND line.ordinal=NEW.ordinal
    WHERE returned.return_id=NEW.return_id AND NEW.ordinal<returned.source_count AND NEW.departure_operation_id=d.operation_id AND NEW.original_group_id=line.group_id
    AND NEW.previously_returned_packs<=line.packs AND NEW.previously_returned_packs=COALESCE((SELECT sum(p.original_packs) FROM hub_returns prior JOIN hub_receipts p ON p.trace_id=prior.population_id JOIN hub_stock_operations original ON original.operation_id=prior.operation_id WHERE prior.return_id<>returned.return_id AND prior.known_departure_id=d.departure_id AND prior.established_group_id=line.group_id AND prior.resolution='established' AND original.stock_revision<current.stock_revision),0)
) BEGIN SELECT RAISE(ABORT,'returned-source context must retain every original line and exact prior association total'); END;
CREATE TRIGGER hub_disposition_guard BEFORE INSERT ON hub_dispositions WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_stock_groups g ON g.group_id=NEW.group_id
    JOIN hub_stock_positions p ON p.group_id=g.group_id AND p.location_id=NEW.location_id
    WHERE o.operation_id=NEW.operation_id AND o.kind='disposition' AND o.result_id=NEW.disposition_id
    AND g.trace_id=NEW.trace_id AND g.revision=NEW.before_group_revision
    AND p.packs=NEW.packs AND EXISTS (SELECT 1 FROM hub_reference_versions location WHERE location.record_id=NEW.location_id AND location.record_revision=NEW.location_revision AND location.hub_id=o.hub_id AND location.kind='location' AND location.catalog_revision<=o.catalog_revision AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=location.record_id AND newer.catalog_revision<=o.catalog_revision AND newer.record_revision>location.record_revision)) AND (SELECT count(*) FROM hub_stock_positions all_positions WHERE all_positions.group_id=g.group_id AND all_positions.packs>0)=1
    AND NOT EXISTS (SELECT 1 FROM hub_departure_lines d WHERE d.group_id=g.group_id)
    AND NEW.scope_count=(SELECT count(*) FROM hub_group_work a WHERE a.group_id=g.group_id)
    AND NEW.work_change_count=(SELECT count(*) FROM hub_group_work a WHERE a.group_id=g.group_id AND a.progress='pending')
    AND (NEW.occurrence_kind='unknown' OR NEW.occurrence_end_unix_s<=o.recorded_at_unix_s)
) BEGIN SELECT RAISE(ABORT,'disposition needs its exact unconsumed whole physical group and work scope'); END;
CREATE TRIGGER hub_disposition_scope_guard BEFORE INSERT ON hub_disposition_work_scope WHEN NOT EXISTS (
    SELECT 1 FROM hub_dispositions d JOIN hub_group_work a ON a.group_id=d.group_id AND a.work_id=NEW.work_id
    JOIN hub_work_requirements w ON w.work_id=a.work_id JOIN hub_receipts p ON p.trace_id=d.trace_id
    WHERE d.operation_id=NEW.operation_id AND a.scoped_packs=d.packs AND a.progress=NEW.before_progress
    AND w.trace_id=d.trace_id AND w.variant_id=p.variant_id AND w.revision=NEW.before_work_revision
    AND w.completed=NEW.before_completed AND w.no_longer_required=NEW.before_no_longer_required AND w.remaining=NEW.before_remaining
    AND w.required=NEW.before_completed+NEW.before_no_longer_required+NEW.before_remaining
) BEGIN SELECT RAISE(ABORT,'disposition work must retain every complete original allocation'); END;
CREATE TRIGGER hub_returns_no_update BEFORE UPDATE ON hub_returns BEGIN SELECT RAISE(ABORT,'returned originals are immutable'); END;
CREATE TRIGGER hub_returns_no_delete BEFORE DELETE ON hub_returns BEGIN SELECT RAISE(ABORT,'returned originals are retained'); END;
CREATE TRIGGER hub_return_sources_no_update BEFORE UPDATE ON hub_return_sources BEGIN SELECT RAISE(ABORT,'returned source snapshots are immutable'); END;
CREATE TRIGGER hub_return_sources_no_delete BEFORE DELETE ON hub_return_sources BEGIN SELECT RAISE(ABORT,'returned source snapshots are retained'); END;
CREATE TRIGGER hub_dispositions_no_update BEFORE UPDATE ON hub_dispositions BEGIN SELECT RAISE(ABORT,'actual dispositions are immutable'); END;
CREATE TRIGGER hub_dispositions_no_delete BEFORE DELETE ON hub_dispositions BEGIN SELECT RAISE(ABORT,'actual dispositions are retained'); END;
CREATE TRIGGER hub_disposition_scope_no_update BEFORE UPDATE ON hub_disposition_work_scope BEGIN SELECT RAISE(ABORT,'disposition scope is immutable'); END;
CREATE TRIGGER hub_disposition_scope_no_delete BEFORE DELETE ON hub_disposition_work_scope BEGIN SELECT RAISE(ABORT,'disposition scope is retained'); END;
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
            OR (o.kind='return' AND r.origin_kind='return' AND NEW.physical_label IS NOT NULL AND EXISTS (
                SELECT 1 FROM hub_returns returned WHERE returned.population_id=r.trace_id AND returned.operation_id=o.operation_id
                AND returned.return_id=o.result_id AND r.supplier_id IS NULL AND r.producer_id IS NULL
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
DROP TRIGGER hub_departure_line_guard;
CREATE TRIGGER hub_departure_line_guard BEFORE INSERT ON hub_departure_lines WHEN NOT EXISTS (SELECT 1 FROM hub_departures d JOIN hub_dispatch_identifications i ON i.identification_id=d.identification_id JOIN hub_dispatch_material_lines m ON m.operation_id=i.material_operation_id AND m.ordinal=NEW.ordinal JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_stock_positions p ON p.group_id=g.group_id AND p.location_id=NEW.location_id WHERE d.operation_id=NEW.operation_id AND NEW.ordinal<d.line_count AND m.group_id=NEW.group_id AND m.trace_id=NEW.trace_id AND m.group_revision=NEW.before_group_revision AND m.packs=NEW.packs AND m.location_id=NEW.location_id AND m.location_revision=NEW.location_revision AND g.trace_id=NEW.trace_id AND g.revision=NEW.before_group_revision AND p.packs=NEW.packs AND (SELECT count(*) FROM hub_stock_positions all_positions WHERE all_positions.group_id=g.group_id AND all_positions.packs>0)=1 AND NEW.scope_count=(SELECT count(*) FROM hub_group_work a WHERE a.group_id=g.group_id)) OR EXISTS (SELECT 1 FROM hub_dispositions consumed WHERE consumed.group_id=NEW.group_id) BEGIN SELECT RAISE(ABORT,'departure must retain its complete current whole group'); END;
DROP TRIGGER hub_submission_uncertain_stock_guard;
CREATE TRIGGER hub_submission_uncertain_stock_guard BEFORE INSERT ON hub_stock_operations
WHEN NEW.kind IN ('receipt','opening','move','split','work_new','work_replan','dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','departure','count_post','return','disposition') AND (
    EXISTS (SELECT 1 FROM hub_actual_report_reviews WHERE review_operation_id IS NULL) OR EXISTS (
        SELECT 1 FROM hub_submissions s WHERE s.operation_id<>NEW.operation_id
        AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections r WHERE r.operation_id=s.operation_id)
        AND NOT EXISTS (SELECT 1 FROM hub_history h WHERE h.operation_id=s.operation_id AND s.kind='initialize' AND h.event_kind='initialized' AND h.hub_id=s.hub_id AND h.actor_public_key=s.actor_public_key AND h.recorded_at_unix_s>=s.submitted_at_unix_s)
        AND NOT EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.operation_id=s.operation_id AND s.kind='reference' AND v.hub_id=s.hub_id AND v.actor_public_key=s.actor_public_key AND v.intent_hash=s.domain_intent_hash AND v.recorded_at_unix_s>=s.submitted_at_unix_s)
        AND NOT EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=s.operation_id AND o.hub_id=s.hub_id AND o.actor_public_key=s.actor_public_key AND o.kind=s.kind AND o.intent_hash=s.domain_intent_hash AND o.recorded_at_unix_s>=s.submitted_at_unix_s)
    )
)
BEGIN SELECT RAISE(ABORT,'planned stock effects require prior outcomes and actual reports to be reviewed'); END;
