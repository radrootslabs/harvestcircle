CREATE TABLE hub_correction_observations (
    observation_id BLOB NOT NULL PRIMARY KEY CHECK (length(observation_id)=16 AND observation_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    previous_operation_id TEXT UNIQUE REFERENCES hub_correction_observations(operation_id),
    family TEXT NOT NULL CHECK (family IN ('found','count_classification')),
    target_id BLOB NOT NULL CHECK (length(target_id)=16 AND target_id<>zeroblob(16)),
    group_id BLOB CHECK (group_id IS NULL OR (length(group_id)=16 AND group_id<>zeroblob(16))),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    closure_events INTEGER NOT NULL CHECK ((family='found' AND closure_events BETWEEN 1 AND 64) OR (family='count_classification' AND closure_events=0)),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 262144),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    CHECK ((family='found' AND group_id IS NOT NULL) OR (family='count_classification' AND group_id IS NULL))
) STRICT;
CREATE TABLE hub_corrections (
    correction_id BLOB NOT NULL PRIMARY KEY CHECK (length(correction_id)=16 AND correction_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_stock_operations(operation_id),
    family TEXT NOT NULL CHECK (family IN ('source','recipient','additional','found','count_explain','count_classify','follow_up','document')),
    target_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('population','event','line','count','classification','document','follow_population','follow_source','follow_departure','follow_issued','follow_recipient','follow_unknown_source','follow_unknown_recipient')),
    subject_id BLOB NOT NULL CHECK (length(subject_id)=16 AND subject_id<>zeroblob(16)),
    previous_operation_id TEXT REFERENCES hub_corrections(operation_id),
    account_operation_id TEXT REFERENCES hub_stock_operations(operation_id),
    account_previous_operation_id TEXT REFERENCES hub_corrections(operation_id),
    report_id BLOB UNIQUE REFERENCES hub_actual_reports(report_id),
    observation_operation_id TEXT UNIQUE REFERENCES hub_correction_observations(operation_id),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 16777216),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32),
    CHECK ((family='source' AND subject_kind='population') OR (family='recipient' AND subject_kind='event') OR (family IN ('additional','found') AND subject_kind='line') OR (family='count_explain' AND subject_kind='count') OR (family='count_classify' AND subject_kind='classification') OR (family='document' AND subject_kind='document') OR (family='follow_up' AND subject_kind IN ('follow_population','follow_source','follow_departure','follow_issued','follow_recipient','follow_unknown_source','follow_unknown_recipient'))),
    CHECK ((family IN ('count_explain','count_classify') AND account_operation_id IS NOT NULL) OR (family NOT IN ('count_explain','count_classify') AND account_operation_id IS NULL AND account_previous_operation_id IS NULL)),
    CHECK ((family IN ('additional','count_explain') AND report_id IS NOT NULL AND observation_operation_id IS NULL) OR (family='found' AND report_id IS NULL AND observation_operation_id IS NOT NULL) OR (family='count_classify' AND observation_operation_id IS NOT NULL) OR (family IN ('source','recipient','follow_up','document') AND report_id IS NULL AND observation_operation_id IS NULL))
) STRICT;
CREATE INDEX hub_correction_target ON hub_corrections(target_operation_id,family,subject_kind,subject_id);
CREATE INDEX hub_correction_count_account ON hub_corrections(account_operation_id,operation_id);
CREATE TABLE hub_correction_scope_links (
    correction_operation_id TEXT NOT NULL REFERENCES hub_corrections(operation_id),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('population','source','departure','issued','recipient','unknown_source','unknown_recipient')),
    target_id BLOB NOT NULL CHECK (length(target_id)=16 AND target_id<>zeroblob(16)),
    introduced_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    PRIMARY KEY (correction_operation_id,target_kind,target_id)
) STRICT;
CREATE INDEX hub_correction_scope_target ON hub_correction_scope_links(target_kind,target_id,correction_operation_id);
CREATE TABLE hub_correction_physical_debits (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_corrections(operation_id),
    target_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    target_group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    variant_id BLOB NOT NULL CHECK (length(variant_id)=16 AND variant_id<>zeroblob(16)),
    before_group_revision INTEGER NOT NULL CHECK (before_group_revision>=1),
    after_group_revision INTEGER NOT NULL CHECK (after_group_revision=before_group_revision+1),
    before_packs INTEGER NOT NULL CHECK (before_packs BETWEEN 1 AND 1000000),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    after_packs INTEGER NOT NULL CHECK (after_packs BETWEEN 0 AND 1000000 AND after_packs=before_packs-packs),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32)
) STRICT;
CREATE INDEX hub_correction_debit_trace ON hub_correction_physical_debits(trace_id,group_id);
CREATE TABLE hub_correction_found_allocations (
    operation_id TEXT NOT NULL REFERENCES hub_corrections(operation_id),
    allocation INTEGER NOT NULL CHECK (allocation BETWEEN 0 AND 63),
    debit_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    debit_group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    debit_stock_revision INTEGER NOT NULL CHECK (debit_stock_revision>=2),
    child_group_id BLOB NOT NULL UNIQUE REFERENCES hub_stock_groups(group_id),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    birth_schema INTEGER NOT NULL CHECK (birth_schema=1),
    birth_payload BLOB NOT NULL CHECK (length(birth_payload) BETWEEN 1 AND 16384),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32),
    PRIMARY KEY (operation_id,allocation)
) STRICT;
CREATE INDEX hub_correction_found_debit ON hub_correction_found_allocations(debit_operation_id,debit_group_id);
CREATE TABLE hub_correction_initial_reviews (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_corrections(operation_id),
    record_id BLOB NOT NULL UNIQUE CHECK (length(record_id)=16 AND record_id<>zeroblob(16)),
    trace_id BLOB NOT NULL REFERENCES hub_receipts(trace_id),
    capture_stock_revision INTEGER NOT NULL CHECK (capture_stock_revision>=2),
    anchor_count INTEGER NOT NULL CHECK (anchor_count BETWEEN 1 AND 64),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 65536),
    result_hash BLOB NOT NULL CHECK (length(result_hash)=32)
) STRICT;
CREATE INDEX hub_correction_review_trace ON hub_correction_initial_reviews(trace_id,capture_stock_revision);
CREATE TABLE hub_correction_initial_review_anchors (
    operation_id TEXT NOT NULL REFERENCES hub_correction_initial_reviews(operation_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    created_revision INTEGER NOT NULL CHECK (created_revision>=2),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    PRIMARY KEY (operation_id,group_id)
) STRICT;
CREATE TABLE hub_correction_quantity_changes (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_corrections(operation_id),
    target_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    group_id BLOB NOT NULL REFERENCES hub_stock_groups(group_id),
    before_packs INTEGER NOT NULL CHECK (before_packs BETWEEN 0 AND 1000000),
    after_packs INTEGER NOT NULL CHECK (after_packs BETWEEN 0 AND 1000000 AND after_packs<>before_packs),
    delta_kind TEXT NOT NULL CHECK (delta_kind IN ('additional','found','classification')),
    delta_packs INTEGER NOT NULL CHECK (delta_packs BETWEEN 1 AND 1000000),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32),
    CHECK ((delta_kind='additional' AND after_packs=before_packs+delta_packs) OR (delta_kind='found' AND after_packs=before_packs-delta_packs) OR (delta_kind='classification' AND delta_packs=abs(after_packs-before_packs)))
) STRICT;
CREATE INDEX hub_correction_quantity_target ON hub_correction_quantity_changes(target_operation_id,group_id);
CREATE TABLE hub_correction_count_explanations (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_corrections(operation_id),
    count_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    reconciliation_id BLOB NOT NULL CHECK (length(reconciliation_id)=16 AND reconciliation_id<>zeroblob(16)),
    packs INTEGER NOT NULL CHECK (packs BETWEEN 1 AND 1000000),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32)
) STRICT;
CREATE INDEX hub_correction_explanation_count ON hub_correction_count_explanations(count_operation_id);
CREATE TABLE hub_correction_documents (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_corrections(operation_id),
    issued_version_id BLOB NOT NULL UNIQUE CHECK (length(issued_version_id)=16 AND issued_version_id<>zeroblob(16)),
    departure_operation_id TEXT NOT NULL REFERENCES hub_stock_operations(operation_id),
    replaces_version_id BLOB NOT NULL CHECK (length(replaces_version_id)=16 AND replaces_version_id<>zeroblob(16)),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32)
) STRICT;
CREATE INDEX hub_correction_document_departure ON hub_correction_documents(departure_operation_id);
CREATE TABLE hub_correction_captures (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_stock_operations(operation_id),
    kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 32 AND kind NOT GLOB '*[^a-z_]*'),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 16777216),
    scope_hash BLOB NOT NULL CHECK (length(scope_hash)=32)
) STRICT;
CREATE TABLE hub_correction_return_projections (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_returns(operation_id),
    payload_schema INTEGER NOT NULL CHECK (payload_schema=1),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 16384),
    projection_hash BLOB NOT NULL CHECK (length(projection_hash)=32)
) STRICT;
CREATE TRIGGER hub_correction_return_projection_guard BEFORE INSERT ON hub_correction_return_projections
WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations original JOIN hub_returns returned ON returned.operation_id=original.operation_id JOIN hub_submissions admission ON admission.operation_id=original.operation_id JOIN hub_correction_captures captured ON captured.operation_id=original.operation_id WHERE original.operation_id=NEW.operation_id AND original.kind='return' AND original.result_id=returned.return_id AND admission.kind=original.kind AND admission.hub_id=original.hub_id AND admission.actor_public_key=original.actor_public_key AND admission.domain_intent_hash=original.intent_hash AND captured.kind=original.kind AND captured.payload_schema=1)
BEGIN SELECT RAISE(ABORT,'return linkage projections require their exact native original, admission and captured allowance'); END;
CREATE TRIGGER hub_correction_return_projections_no_update BEFORE UPDATE ON hub_correction_return_projections BEGIN SELECT RAISE(ABORT,'return linkage projections are immutable'); END;
CREATE TRIGGER hub_correction_return_projections_no_delete BEFORE DELETE ON hub_correction_return_projections BEGIN SELECT RAISE(ABORT,'return linkage projections are retained'); END;
CREATE TRIGGER hub_correction_observation_guard BEFORE INSERT ON hub_correction_observations
WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_submissions s ON s.operation_id=o.operation_id WHERE o.operation_id=NEW.operation_id AND o.kind='correction_observe' AND o.result_id=NEW.observation_id AND s.kind=o.kind AND s.hub_id=o.hub_id AND s.actor_public_key=o.actor_public_key AND s.domain_intent_hash=o.intent_hash) OR (NEW.previous_operation_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM hub_correction_observations p JOIN hub_stock_operations old ON old.operation_id=p.operation_id JOIN hub_stock_operations current ON current.operation_id=NEW.operation_id WHERE p.operation_id=NEW.previous_operation_id AND p.family=NEW.family AND old.hub_id=current.hub_id AND old.stock_revision<current.stock_revision AND NOT EXISTS (SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=p.operation_id)))
BEGIN SELECT RAISE(ABORT,'correction observations require their own immutable admission and an unused predecessor'); END;
CREATE TRIGGER hub_correction_record_guard BEFORE INSERT ON hub_corrections
WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_submissions s ON s.operation_id=o.operation_id JOIN hub_stock_operations target ON target.operation_id=NEW.target_operation_id WHERE o.operation_id=NEW.operation_id AND o.result_id=NEW.correction_id AND o.kind=CASE NEW.family WHEN 'source' THEN 'correction_source' WHEN 'recipient' THEN 'correction_recipient' WHEN 'additional' THEN 'correction_additional' WHEN 'found' THEN 'correction_found' WHEN 'count_explain' THEN 'correction_count_explain' WHEN 'count_classify' THEN 'correction_count_classify' WHEN 'follow_up' THEN 'correction_follow_up' WHEN 'document' THEN 'correction_document' END AND s.kind=o.kind AND s.hub_id=o.hub_id AND s.actor_public_key=o.actor_public_key AND s.domain_intent_hash=o.intent_hash AND target.hub_id=o.hub_id AND target.stock_revision<o.stock_revision) OR (NEW.previous_operation_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM hub_corrections p JOIN hub_stock_operations old ON old.operation_id=p.operation_id JOIN hub_stock_operations current ON current.operation_id=NEW.operation_id WHERE p.operation_id=NEW.previous_operation_id AND (p.family=NEW.family OR (p.family IN ('additional','found') AND NEW.family IN ('additional','found'))) AND p.target_operation_id=NEW.target_operation_id AND p.subject_kind=NEW.subject_kind AND p.subject_id=NEW.subject_id AND old.hub_id=current.hub_id AND old.stock_revision<current.stock_revision)) OR (NEW.report_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM hub_actual_reports report JOIN hub_stock_operations original ON original.operation_id=report.operation_id JOIN hub_stock_operations current ON current.operation_id=NEW.operation_id JOIN hub_actual_report_reviews marker ON marker.report_id=report.report_id WHERE report.report_id=NEW.report_id AND original.hub_id=current.hub_id AND original.stock_revision<current.stock_revision AND marker.review_operation_id IS NULL)) OR (NEW.observation_operation_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM hub_correction_observations observation JOIN hub_stock_operations original ON original.operation_id=observation.operation_id JOIN hub_stock_operations current ON current.operation_id=NEW.operation_id WHERE observation.operation_id=NEW.observation_operation_id AND observation.family=CASE NEW.family WHEN 'found' THEN 'found' WHEN 'count_classify' THEN 'count_classification' END AND lower(hex(observation.target_id))=replace(NEW.target_operation_id,'-','') AND (NEW.family<>'found' OR observation.group_id=NEW.subject_id) AND original.hub_id=current.hub_id AND original.stock_revision<current.stock_revision AND NOT EXISTS (SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=observation.operation_id)))
BEGIN SELECT RAISE(ABORT,'correction records require exact native admission, original targets and unresolved factual originals'); END;
CREATE TRIGGER hub_correction_scope_guard BEFORE INSERT ON hub_correction_scope_links
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c JOIN hub_stock_operations original ON original.operation_id=c.operation_id JOIN hub_stock_operations introduced ON introduced.operation_id=NEW.introduced_operation_id WHERE c.operation_id=NEW.correction_operation_id AND original.hub_id=introduced.hub_id AND introduced.stock_revision>=original.stock_revision AND (introduced.operation_id=original.operation_id OR (NEW.target_kind='issued' AND introduced.kind IN ('dispatch_issue','dispatch_replace','correction_document'))))
BEGIN SELECT RAISE(ABORT,'late scope growth is a separately proved linked issued version'); END;
CREATE TRIGGER hub_correction_debit_guard BEFORE INSERT ON hub_correction_physical_debits
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c JOIN hub_stock_operations o ON o.operation_id=c.operation_id JOIN hub_stock_groups g ON g.group_id=NEW.group_id WHERE c.operation_id=NEW.operation_id AND c.family='additional' AND c.target_operation_id=NEW.target_operation_id AND c.subject_id=NEW.target_group_id AND g.trace_id=NEW.trace_id)
BEGIN SELECT RAISE(ABORT,'only an additional-quantity correction has a new physical debit'); END;
CREATE TRIGGER hub_correction_found_guard BEFORE INSERT ON hub_correction_found_allocations
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c JOIN hub_stock_operations current ON current.operation_id=c.operation_id JOIN hub_stock_operations debit ON debit.operation_id=NEW.debit_operation_id JOIN hub_stock_groups parent ON parent.group_id=NEW.debit_group_id JOIN hub_stock_groups child ON child.group_id=NEW.child_group_id WHERE c.operation_id=NEW.operation_id AND c.family='found' AND debit.hub_id=current.hub_id AND debit.stock_revision=NEW.debit_stock_revision AND debit.stock_revision<current.stock_revision AND debit.kind IN ('departure','actual_review','correction_additional') AND parent.trace_id=child.trace_id AND child.revision=1
    AND EXISTS (SELECT 1 FROM hub_group_identification birth WHERE birth.group_id=child.group_id AND birth.parent_group_id=parent.group_id AND birth.created_operation_id=current.operation_id)
    AND (
        (debit.kind='departure' AND EXISTS (SELECT 1 FROM hub_departure_lines line WHERE line.operation_id=debit.operation_id AND line.group_id=parent.group_id AND line.trace_id=parent.trace_id AND line.packs>=NEW.packs)) OR
        (debit.kind='actual_review' AND EXISTS (SELECT 1 FROM hub_actual_reviews review WHERE review.operation_id=debit.operation_id AND review.review_kind='departure' AND review.group_id=parent.group_id AND review.trace_id=parent.trace_id AND review.packs>=NEW.packs)) OR
        (debit.kind='correction_additional' AND EXISTS (SELECT 1 FROM hub_correction_physical_debits additional WHERE additional.operation_id=debit.operation_id AND additional.group_id=parent.group_id AND additional.trace_id=parent.trace_id AND additional.packs>=NEW.packs))
    ))
