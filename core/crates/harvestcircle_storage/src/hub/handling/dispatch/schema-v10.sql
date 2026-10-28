CREATE TABLE hub_dispatch_materials (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    preparation_id BLOB NOT NULL CHECK (length(preparation_id)=16 AND preparation_id<>zeroblob(16)),
    captured_stock_revision INTEGER NOT NULL CHECK (captured_stock_revision>=1),
    catalog_revision INTEGER NOT NULL CHECK (catalog_revision>=1),
    recipient_id BLOB NOT NULL,
    recipient_revision INTEGER NOT NULL CHECK (recipient_revision>=1),
    basis_id TEXT NOT NULL CHECK (length(CAST(basis_id AS BLOB)) BETWEEN 1 AND 64),
    basis_version INTEGER NOT NULL CHECK (basis_version BETWEEN 1 AND 4294967295),
    site_timezone TEXT NOT NULL CHECK (length(CAST(site_timezone AS BLOB)) BETWEEN 1 AND 64),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    line_count INTEGER NOT NULL CHECK (line_count BETWEEN 1 AND 64),
    FOREIGN KEY (recipient_id,recipient_revision) REFERENCES hub_reference_versions(record_id,record_revision)
) STRICT;
CREATE TABLE hub_dispatch_material_lines (
    operation_id TEXT NOT NULL REFERENCES hub_dispatch_materials(operation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    group_id BLOB NOT NULL REFERENCES hub_group_identification(group_id),
    group_revision INTEGER NOT NULL CHECK (group_revision>=1),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    product_id BLOB NOT NULL,
    product_revision INTEGER NOT NULL CHECK (product_revision>=1),
    variant_id BLOB NOT NULL,
    variant_revision INTEGER NOT NULL CHECK (variant_revision>=1),
    location_id BLOB NOT NULL,
    location_revision INTEGER NOT NULL CHECK (location_revision>=1),
    PRIMARY KEY (operation_id,ordinal),
    UNIQUE (operation_id,group_id),
    FOREIGN KEY (product_id,product_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (variant_id,variant_revision) REFERENCES hub_reference_versions(record_id,record_revision),
    FOREIGN KEY (location_id,location_revision) REFERENCES hub_reference_versions(record_id,record_revision)
) STRICT;
CREATE TABLE hub_dispatch_identifications (
    identification_id BLOB NOT NULL PRIMARY KEY CHECK (length(identification_id)=16 AND identification_id<>zeroblob(16)),
    preparation_id BLOB NOT NULL CHECK (length(preparation_id)=16 AND preparation_id<>zeroblob(16)),
    version_number INTEGER NOT NULL CHECK (version_number BETWEEN 1 AND 32),
    previous_identification_id BLOB REFERENCES hub_dispatch_identifications(identification_id),
    issued_operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    material_operation_id TEXT NOT NULL REFERENCES hub_dispatch_materials(operation_id),
    UNIQUE (preparation_id,version_number),
    CHECK ((version_number=1 AND previous_identification_id IS NULL) OR (version_number>=2 AND previous_identification_id IS NOT NULL AND previous_identification_id<>identification_id))
) STRICT;
CREATE TABLE hub_dispatch_changes (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    preparation_id BLOB NOT NULL CHECK (length(preparation_id)=16 AND preparation_id<>zeroblob(16)),
    record_revision INTEGER NOT NULL CHECK (record_revision>=1),
    expected_record_revision INTEGER,
    kind TEXT NOT NULL CHECK (kind IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused')),
    material_operation_id TEXT NOT NULL REFERENCES hub_dispatch_materials(operation_id),
    identification_id BLOB CHECK (identification_id IS NULL OR (length(identification_id)=16 AND identification_id<>zeroblob(16))),
    previous_identification_id BLOB REFERENCES hub_dispatch_identifications(identification_id),
    observation TEXT NOT NULL CHECK (length(CAST(observation AS BLOB)) BETWEEN 1 AND 512),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    UNIQUE (preparation_id,record_revision),
    CHECK ((kind='dispatch_prepare' AND record_revision=1 AND expected_record_revision IS NULL AND material_operation_id=operation_id AND identification_id IS NULL AND previous_identification_id IS NULL) OR (kind<>'dispatch_prepare' AND expected_record_revision IS NOT NULL AND expected_record_revision=record_revision-1 AND record_revision>=2 AND ((kind='dispatch_revise' AND material_operation_id=operation_id AND identification_id IS NULL AND previous_identification_id IS NULL) OR (kind='dispatch_issue' AND material_operation_id<>operation_id AND identification_id IS NOT NULL AND previous_identification_id IS NULL) OR (kind='dispatch_replace' AND material_operation_id=operation_id AND identification_id IS NOT NULL AND previous_identification_id IS NOT NULL AND identification_id<>previous_identification_id) OR (kind='dispatch_unused' AND material_operation_id<>operation_id AND identification_id IS NULL AND previous_identification_id IS NOT NULL))))
) STRICT;
CREATE INDEX hub_dispatch_material_trace ON hub_dispatch_material_lines(trace_id,operation_id);
CREATE TRIGGER hub_dispatch_material_guard BEFORE INSERT ON hub_dispatch_materials WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_reference_versions p ON p.record_id=NEW.recipient_id AND p.record_revision=NEW.recipient_revision WHERE o.operation_id=NEW.operation_id AND o.kind IN ('dispatch_prepare','dispatch_revise','dispatch_replace') AND o.result_id=NEW.preparation_id AND o.stock_revision=NEW.captured_stock_revision+1 AND o.catalog_revision=NEW.catalog_revision AND p.hub_id=o.hub_id AND p.kind='party' AND p.catalog_revision<=o.catalog_revision) BEGIN SELECT RAISE(ABORT,'dispatch material must retain current native capture'); END;
CREATE TRIGGER hub_dispatch_line_guard BEFORE INSERT ON hub_dispatch_material_lines WHEN NOT EXISTS (SELECT 1 FROM hub_dispatch_materials m JOIN hub_stock_operations o ON o.operation_id=m.operation_id JOIN hub_stock_groups g ON g.group_id=NEW.group_id JOIN hub_receipts r ON r.trace_id=g.trace_id JOIN hub_stock_operations received ON received.operation_id=r.operation_id JOIN hub_reference_versions p ON p.record_id=NEW.product_id AND p.record_revision=NEW.product_revision JOIN hub_reference_versions v ON v.record_id=NEW.variant_id AND v.record_revision=NEW.variant_revision JOIN hub_reference_versions l ON l.record_id=NEW.location_id AND l.record_revision=NEW.location_revision WHERE m.operation_id=NEW.operation_id AND g.trace_id=NEW.trace_id AND g.revision=NEW.group_revision AND r.variant_id=NEW.variant_id AND received.hub_id=o.hub_id AND NEW.packs=(SELECT COALESCE(sum(packs),0) FROM hub_stock_positions WHERE group_id=g.group_id AND location_id=NEW.location_id) AND NEW.ordinal<m.line_count AND p.hub_id=o.hub_id AND p.kind='product' AND p.catalog_revision<=m.catalog_revision AND v.hub_id=o.hub_id AND v.kind='pack' AND v.product_id=NEW.product_id AND v.catalog_revision<=m.catalog_revision AND l.hub_id=o.hub_id AND l.kind='location' AND l.catalog_revision<=m.catalog_revision) BEGIN SELECT RAISE(ABORT,'dispatch line must retain a whole current physical group'); END;
CREATE TRIGGER hub_dispatch_change_guard BEFORE INSERT ON hub_dispatch_changes WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_dispatch_materials m ON m.operation_id=NEW.material_operation_id WHERE o.operation_id=NEW.operation_id AND o.kind=NEW.kind AND o.result_id=NEW.preparation_id AND m.preparation_id=NEW.preparation_id AND ((NEW.kind='dispatch_prepare' AND NOT EXISTS (SELECT 1 FROM hub_dispatch_changes prior WHERE prior.preparation_id=NEW.preparation_id)) OR (NEW.kind<>'dispatch_prepare' AND EXISTS (SELECT 1 FROM hub_dispatch_changes prior JOIN hub_stock_operations previous ON previous.operation_id=prior.operation_id WHERE prior.preparation_id=NEW.preparation_id AND prior.record_revision=NEW.expected_record_revision AND previous.hub_id=o.hub_id AND previous.stock_revision<o.stock_revision AND NOT EXISTS (SELECT 1 FROM hub_dispatch_changes newer WHERE newer.preparation_id=prior.preparation_id AND newer.record_revision>prior.record_revision) AND ((NEW.kind='dispatch_revise' AND prior.kind IN ('dispatch_prepare','dispatch_revise')) OR (NEW.kind='dispatch_issue' AND prior.kind IN ('dispatch_prepare','dispatch_revise') AND NEW.material_operation_id=prior.material_operation_id) OR (NEW.kind='dispatch_replace' AND prior.kind IN ('dispatch_issue','dispatch_replace') AND NEW.previous_identification_id=prior.identification_id) OR (NEW.kind='dispatch_unused' AND prior.kind IN ('dispatch_issue','dispatch_replace') AND NEW.previous_identification_id=prior.identification_id AND NEW.material_operation_id=prior.material_operation_id)))))) BEGIN SELECT RAISE(ABORT,'dispatch transition must extend its current preparation'); END;
CREATE TRIGGER hub_dispatch_identification_guard BEFORE INSERT ON hub_dispatch_identifications WHEN NOT EXISTS (SELECT 1 FROM hub_dispatch_changes c WHERE c.operation_id=NEW.issued_operation_id AND c.preparation_id=NEW.preparation_id AND c.identification_id=NEW.identification_id AND c.material_operation_id=NEW.material_operation_id AND ((c.kind='dispatch_issue' AND NEW.version_number=1 AND NEW.previous_identification_id IS NULL) OR (c.kind='dispatch_replace' AND c.previous_identification_id=NEW.previous_identification_id AND EXISTS (SELECT 1 FROM hub_dispatch_identifications prior WHERE prior.identification_id=NEW.previous_identification_id AND prior.preparation_id=NEW.preparation_id AND prior.version_number+1=NEW.version_number)))) BEGIN SELECT RAISE(ABORT,'issued identification must retain its exact linked version'); END;
CREATE TRIGGER hub_dispatch_material_no_update BEFORE UPDATE ON hub_dispatch_materials BEGIN SELECT RAISE(ABORT,'dispatch material is immutable'); END;
CREATE TRIGGER hub_dispatch_material_no_delete BEFORE DELETE ON hub_dispatch_materials BEGIN SELECT RAISE(ABORT,'dispatch material is retained'); END;
CREATE TRIGGER hub_dispatch_line_no_update BEFORE UPDATE ON hub_dispatch_material_lines BEGIN SELECT RAISE(ABORT,'dispatch line is immutable'); END;
CREATE TRIGGER hub_dispatch_line_no_delete BEFORE DELETE ON hub_dispatch_material_lines BEGIN SELECT RAISE(ABORT,'dispatch line is retained'); END;
CREATE TRIGGER hub_dispatch_identification_no_update BEFORE UPDATE ON hub_dispatch_identifications BEGIN SELECT RAISE(ABORT,'issued identification is immutable'); END;
CREATE TRIGGER hub_dispatch_identification_no_delete BEFORE DELETE ON hub_dispatch_identifications BEGIN SELECT RAISE(ABORT,'issued identification is retained'); END;
CREATE TRIGGER hub_dispatch_change_no_update BEFORE UPDATE ON hub_dispatch_changes BEGIN SELECT RAISE(ABORT,'dispatch change is immutable'); END;
CREATE TRIGGER hub_dispatch_change_no_delete BEFORE DELETE ON hub_dispatch_changes BEGIN SELECT RAISE(ABORT,'dispatch change is retained'); END;
