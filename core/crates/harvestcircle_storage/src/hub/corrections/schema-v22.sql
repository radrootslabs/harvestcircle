CREATE TABLE hub_count_return_epochs (
    hub_id BLOB NOT NULL PRIMARY KEY REFERENCES hub_context(hub_id),
    epoch INTEGER NOT NULL CHECK (epoch=1),
    legacy_stock_revision INTEGER NOT NULL CHECK (legacy_stock_revision BETWEEN 1 AND 32769),
    legacy_catalog_revision INTEGER NOT NULL CHECK (legacy_catalog_revision>=1),
    UNIQUE (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
) STRICT;
CREATE TABLE hub_count_return_associations (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    association_id BLOB NOT NULL UNIQUE CHECK (length(association_id)=16),
    hub_id BLOB NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch=1),
    legacy_stock_revision INTEGER NOT NULL,
    legacy_catalog_revision INTEGER NOT NULL,
    stock_revision INTEGER NOT NULL CHECK (stock_revision>legacy_stock_revision),
    catalog_revision INTEGER NOT NULL CHECK (catalog_revision>=legacy_catalog_revision),
    return_operation_id TEXT NOT NULL UNIQUE REFERENCES hub_returns(operation_id),
    explanation_operation_id TEXT NOT NULL REFERENCES hub_correction_count_explanations(operation_id),
    count_operation_id TEXT NOT NULL REFERENCES hub_count_reconciliations(operation_id),
    account_previous_operation_id TEXT NOT NULL REFERENCES hub_corrections(operation_id),
    previous_operation_id TEXT UNIQUE REFERENCES hub_count_return_associations(operation_id),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 65536),
    projection_schema INTEGER NOT NULL CHECK (projection_schema=1),
    projection BLOB NOT NULL CHECK (length(projection) BETWEEN 1 AND 16384),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    capture_hash BLOB NOT NULL CHECK (length(capture_hash)=32),
    FOREIGN KEY (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision) REFERENCES hub_count_return_epochs(hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
) STRICT;
CREATE TRIGGER hub_count_return_epoch_guard BEFORE INSERT ON hub_count_return_epochs
WHEN EXISTS (SELECT 1 FROM hub_count_return_epochs)
OR EXISTS (SELECT 1 FROM hub_stock_operations WHERE kind='count_return_associate')
OR NOT EXISTS (
    SELECT 1 FROM hub_stock_state s JOIN hub_context h ON h.hub_id=s.hub_id
    JOIN hub_count_interpretation_epochs parent ON parent.hub_id=s.hub_id
    WHERE s.hub_id=NEW.hub_id AND NEW.epoch=1
    AND NEW.legacy_stock_revision=s.revision
    AND NEW.legacy_catalog_revision=COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1)
    AND parent.epoch=1 AND parent.legacy_stock_revision<=NEW.legacy_stock_revision
    AND parent.legacy_catalog_revision<=NEW.legacy_catalog_revision
) BEGIN SELECT RAISE(ABORT,'count return epoch requires the exact native hub and parent interpretation cut'); END;
INSERT INTO hub_count_return_epochs (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
SELECT s.hub_id,1,s.revision,COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1) FROM hub_stock_state s;
CREATE TRIGGER hub_count_return_initialize AFTER INSERT ON hub_count_interpretation_epochs
BEGIN INSERT INTO hub_count_return_epochs (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
SELECT s.hub_id,1,s.revision,COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1) FROM hub_stock_state s WHERE s.hub_id=NEW.hub_id; END;
CREATE TRIGGER hub_count_return_epochs_no_update BEFORE UPDATE ON hub_count_return_epochs BEGIN SELECT RAISE(ABORT,'count return epochs are immutable'); END;
CREATE TRIGGER hub_count_return_epochs_no_delete BEFORE DELETE ON hub_count_return_epochs BEGIN SELECT RAISE(ABORT,'count return epochs are retained'); END;
CREATE TABLE hub_count_return_intake_modes (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    hub_id BLOB NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch=1),
    legacy_stock_revision INTEGER NOT NULL,
    legacy_catalog_revision INTEGER NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('owned','legacy_compat')),
    association_count INTEGER NOT NULL CHECK (association_count BETWEEN 0 AND 4096),
    scope_hash BLOB NOT NULL CHECK (length(scope_hash)=32),
    inner_result_hash BLOB NOT NULL CHECK (length(inner_result_hash)=32),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    capture_hash BLOB CHECK (capture_hash IS NULL OR length(capture_hash)=32),
    CHECK ((mode='owned' AND capture_hash IS NOT NULL) OR (mode='legacy_compat' AND capture_hash IS NULL AND association_count=0)),
    FOREIGN KEY (hub_id,epoch,legacy_stock_revision,legacy_catalog_revision) REFERENCES hub_count_return_epochs(hub_id,epoch,legacy_stock_revision,legacy_catalog_revision)
) STRICT;
CREATE TRIGGER hub_count_return_intake_mode_guard BEFORE INSERT ON hub_count_return_intake_modes
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations own JOIN hub_count_return_epochs epoch ON epoch.hub_id=own.hub_id
    WHERE own.operation_id=NEW.operation_id AND own.kind='return' AND own.hub_id=NEW.hub_id
    AND epoch.epoch=NEW.epoch AND epoch.legacy_stock_revision=NEW.legacy_stock_revision
    AND epoch.legacy_catalog_revision=NEW.legacy_catalog_revision
    AND own.stock_revision>epoch.legacy_stock_revision AND own.catalog_revision>=epoch.legacy_catalog_revision
    AND ((NEW.mode='owned' AND EXISTS (
        SELECT 1 FROM hub_submissions admitted JOIN hub_runtime_session runtime ON runtime.singleton=1
        WHERE admitted.operation_id=own.operation_id AND admitted.hub_id=own.hub_id
        AND admitted.kind=own.kind AND admitted.actor_public_key=own.actor_public_key
        AND admitted.domain_intent_hash=own.intent_hash AND admitted.capture_hash=NEW.capture_hash
        AND admitted.runtime_id=runtime.runtime_id AND admitted.generation=runtime.generation
        AND admitted.actor_public_key=runtime.actor_public_key
        AND own.recorded_at_unix_s>=admitted.submitted_at_unix_s
        AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections rejected WHERE rejected.operation_id=own.operation_id)
    )) OR (NEW.mode='legacy_compat' AND NEW.capture_hash IS NULL AND NEW.association_count=0
        AND NOT EXISTS (SELECT 1 FROM hub_runtime_session)
        AND NOT EXISTS (SELECT 1 FROM hub_submissions WHERE operation_id=own.operation_id)
        AND NOT EXISTS (SELECT 1 FROM hub_corrections)
        AND NOT EXISTS (SELECT 1 FROM hub_correction_observations)
        AND NOT EXISTS (SELECT 1 FROM hub_correction_captures)
        AND NOT EXISTS (SELECT 1 FROM hub_correction_return_projections)
        AND NOT EXISTS (SELECT 1 FROM hub_count_return_associations)
    ))
) BEGIN SELECT RAISE(ABORT,'new return mode requires the native epoch and exact current owned admission or explicit pristine compatibility'); END;
CREATE TRIGGER hub_count_return_intake_modes_no_update BEFORE UPDATE ON hub_count_return_intake_modes BEGIN SELECT RAISE(ABORT,'return intake modes are immutable'); END;
CREATE TRIGGER hub_count_return_intake_modes_no_delete BEFORE DELETE ON hub_count_return_intake_modes BEGIN SELECT RAISE(ABORT,'return intake modes are retained'); END;
CREATE TRIGGER hub_count_return_intake_body_guard BEFORE INSERT ON hub_returns
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations own JOIN hub_count_return_epochs epoch ON epoch.hub_id=own.hub_id
    WHERE own.operation_id=NEW.operation_id AND own.kind='return'
    AND (own.stock_revision<=epoch.legacy_stock_revision OR EXISTS (
        SELECT 1 FROM hub_count_return_intake_modes mode WHERE mode.operation_id=own.operation_id
        AND mode.hub_id=own.hub_id AND mode.epoch=epoch.epoch
        AND mode.legacy_stock_revision=epoch.legacy_stock_revision AND mode.legacy_catalog_revision=epoch.legacy_catalog_revision
        AND mode.result_hash=NEW.result_hash
    ))
) BEGIN SELECT RAISE(ABORT,'new return body requires its immutable native mode and wrapped result'); END;
CREATE TRIGGER hub_count_return_association_guard BEFORE INSERT ON hub_count_return_associations
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_operations own
    JOIN hub_count_return_epochs epoch ON epoch.hub_id=own.hub_id
    JOIN hub_submissions admitted ON admitted.operation_id=own.operation_id
    JOIN hub_returns returned ON returned.operation_id=NEW.return_operation_id
    JOIN hub_stock_operations intake ON intake.operation_id=returned.operation_id
    JOIN hub_receipts population ON population.operation_id=intake.operation_id AND population.trace_id=returned.population_id
    JOIN hub_correction_count_explanations explanation ON explanation.operation_id=NEW.explanation_operation_id
    JOIN hub_corrections original ON original.operation_id=explanation.operation_id
    JOIN hub_stock_operations explained ON explained.operation_id=original.operation_id
    JOIN hub_count_reconciliations counted ON counted.operation_id=NEW.count_operation_id
    JOIN hub_stock_operations count_op ON count_op.operation_id=counted.operation_id
    JOIN hub_corrections account_head ON account_head.operation_id=NEW.account_previous_operation_id
    JOIN hub_stock_operations head_op ON head_op.operation_id=account_head.operation_id
    WHERE own.operation_id=NEW.operation_id AND own.kind='count_return_associate'
    AND own.hub_id=NEW.hub_id AND own.result_id=NEW.association_id
    AND own.stock_revision=NEW.stock_revision AND own.catalog_revision=NEW.catalog_revision
    AND epoch.epoch=NEW.epoch AND epoch.legacy_stock_revision=NEW.legacy_stock_revision
    AND epoch.legacy_catalog_revision=NEW.legacy_catalog_revision
    AND admitted.hub_id=own.hub_id AND admitted.actor_public_key=own.actor_public_key
    AND admitted.kind=own.kind AND admitted.domain_intent_hash=own.intent_hash
    AND admitted.capture_hash=NEW.capture_hash
    AND intake.hub_id=own.hub_id AND intake.kind='return' AND intake.stock_revision<own.stock_revision
    AND intake.catalog_revision<=own.catalog_revision AND intake.recorded_at_unix_s<=own.recorded_at_unix_s
    AND NEW.packs=population.original_packs
    AND returned.resolution IN ('unknown','unrecorded') AND returned.established_group_id IS NULL
    AND returned.known_departure_id IS NULL AND returned.source_count=0
    AND explained.hub_id=own.hub_id AND explained.kind='correction_count_explain'
    AND explained.stock_revision<own.stock_revision AND original.family='count_explain'
    AND explained.catalog_revision<=own.catalog_revision AND explained.recorded_at_unix_s<=own.recorded_at_unix_s
    AND original.account_operation_id=counted.operation_id AND original.target_operation_id=counted.operation_id
    AND explanation.count_operation_id=counted.operation_id AND explanation.reconciliation_id=counted.reconciliation_id
    AND counted.effect_kind='shortage' AND count_op.kind='count_post' AND count_op.hub_id=own.hub_id
    AND count_op.stock_revision<explained.stock_revision
    AND account_head.family IN ('count_explain','count_classify')
    AND account_head.account_operation_id=counted.operation_id AND head_op.hub_id=own.hub_id
    AND head_op.stock_revision<own.stock_revision
    AND NOT EXISTS (
        SELECT 1 FROM hub_corrections newer JOIN hub_stock_operations next ON next.operation_id=newer.operation_id
        WHERE newer.account_operation_id=counted.operation_id AND newer.family IN ('count_explain','count_classify')
        AND next.stock_revision>head_op.stock_revision AND next.stock_revision<own.stock_revision
    )
    AND ((NEW.previous_operation_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM hub_count_return_associations prior WHERE prior.count_operation_id=counted.operation_id
    )) OR EXISTS (
        SELECT 1 FROM hub_count_return_associations prior WHERE prior.operation_id=NEW.previous_operation_id
        AND prior.hub_id=own.hub_id AND prior.count_operation_id=counted.operation_id
        AND prior.stock_revision<own.stock_revision AND NOT EXISTS (
            SELECT 1 FROM hub_count_return_associations newer WHERE newer.count_operation_id=counted.operation_id
            AND newer.stock_revision>prior.stock_revision
        )
    ))
) BEGIN SELECT RAISE(ABORT,'count return association requires canonical original scope and exact fresh admission and heads'); END;
CREATE TRIGGER hub_count_return_associations_no_update BEFORE UPDATE ON hub_count_return_associations BEGIN SELECT RAISE(ABORT,'count return associations are immutable'); END;
CREATE TRIGGER hub_count_return_associations_no_delete BEFORE DELETE ON hub_count_return_associations BEGIN SELECT RAISE(ABORT,'count return associations are retained'); END;