BEGIN SELECT RAISE(ABORT,'found groups retain an earlier real debit and their separate native birth'); END;
CREATE TRIGGER hub_correction_quantity_guard BEFORE INSERT ON hub_correction_quantity_changes
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c WHERE c.operation_id=NEW.operation_id AND c.target_operation_id=NEW.target_operation_id AND c.subject_id=CASE WHEN c.family='count_classify' THEN c.subject_id ELSE NEW.group_id END AND ((c.family='additional' AND NEW.delta_kind='additional') OR (c.family='found' AND NEW.delta_kind='found') OR (c.family='count_classify' AND NEW.delta_kind='classification')))
BEGIN SELECT RAISE(ABORT,'quantity changes retain their exact specialized family and target'); END;
CREATE TRIGGER hub_correction_explanation_guard BEFORE INSERT ON hub_correction_count_explanations
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c JOIN hub_stock_operations count ON count.operation_id=NEW.count_operation_id WHERE c.operation_id=NEW.operation_id AND c.family='count_explain' AND c.target_operation_id=count.operation_id AND count.kind='count_post' AND c.subject_id=NEW.reconciliation_id)
BEGIN SELECT RAISE(ABORT,'count explanation classifies its original count rather than another physical debit'); END;
CREATE TRIGGER hub_correction_document_guard BEFORE INSERT ON hub_correction_documents
WHEN NOT EXISTS (SELECT 1 FROM hub_corrections c JOIN hub_stock_operations departure ON departure.operation_id=NEW.departure_operation_id WHERE c.operation_id=NEW.operation_id AND c.family='document' AND c.target_operation_id=departure.operation_id AND departure.kind='departure')
BEGIN SELECT RAISE(ABORT,'a linked identification replacement is not a physical departure'); END;
CREATE TRIGGER hub_correction_capture_guard BEFORE INSERT ON hub_correction_captures
WHEN NOT EXISTS (SELECT 1 FROM hub_stock_operations original JOIN hub_submissions admission ON admission.operation_id=original.operation_id WHERE original.operation_id=NEW.operation_id AND original.kind=NEW.kind AND admission.kind=original.kind AND admission.hub_id=original.hub_id AND admission.actor_public_key=original.actor_public_key AND admission.domain_intent_hash=original.intent_hash)
BEGIN SELECT RAISE(ABORT,'frozen correction context belongs to its exact native original and admission'); END;
CREATE TRIGGER hub_correction_observations_no_update BEFORE UPDATE ON hub_correction_observations BEGIN SELECT RAISE(ABORT,'observed correction facts are immutable'); END;
CREATE TRIGGER hub_correction_observations_no_delete BEFORE DELETE ON hub_correction_observations BEGIN SELECT RAISE(ABORT,'observed correction facts are retained'); END;
CREATE TRIGGER hub_corrections_no_update BEFORE UPDATE ON hub_corrections BEGIN SELECT RAISE(ABORT,'correction originals are immutable'); END;
CREATE TRIGGER hub_corrections_no_delete BEFORE DELETE ON hub_corrections BEGIN SELECT RAISE(ABORT,'correction originals are retained'); END;
CREATE TRIGGER hub_correction_scope_no_update BEFORE UPDATE ON hub_correction_scope_links BEGIN SELECT RAISE(ABORT,'correction scope links are immutable'); END;
CREATE TRIGGER hub_correction_scope_no_delete BEFORE DELETE ON hub_correction_scope_links BEGIN SELECT RAISE(ABORT,'correction scope links are retained'); END;
CREATE TRIGGER hub_correction_debits_no_update BEFORE UPDATE ON hub_correction_physical_debits BEGIN SELECT RAISE(ABORT,'physical debit originals are immutable'); END;
CREATE TRIGGER hub_correction_debits_no_delete BEFORE DELETE ON hub_correction_physical_debits BEGIN SELECT RAISE(ABORT,'physical debit originals are retained'); END;
CREATE TRIGGER hub_correction_found_no_update BEFORE UPDATE ON hub_correction_found_allocations BEGIN SELECT RAISE(ABORT,'found allocations are immutable'); END;
CREATE TRIGGER hub_correction_found_no_delete BEFORE DELETE ON hub_correction_found_allocations BEGIN SELECT RAISE(ABORT,'found allocations are retained'); END;
CREATE TRIGGER hub_correction_quantity_no_update BEFORE UPDATE ON hub_correction_quantity_changes BEGIN SELECT RAISE(ABORT,'quantity amendments are immutable'); END;
CREATE TRIGGER hub_correction_quantity_no_delete BEFORE DELETE ON hub_correction_quantity_changes BEGIN SELECT RAISE(ABORT,'quantity amendments are retained'); END;
CREATE TRIGGER hub_correction_explanations_no_update BEFORE UPDATE ON hub_correction_count_explanations BEGIN SELECT RAISE(ABORT,'known count explanations are immutable'); END;
CREATE TRIGGER hub_correction_explanations_no_delete BEFORE DELETE ON hub_correction_count_explanations BEGIN SELECT RAISE(ABORT,'known count explanations are retained'); END;
CREATE TRIGGER hub_correction_documents_no_update BEFORE UPDATE ON hub_correction_documents BEGIN SELECT RAISE(ABORT,'as-issued replacement versions are immutable'); END;
CREATE TRIGGER hub_correction_documents_no_delete BEFORE DELETE ON hub_correction_documents BEGIN SELECT RAISE(ABORT,'as-issued replacement versions are retained'); END;
CREATE TRIGGER hub_correction_captures_no_update BEFORE UPDATE ON hub_correction_captures BEGIN SELECT RAISE(ABORT,'captured correction scope is immutable'); END;
CREATE TRIGGER hub_correction_captures_no_delete BEFORE DELETE ON hub_correction_captures BEGIN SELECT RAISE(ABORT,'captured correction scope is retained'); END;
CREATE TRIGGER hub_correction_exact_head_guard BEFORE INSERT ON hub_corrections
WHEN NEW.previous_operation_id IS NOT (
    SELECT previous.operation_id FROM hub_corrections previous
    JOIN hub_stock_operations original ON original.operation_id=previous.operation_id
    WHERE previous.target_operation_id=NEW.target_operation_id
        AND previous.subject_kind=NEW.subject_kind AND previous.subject_id=NEW.subject_id
        AND (previous.family=NEW.family OR (previous.family IN ('additional','found') AND NEW.family IN ('additional','found')))
    ORDER BY original.stock_revision DESC LIMIT 1
)
BEGIN SELECT RAISE(ABORT,'correction predecessor must be its exact current head'); END;
CREATE TRIGGER hub_correction_count_account_guard BEFORE INSERT ON hub_corrections
WHEN NEW.family IN ('count_explain','count_classify') AND (
    NOT EXISTS (SELECT 1 FROM hub_stock_operations account WHERE account.operation_id=NEW.account_operation_id AND account.kind='count_post')
    OR (NEW.family='count_explain' AND NEW.target_operation_id<>NEW.account_operation_id)
    OR (NEW.family='count_classify' AND NOT EXISTS (
        SELECT 1 FROM hub_corrections explanation
        WHERE explanation.operation_id=NEW.target_operation_id AND explanation.family='count_explain'
            AND explanation.account_operation_id=NEW.account_operation_id
    ))
    OR NEW.account_previous_operation_id IS NOT (
        SELECT previous.operation_id FROM hub_corrections previous
        JOIN hub_stock_operations original ON original.operation_id=previous.operation_id
        WHERE previous.account_operation_id=NEW.account_operation_id
        ORDER BY original.stock_revision DESC LIMIT 1
    )
)
BEGIN SELECT RAISE(ABORT,'count-account head is shared across explanations and classification amendments'); END;
CREATE TRIGGER hub_correction_observed_stock_guard BEFORE INSERT ON hub_stock_operations
WHEN NEW.kind IN ('receipt','opening','move','split','work_new','work_replan','dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','departure','count_post','return','disposition','correction_document') AND EXISTS (
    SELECT 1 FROM hub_correction_observations observation
    WHERE NOT EXISTS (SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=observation.operation_id)
        AND NOT EXISTS (SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=observation.operation_id)
)
BEGIN SELECT RAISE(ABORT,'planned effects require review of retained correction observations'); END;
CREATE TRIGGER hub_correction_initial_review_guard BEFORE INSERT ON hub_correction_initial_reviews
WHEN NOT EXISTS (
    SELECT 1 FROM hub_corrections correction JOIN hub_stock_operations original ON original.operation_id=correction.operation_id
    WHERE correction.operation_id=NEW.operation_id AND correction.family='found' AND original.stock_revision=NEW.capture_stock_revision
        AND NEW.anchor_count=(SELECT count(*) FROM hub_correction_found_allocations allocation WHERE allocation.operation_id=original.operation_id)
        AND NEW.anchor_count=(SELECT count(*) FROM hub_group_identification birth WHERE birth.created_operation_id=original.operation_id)
        AND NOT EXISTS (SELECT 1 FROM hub_correction_found_allocations allocation JOIN hub_stock_groups child ON child.group_id=allocation.child_group_id WHERE allocation.operation_id=original.operation_id AND child.trace_id<>NEW.trace_id)
        AND NOT EXISTS (SELECT 1 FROM hub_treatment_records ordinary WHERE ordinary.record_id=NEW.record_id)
)
BEGIN SELECT RAISE(ABORT,'initial review covers every separately born found group at its own birth cut'); END;
CREATE TRIGGER hub_correction_initial_anchor_guard BEFORE INSERT ON hub_correction_initial_review_anchors
WHEN NOT EXISTS (
    SELECT 1 FROM hub_correction_initial_reviews review
    JOIN hub_correction_found_allocations allocation ON allocation.operation_id=review.operation_id AND allocation.child_group_id=NEW.group_id
    JOIN hub_group_identification birth ON birth.group_id=allocation.child_group_id
    JOIN hub_stock_operations original ON original.operation_id=birth.created_operation_id
    WHERE review.operation_id=NEW.operation_id AND review.capture_stock_revision=NEW.created_revision
        AND original.operation_id=review.operation_id AND original.stock_revision=NEW.created_revision AND allocation.packs=NEW.packs
)
BEGIN SELECT RAISE(ABORT,'initial-review anchors are the exact original newborn quantities'); END;
CREATE TRIGGER hub_correction_found_closed_guard BEFORE INSERT ON hub_correction_found_allocations
WHEN EXISTS (SELECT 1 FROM hub_correction_initial_reviews review WHERE review.operation_id=NEW.operation_id)
BEGIN SELECT RAISE(ABORT,'found allocations cannot grow after their complete initial review'); END;
CREATE TRIGGER hub_correction_initial_reviews_no_update BEFORE UPDATE ON hub_correction_initial_reviews BEGIN SELECT RAISE(ABORT,'initial reviews are immutable'); END;
CREATE TRIGGER hub_correction_initial_reviews_no_delete BEFORE DELETE ON hub_correction_initial_reviews BEGIN SELECT RAISE(ABORT,'initial reviews are retained'); END;
CREATE TRIGGER hub_correction_initial_anchors_no_update BEFORE UPDATE ON hub_correction_initial_review_anchors BEGIN SELECT RAISE(ABORT,'initial-review anchors are immutable'); END;
CREATE TRIGGER hub_correction_initial_anchors_no_delete BEFORE DELETE ON hub_correction_initial_review_anchors BEGIN SELECT RAISE(ABORT,'initial-review anchors are retained'); END;
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
        OR
        (NEW.parent_group_id IS NOT NULL AND o.kind='correction_found' AND NEW.physical_label IS NOT NULL AND EXISTS (
            SELECT 1 FROM hub_corrections correction
            JOIN hub_group_identification parent ON parent.group_id=NEW.parent_group_id
            JOIN hub_stock_groups pg ON pg.group_id=parent.group_id
            JOIN hub_stock_operations po ON po.operation_id=parent.created_operation_id
            WHERE correction.operation_id=o.operation_id AND correction.family='found' AND correction.correction_id=o.result_id
                AND pg.trace_id=g.trace_id AND po.hub_id=o.hub_id AND po.stock_revision<o.stock_revision
                AND NOT EXISTS (SELECT 1 FROM hub_correction_initial_reviews review WHERE review.operation_id=o.operation_id)
        ))
    )
)
BEGIN SELECT RAISE(ABORT,'physical group birth must retain its original population'); END;
DROP TRIGGER hub_actual_report_review_update_guard;
CREATE TRIGGER hub_actual_report_review_update_guard BEFORE UPDATE ON hub_actual_report_reviews
WHEN NEW.report_id<>OLD.report_id OR NEW.operation_id<>OLD.operation_id OR OLD.review_operation_id IS NOT NULL OR NEW.review_operation_id IS NULL OR NOT (
    EXISTS (SELECT 1 FROM hub_stock_operations o JOIN hub_actual_reviews review ON review.operation_id=o.operation_id WHERE o.operation_id=NEW.review_operation_id AND o.kind='actual_review' AND review.report_id=OLD.report_id)
    OR EXISTS (
        SELECT 1 FROM hub_corrections correction
        JOIN hub_stock_operations current ON current.operation_id=correction.operation_id
        JOIN hub_actual_reports report ON report.report_id=correction.report_id
        JOIN hub_stock_operations original ON original.operation_id=report.operation_id
        WHERE correction.operation_id=NEW.review_operation_id AND correction.report_id=OLD.report_id AND report.operation_id=OLD.operation_id
            AND current.hub_id=original.hub_id AND current.stock_revision>original.stock_revision AND current.result_id=correction.correction_id
            AND ((correction.family='additional' AND current.kind='correction_additional')
                OR (correction.family='count_explain' AND current.kind='correction_count_explain')
                OR (correction.family='count_classify' AND current.kind='correction_count_classify'))
    )
)
BEGIN SELECT RAISE(ABORT,'actual review closure binds the exact immutable owner review'); END;
