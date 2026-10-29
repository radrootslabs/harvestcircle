CREATE TABLE hub_runtime_session (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton=1),
    runtime_id BLOB NOT NULL CHECK (length(runtime_id)=16 AND runtime_id<>zeroblob(16)),
    generation BLOB NOT NULL CHECK (length(generation)=8),
    actor_public_key BLOB CHECK (actor_public_key IS NULL OR length(actor_public_key)=32),
    updated_at_unix_s INTEGER NOT NULL CHECK (updated_at_unix_s>=0)
) STRICT;
CREATE TABLE hub_submissions (
    operation_id TEXT NOT NULL PRIMARY KEY CHECK (length(CAST(operation_id AS BLOB))=36 AND operation_id=lower(operation_id) AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1)='-' AND substr(operation_id,14,1)='-' AND substr(operation_id,15,1)='7' AND substr(operation_id,19,1)='-' AND substr(operation_id,20,1) IN ('8','9','a','b') AND substr(operation_id,24,1)='-'),
    hub_id BLOB NOT NULL CHECK (length(hub_id)=16 AND hub_id<>zeroblob(16)),
    actor_public_key BLOB NOT NULL CHECK (length(actor_public_key)=32),
    kind TEXT NOT NULL CHECK (length(CAST(kind AS BLOB)) BETWEEN 1 AND 32 AND kind NOT GLOB '*[^a-z_]*'),
    runtime_id BLOB NOT NULL CHECK (length(runtime_id)=16 AND runtime_id<>zeroblob(16)),
    generation BLOB NOT NULL CHECK (length(generation)=8),
    input_hash BLOB NOT NULL CHECK (length(input_hash)=32),
    domain_intent_hash BLOB CHECK (domain_intent_hash IS NULL OR length(domain_intent_hash)=32),
    capture_schema INTEGER NOT NULL CHECK (capture_schema=1),
    capture_body BLOB NOT NULL CHECK (length(capture_body) BETWEEN 1 AND 16777216),
    capture_hash BLOB NOT NULL CHECK (length(capture_hash)=32),
    submitted_at_unix_s INTEGER NOT NULL CHECK (submitted_at_unix_s>=0),
    CHECK ((kind='initialize' AND domain_intent_hash IS NULL) OR (kind<>'initialize' AND domain_intent_hash IS NOT NULL))
) STRICT;
CREATE INDEX hub_submission_runtime ON hub_submissions(runtime_id,generation);
CREATE TABLE hub_submission_rejections (
    operation_id TEXT NOT NULL PRIMARY KEY REFERENCES hub_submissions(operation_id),
    code TEXT NOT NULL CHECK (code IN ('conflict','invalid_input','capacity','permission_denied','storage_unavailable','storage_corrupt','internal')),
    recorded_at_unix_s INTEGER NOT NULL CHECK (recorded_at_unix_s>=0)
) STRICT;
CREATE TABLE hub_actual_report_reviews (
    report_id BLOB NOT NULL PRIMARY KEY CHECK (length(report_id)=16 AND report_id<>zeroblob(16)),
    operation_id TEXT NOT NULL UNIQUE REFERENCES hub_submissions(operation_id),
    review_operation_id TEXT UNIQUE REFERENCES hub_stock_operations(operation_id)
) STRICT;
CREATE TRIGGER hub_runtime_session_insert_guard BEFORE INSERT ON hub_runtime_session
WHEN NEW.generation<>zeroblob(8) OR NEW.actor_public_key IS NOT NULL OR EXISTS (SELECT 1 FROM hub_submissions s WHERE s.runtime_id=NEW.runtime_id)
BEGIN SELECT RAISE(ABORT,'a runtime starts signed out with a fresh incarnation'); END;
CREATE TRIGGER hub_runtime_session_update_guard BEFORE UPDATE ON hub_runtime_session
WHEN NEW.updated_at_unix_s<OLD.updated_at_unix_s OR (NEW.runtime_id=OLD.runtime_id AND NEW.generation<=OLD.generation) OR (NEW.runtime_id<>OLD.runtime_id AND (NEW.generation<>zeroblob(8) OR NEW.actor_public_key IS NOT NULL OR EXISTS (SELECT 1 FROM hub_submissions s WHERE s.runtime_id=NEW.runtime_id)))
BEGIN SELECT RAISE(ABORT,'runtime session replacement must advance its native context'); END;
CREATE TRIGGER hub_runtime_session_no_delete BEFORE DELETE ON hub_runtime_session BEGIN SELECT RAISE(ABORT,'runtime fencing must be replaced rather than removed'); END;
CREATE TRIGGER hub_submission_admission_guard BEFORE INSERT ON hub_submissions
WHEN NOT ((NEW.kind='initialize' AND NOT EXISTS (SELECT 1 FROM hub_context)) OR EXISTS (SELECT 1 FROM hub_context h WHERE h.hub_id=NEW.hub_id AND h.owner_public_key=NEW.actor_public_key)) OR NOT EXISTS (SELECT 1 FROM hub_runtime_session r WHERE r.singleton=1 AND r.runtime_id=NEW.runtime_id AND r.generation=NEW.generation AND r.actor_public_key=NEW.actor_public_key) OR EXISTS (SELECT 1 FROM hub_history h WHERE h.operation_id=NEW.operation_id) OR EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.operation_id=NEW.operation_id) OR EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=NEW.operation_id)
BEGIN SELECT RAISE(ABORT,'submission admission requires a current native session and a new operation'); END;
CREATE TRIGGER hub_submissions_no_update BEFORE UPDATE ON hub_submissions BEGIN SELECT RAISE(ABORT,'submitted semantics are immutable'); END;
CREATE TRIGGER hub_submissions_no_delete BEFORE DELETE ON hub_submissions BEGIN SELECT RAISE(ABORT,'submitted semantics are retained'); END;
CREATE TRIGGER hub_submission_rejection_guard BEFORE INSERT ON hub_submission_rejections
WHEN NOT EXISTS (SELECT 1 FROM hub_submissions s WHERE s.operation_id=NEW.operation_id AND NEW.recorded_at_unix_s>=s.submitted_at_unix_s) OR EXISTS (SELECT 1 FROM hub_history h WHERE h.operation_id=NEW.operation_id) OR EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.operation_id=NEW.operation_id) OR EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=NEW.operation_id)
BEGIN SELECT RAISE(ABORT,'a known rejection cannot replace an original recorded operation'); END;
CREATE TRIGGER hub_submission_rejections_no_update BEFORE UPDATE ON hub_submission_rejections BEGIN SELECT RAISE(ABORT,'original rejection is immutable'); END;
CREATE TRIGGER hub_submission_rejections_no_delete BEFORE DELETE ON hub_submission_rejections BEGIN SELECT RAISE(ABORT,'original rejection is retained'); END;
CREATE TRIGGER hub_submission_stock_effect_guard BEFORE INSERT ON hub_stock_operations
WHEN EXISTS (SELECT 1 FROM hub_runtime_session) AND NOT EXISTS (
    SELECT 1 FROM hub_submissions s JOIN hub_runtime_session r ON r.singleton=1
    WHERE s.operation_id=NEW.operation_id AND s.hub_id=NEW.hub_id AND s.actor_public_key=NEW.actor_public_key AND s.kind=NEW.kind AND s.domain_intent_hash=NEW.intent_hash
    AND r.runtime_id=s.runtime_id AND r.generation=s.generation AND r.actor_public_key=s.actor_public_key
    AND NEW.recorded_at_unix_s>=s.submitted_at_unix_s AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections rejected WHERE rejected.operation_id=s.operation_id)
)
BEGIN SELECT RAISE(ABORT,'stock effects require their original current-session admission'); END;
CREATE TRIGGER hub_submission_reference_effect_guard BEFORE INSERT ON hub_reference_versions
WHEN EXISTS (SELECT 1 FROM hub_runtime_session) AND NOT EXISTS (
    SELECT 1 FROM hub_submissions s JOIN hub_runtime_session r ON r.singleton=1
    WHERE s.operation_id=NEW.operation_id AND s.hub_id=NEW.hub_id AND s.actor_public_key=NEW.actor_public_key AND s.kind='reference' AND s.domain_intent_hash=NEW.intent_hash
    AND r.runtime_id=s.runtime_id AND r.generation=s.generation AND r.actor_public_key=s.actor_public_key
    AND NEW.recorded_at_unix_s>=s.submitted_at_unix_s AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections rejected WHERE rejected.operation_id=s.operation_id)
)
BEGIN SELECT RAISE(ABORT,'reference effects require their original current-session admission'); END;
CREATE TRIGGER hub_submission_initialization_effect_guard BEFORE INSERT ON hub_history
WHEN NEW.event_kind='initialized' AND EXISTS (SELECT 1 FROM hub_runtime_session) AND NOT EXISTS (
    SELECT 1 FROM hub_submissions s JOIN hub_runtime_session r ON r.singleton=1
    WHERE s.operation_id=NEW.operation_id AND s.hub_id=NEW.hub_id AND s.actor_public_key=NEW.actor_public_key AND s.kind='initialize'
    AND r.runtime_id=s.runtime_id AND r.generation=s.generation AND r.actor_public_key=s.actor_public_key
    AND NEW.recorded_at_unix_s>=s.submitted_at_unix_s AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections rejected WHERE rejected.operation_id=s.operation_id)
)
BEGIN SELECT RAISE(ABORT,'initialization requires its original current-session admission'); END;
CREATE TRIGGER hub_actual_report_review_insert_guard BEFORE INSERT ON hub_actual_report_reviews
WHEN NEW.review_operation_id IS NOT NULL OR NOT EXISTS (SELECT 1 FROM hub_submissions s WHERE s.operation_id=NEW.operation_id AND s.kind='actual_report')
BEGIN SELECT RAISE(ABORT,'an actual report starts with unresolved native review'); END;
CREATE TRIGGER hub_actual_report_review_update_guard BEFORE UPDATE ON hub_actual_report_reviews
WHEN NEW.report_id<>OLD.report_id OR NEW.operation_id<>OLD.operation_id OR OLD.review_operation_id IS NOT NULL OR NEW.review_operation_id IS NULL OR NOT EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=NEW.review_operation_id AND o.kind='actual_review')
BEGIN SELECT RAISE(ABORT,'actual review retains its original report and native review operation'); END;
CREATE TRIGGER hub_actual_report_reviews_no_delete BEFORE DELETE ON hub_actual_report_reviews BEGIN SELECT RAISE(ABORT,'actual reports and their review are retained'); END;
CREATE TRIGGER hub_submission_uncertain_stock_guard BEFORE INSERT ON hub_stock_operations
WHEN NEW.kind IN ('receipt','opening','move','split','work_new','work_replan','dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','departure','count_post') AND (
    EXISTS (SELECT 1 FROM hub_actual_report_reviews WHERE review_operation_id IS NULL) OR EXISTS (
        SELECT 1 FROM hub_submissions s WHERE s.operation_id<>NEW.operation_id
        AND NOT EXISTS (SELECT 1 FROM hub_submission_rejections r WHERE r.operation_id=s.operation_id)
        AND NOT EXISTS (SELECT 1 FROM hub_history h WHERE h.operation_id=s.operation_id AND s.kind='initialize' AND h.event_kind='initialized' AND h.hub_id=s.hub_id AND h.actor_public_key=s.actor_public_key AND h.recorded_at_unix_s>=s.submitted_at_unix_s)
        AND NOT EXISTS (SELECT 1 FROM hub_reference_versions v WHERE v.operation_id=s.operation_id AND s.kind='reference' AND v.hub_id=s.hub_id AND v.actor_public_key=s.actor_public_key AND v.intent_hash=s.domain_intent_hash AND v.recorded_at_unix_s>=s.submitted_at_unix_s)
        AND NOT EXISTS (SELECT 1 FROM hub_stock_operations o WHERE o.operation_id=s.operation_id AND o.hub_id=s.hub_id AND o.actor_public_key=s.actor_public_key AND o.kind=s.kind AND o.intent_hash=s.domain_intent_hash AND o.recorded_at_unix_s>=s.submitted_at_unix_s)
    )
)
BEGIN SELECT RAISE(ABORT,'planned stock effects require prior outcomes and actual reports to be reviewed'); END;
