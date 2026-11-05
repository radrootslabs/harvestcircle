CREATE TABLE hub_count_interpretation_epochs (
    hub_id BLOB NOT NULL PRIMARY KEY REFERENCES hub_context(hub_id),
    epoch INTEGER NOT NULL CHECK (epoch=1),
    legacy_stock_revision INTEGER NOT NULL CHECK (legacy_stock_revision BETWEEN 1 AND 32769),
    legacy_catalog_revision INTEGER NOT NULL CHECK (legacy_catalog_revision>=1),
    UNIQUE (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
) STRICT;
CREATE TABLE hub_count_interpretation_markers (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    hub_id BLOB NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch=1),
    legacy_stock_revision INTEGER NOT NULL,
    legacy_catalog_revision INTEGER NOT NULL,
    stock_revision INTEGER NOT NULL CHECK (stock_revision>legacy_stock_revision),
    catalog_revision INTEGER NOT NULL CHECK (catalog_revision>=legacy_catalog_revision),
    capture_hash BLOB NOT NULL CHECK (length(capture_hash)=32),
    FOREIGN KEY (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision) REFERENCES hub_count_interpretation_epochs(hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
) STRICT;
CREATE TRIGGER hub_count_interpretation_epoch_guard BEFORE INSERT ON hub_count_interpretation_epochs
WHEN EXISTS (SELECT 1 FROM hub_count_interpretation_epochs) OR EXISTS (SELECT 1 FROM hub_stock_operations WHERE kind IN ('correction_count_explain','correction_count_classify')) OR NOT EXISTS (
    SELECT 1 FROM hub_stock_state s JOIN hub_context h ON h.hub_id=s.hub_id WHERE s.hub_id=NEW.hub_id
    AND NEW.epoch=1 AND NEW.legacy_stock_revision=s.revision
    AND NEW.legacy_catalog_revision=COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1)
) BEGIN SELECT RAISE(ABORT,'interpretation epoch belongs to the exact native hub and activation cut'); END;
INSERT INTO hub_count_interpretation_epochs (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
SELECT s.hub_id,1,s.revision,COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1) FROM hub_stock_state s;
CREATE TRIGGER hub_count_interpretation_initialize AFTER INSERT ON hub_stock_state
BEGIN INSERT INTO hub_count_interpretation_epochs (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision) VALUES (NEW.hub_id,1,NEW.revision,COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=NEW.hub_id),1)); END;
CREATE TRIGGER hub_count_interpretation_epochs_no_update BEFORE UPDATE ON hub_count_interpretation_epochs BEGIN SELECT RAISE(ABORT,'interpretation epochs are immutable'); END;
CREATE TRIGGER hub_count_interpretation_epochs_no_delete BEFORE DELETE ON hub_count_interpretation_epochs BEGIN SELECT RAISE(ABORT,'interpretation epochs are retained'); END;
CREATE TRIGGER hub_count_interpretation_marker_guard BEFORE INSERT ON hub_count_interpretation_markers
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_count_interpretation_epochs e ON e.hub_id=o.hub_id
    JOIN hub_submissions s ON s.operation_id=o.operation_id
    WHERE o.operation_id=NEW.operation_id AND o.kind IN ('actual_review','correction_count_explain','correction_count_classify') AND o.hub_id=NEW.hub_id
    AND e.epoch=NEW.epoch AND e.legacy_stock_revision=NEW.legacy_stock_revision AND e.legacy_catalog_revision=NEW.legacy_catalog_revision
    AND o.stock_revision>e.legacy_stock_revision AND o.stock_revision=NEW.stock_revision AND o.catalog_revision=NEW.catalog_revision
    AND s.hub_id=o.hub_id AND s.actor_public_key=o.actor_public_key AND s.kind=o.kind AND s.domain_intent_hash=o.intent_hash AND s.capture_hash=NEW.capture_hash
) BEGIN SELECT RAISE(ABORT,'qualified review marker binds its canonical epoch operation and exact admission'); END;
CREATE TRIGGER hub_count_interpretation_markers_no_update BEFORE UPDATE ON hub_count_interpretation_markers BEGIN SELECT RAISE(ABORT,'interpretation markers are immutable'); END;
CREATE TRIGGER hub_count_interpretation_markers_no_delete BEFORE DELETE ON hub_count_interpretation_markers BEGIN SELECT RAISE(ABORT,'interpretation markers are retained'); END;
CREATE TRIGGER hub_count_correction_mode_guard BEFORE INSERT ON hub_corrections
WHEN NEW.family IN ('count_explain','count_classify') AND NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_count_interpretation_markers mode ON mode.operation_id=o.operation_id
    JOIN hub_count_interpretation_epochs epoch ON epoch.hub_id=o.hub_id
    WHERE o.operation_id=NEW.operation_id AND mode.hub_id=o.hub_id AND mode.epoch=epoch.epoch
    AND mode.legacy_stock_revision=epoch.legacy_stock_revision AND mode.legacy_catalog_revision=epoch.legacy_catalog_revision
    AND o.stock_revision>epoch.legacy_stock_revision AND mode.stock_revision=o.stock_revision AND mode.catalog_revision=o.catalog_revision
    AND o.kind=CASE NEW.family WHEN 'count_explain' THEN 'correction_count_explain' ELSE 'correction_count_classify' END
) BEGIN SELECT RAISE(ABORT,'count correction requires its immutable native interpretation marker'); END;
DROP TRIGGER hub_actual_review_guard;
CREATE TRIGGER hub_actual_review_guard BEFORE INSERT ON hub_actual_reviews WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations o JOIN hub_actual_reports r ON r.report_id=NEW.report_id
    JOIN hub_stock_operations original ON original.operation_id=r.operation_id
    JOIN hub_actual_report_reviews review ON review.report_id=r.report_id AND review.operation_id=r.operation_id
    JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_receipts population ON population.trace_id=g.trace_id
    WHERE o.operation_id=NEW.operation_id AND o.kind='actual_review' AND o.result_id=NEW.report_id
    AND EXISTS (SELECT 1 FROM hub_actual_interpretation_epochs epoch JOIN hub_actual_interpretation_markers mode ON mode.operation_id=o.operation_id AND mode.hub_id=epoch.hub_id AND mode.epoch=epoch.epoch AND mode.legacy_stock_revision=epoch.legacy_stock_revision AND mode.legacy_catalog_revision=epoch.legacy_catalog_revision WHERE epoch.hub_id=o.hub_id AND o.stock_revision>epoch.legacy_stock_revision AND mode.stock_revision=o.stock_revision AND mode.catalog_revision=o.catalog_revision)
    AND EXISTS (SELECT 1 FROM hub_count_interpretation_epochs epoch JOIN hub_count_interpretation_markers mode ON mode.operation_id=o.operation_id AND mode.hub_id=epoch.hub_id AND mode.epoch=epoch.epoch AND mode.legacy_stock_revision=epoch.legacy_stock_revision AND mode.legacy_catalog_revision=epoch.legacy_catalog_revision WHERE epoch.hub_id=o.hub_id AND o.stock_revision>epoch.legacy_stock_revision AND mode.stock_revision=o.stock_revision AND mode.catalog_revision=o.catalog_revision)
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
        AND EXISTS (SELECT 1 FROM hub_stock_operations witness JOIN hub_stock_groups source ON source.group_id=NEW.source_group_id WHERE witness.operation_id=NEW.source_operation_id AND witness.hub_id=o.hub_id AND witness.stock_revision<o.stock_revision AND witness.recorded_at_unix_s<=o.recorded_at_unix_s AND (witness.kind IN ('receipt','opening','return','count_post','move','split') OR (witness.kind='correction_found' AND EXISTS (
            SELECT 1 FROM hub_corrections correction
            JOIN hub_correction_found_allocations allocation ON allocation.operation_id=correction.operation_id AND allocation.child_group_id=source.group_id
            JOIN hub_group_identification birth ON birth.group_id=source.group_id AND birth.created_operation_id=witness.operation_id AND birth.parent_group_id=allocation.debit_group_id
            JOIN hub_stock_groups parent ON parent.group_id=allocation.debit_group_id AND parent.trace_id=source.trace_id
            JOIN hub_stock_operations debit ON debit.operation_id=allocation.debit_operation_id AND debit.hub_id=witness.hub_id AND debit.stock_revision=allocation.debit_stock_revision AND debit.stock_revision<witness.stock_revision
            JOIN hub_correction_initial_reviews initial ON initial.operation_id=witness.operation_id AND initial.trace_id=source.trace_id AND initial.capture_stock_revision=witness.stock_revision
            JOIN hub_correction_initial_review_anchors anchor ON anchor.operation_id=initial.operation_id AND anchor.group_id=source.group_id AND anchor.created_revision=witness.stock_revision AND anchor.packs=allocation.packs
            JOIN hub_correction_quantity_changes quantity ON quantity.operation_id=witness.operation_id AND quantity.target_operation_id=correction.target_operation_id AND quantity.group_id=correction.subject_id AND quantity.delta_kind='found'
            JOIN hub_stock_events born ON born.operation_id=witness.operation_id AND born.group_id=source.group_id AND born.trace_id=source.trace_id AND born.kind='correction_found' AND born.packs=allocation.packs AND born.from_location_id IS NULL AND born.to_location_id=NEW.historical_location_id AND born.to_location_revision=NEW.historical_location_revision
            WHERE correction.operation_id=witness.operation_id AND correction.family='found' AND correction.correction_id=witness.result_id
            AND allocation.birth_schema=1 AND length(allocation.birth_payload) BETWEEN 1 AND 16384 AND length(allocation.projection_hash)=32
            AND initial.payload_schema=1 AND length(initial.result_hash)=32 AND allocation.packs<=quantity.delta_packs
            AND ((debit.kind='departure' AND EXISTS (SELECT 1 FROM hub_departure_lines line WHERE line.operation_id=debit.operation_id AND line.group_id=parent.group_id AND line.trace_id=source.trace_id AND line.packs>=allocation.packs))
                OR (debit.kind='actual_review' AND EXISTS (SELECT 1 FROM hub_actual_reviews prior WHERE prior.operation_id=debit.operation_id AND prior.review_kind='departure' AND prior.group_id=parent.group_id AND prior.trace_id=source.trace_id AND prior.packs>=allocation.packs))
                OR (debit.kind='correction_additional' AND EXISTS (SELECT 1 FROM hub_correction_physical_debits additional WHERE additional.operation_id=debit.operation_id AND additional.group_id=parent.group_id AND additional.trace_id=source.trace_id AND additional.packs>=allocation.packs)))
        ))) AND source.trace_id=g.trace_id AND EXISTS (SELECT 1 FROM hub_reference_versions location WHERE location.record_id=NEW.historical_location_id AND location.record_revision=NEW.historical_location_revision AND location.hub_id=o.hub_id AND location.kind='location' AND location.catalog_revision<=witness.catalog_revision AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=location.record_id AND newer.catalog_revision<=witness.catalog_revision AND newer.record_revision>location.record_revision))))
    OR (NEW.review_kind='link' AND EXISTS (SELECT 1 FROM hub_stock_operations known WHERE known.operation_id=NEW.known_operation_id AND known.hub_id=o.hub_id AND known.stock_revision<o.stock_revision AND ((known.kind='departure' AND EXISTS (SELECT 1 FROM hub_departure_lines line WHERE line.operation_id=known.operation_id AND line.group_id=g.group_id AND line.trace_id=g.trace_id)) OR (known.kind='actual_review' AND EXISTS (SELECT 1 FROM hub_actual_reviews prior WHERE prior.operation_id=known.operation_id AND prior.review_kind='departure' AND prior.trace_id=g.trace_id AND prior.group_id=g.group_id)) OR (known.kind='correction_count_explain' AND EXISTS (SELECT 1 FROM hub_corrections explanation JOIN hub_correction_count_explanations quantity ON quantity.operation_id=explanation.operation_id JOIN hub_stock_operations counted ON counted.operation_id=quantity.count_operation_id JOIN hub_count_reconciliations reconciliation ON reconciliation.operation_id=counted.operation_id JOIN hub_count_observations observed ON observed.count_id=reconciliation.count_id WHERE explanation.operation_id=known.operation_id AND explanation.family='count_explain' AND explanation.account_operation_id=counted.operation_id AND explanation.target_operation_id=counted.operation_id AND explanation.subject_id=reconciliation.reconciliation_id AND quantity.reconciliation_id=reconciliation.reconciliation_id AND reconciliation.effect_kind='shortage' AND observed.group_id=g.group_id AND observed.trace_id=g.trace_id AND observed.variant_id=NEW.variant_id AND quantity.packs=NEW.packs AND counted.hub_id=known.hub_id AND counted.stock_revision<known.stock_revision))))))
) BEGIN SELECT RAISE(ABORT,'actual review needs its original unresolved report and bounded explicit native evidence'); END;
