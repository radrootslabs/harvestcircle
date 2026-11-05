//! Exact capacity boundary with valid sealed, closed duplicate originals.
//! Bulk fixture construction is not production admission or replay permission.
use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::{
    ArchivedHubSubmission, HubMutation, HubSubmissionContext, HubSubmissionScope,
    HubSubmittedActualLink, HubSubmittedMaterial, HubSubmittedRequest,
};
use harvestcircle_domain::hub::{
    HubId, HubOwnership,
    actuals::{ActualDuplicateLink, actual_duplicate_result_digest, actual_report_result_digest},
};
use sqlx::{Row, SqliteConnection};

async fn admission_row(
    raw: &mut SqliteConnection,
    id: &DurableRequestId,
    scope: HubSubmissionScope,
    hub: HubId,
    hash: &[u8; 32],
    archive: &ArchivedHubSubmission,
) {
    sqlx::query("INSERT INTO hub_submissions (operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) VALUES (?,?,?,?,?,?,?,?,1,?,?,150)")
        .bind(id.as_str()).bind(hub.as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(archive.kind())
        .bind(scope.runtime.as_bytes().as_slice()).bind(scope.generation.value().to_be_bytes().as_slice())
        .bind(archive.input_digest().as_slice()).bind(hash.as_slice()).bind(archive.as_bytes()).bind(archive.digest().as_slice())
        .execute(raw).await.unwrap();
}
async fn operation_row(
    raw: &mut SqliteConnection,
    id: &DurableRequestId,
    hub: HubId,
    hash: &[u8; 32],
    catalog: HubRevision,
    revision: HubRevision,
    result: HubRecordId,
) {
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,'actual_report',?,?,?,?,150)")
        .bind(id.as_str()).bind(hub.as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(hash.as_slice())
        .bind(i64::try_from(catalog.value()).unwrap()).bind(i64::try_from(revision.value()).unwrap()).bind(result.as_bytes().as_slice())
        .execute(raw).await.unwrap();
}

#[tokio::test]
async fn four_thousand_ninety_seventh_fact_stays_pending_after_all_completed_reports_are_closed() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let command = review(&db, saved.original.id, intake.original.position.group).await;
    let crate::HubMutationResult::ActualReviewed(original) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("review")
    };
    let HubActualReviewOriginal::Departure(original) = &original.original else {
        panic!("physical")
    };
    let known = original.physical_projection().unwrap();
    let before = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let hub = HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), owner());
    let catalog = before.catalog_revision;
    let mut revision = before.revision;
    let mut meaning = db
        .hub_actual_known_interpretations(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace: known.trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap()
        .interpretations
        .records
        .pop()
        .unwrap();

    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .unwrap();
    let input = report();
    for n in 0..4095 {
        let id = operation(100000 + n);
        let op = HubRecordId::from_hex(&id.as_str().replace('-', "")).unwrap();
        let record = input
            .preserve(hub.id(), revision, catalog, op, owner(), at(150))
            .unwrap();
        let hash = input.fingerprint(hub.id(), owner(), op);
        let seal = actual_report_result_digest(&hash, &record).unwrap();
        let archive = HubSubmittedRequest::ActualReport(&input)
            .capture(
                &id,
                scope,
                &HubSubmissionContext {
                    hub,
                    catalog_revision: Some(catalog),
                    stock_revision: Some(revision),
                    material: &[],
                },
            )
            .unwrap();
        admission_row(&mut raw, &id, scope, hub.id(), &hash, &archive).await;
        operation_row(
            &mut raw,
            &id,
            hub.id(),
            &hash,
            catalog,
            record.stock_revision,
            record.id,
        )
        .await;
        sqlx::query("INSERT INTO hub_actual_reports (report_id,operation_id,policy_version,packs,source_kind,source_count,occurrence_kind,observation,result_hash) VALUES (?,?,1,2,'unknown',0,'unknown',?,?)")
            .bind(record.id.as_bytes().as_slice()).bind(id.as_str()).bind(input.observation.as_str()).bind(seal.as_slice()).execute(&mut raw).await.unwrap();
        sqlx::query("INSERT INTO hub_actual_report_reviews (report_id,operation_id,review_operation_id) VALUES (?,?,NULL)")
            .bind(record.id.as_bytes().as_slice()).bind(id.as_str()).execute(&mut raw).await.unwrap();
        revision = record.stock_revision;
        let id = operation(200000 + n);
        let op = HubRecordId::from_hex(&id.as_str().replace('-', "")).unwrap();
        let link = ActualDuplicateLink {
            report: record.id,
            known_operation: known.operation,
            known_trace: known.trace,
            known_variant: known.variant.id,
            verification: note("Fixture duplicate of the exact sealed physical original"),
        };
        // These SQL capacity fixtures retain genuinely qualified captures and
        // marker seals. They do not claim ordinary native command admission.
        meaning.stock_revision = revision;
        meaning.catalog_revision = catalog;
        meaning.latest_recorded_at = if n == 0 { at(160) } else { at(170) };
        let plan = link
            .plan_with_interpretation(&record, &meaning, None, op, owner(), at(170))
            .unwrap();
        let hash = link.fingerprint(hub.id(), owner(), op);
        let material = [HubSubmittedMaterial::ActualLink(Box::new(
            HubSubmittedActualLink {
                id: op,
                report: record.clone(),
                known: known.clone(),
                interpretation: Some(meaning.clone()),
            },
        ))];
        let archive = HubSubmittedRequest::ActualLink(&link)
            .capture(
                &id,
                scope,
                &HubSubmissionContext {
                    hub,
                    catalog_revision: Some(catalog),
                    stock_revision: Some(revision),
                    material: &material,
                },
            )
            .unwrap();
        admission_row(&mut raw, &id, scope, hub.id(), &hash, &archive).await;
        revision = HubRevision::from_value(revision.value() + 1).unwrap();
        let seal = actual_duplicate_result_digest(&hash, &link, &plan, revision, catalog).unwrap();
        // The fixture link's recorded clock must match its sealed plan.
        sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,'actual_review',?,?,?,?,170)")
            .bind(id.as_str()).bind(hub.id().as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(hash.as_slice())
            .bind(i64::try_from(catalog.value()).unwrap()).bind(i64::try_from(revision.value()).unwrap()).bind(record.id.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
        sqlx::query("INSERT INTO hub_actual_interpretation_markers (operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,capture_hash) SELECT o.operation_id,o.hub_id,e.epoch,e.legacy_stock_revision,e.legacy_catalog_revision,o.stock_revision,o.catalog_revision,s.capture_hash FROM hub_stock_operations o JOIN hub_actual_interpretation_epochs e ON e.hub_id=o.hub_id JOIN hub_submissions s ON s.operation_id=o.operation_id WHERE o.operation_id=?")
            .bind(id.as_str()).execute(&mut raw).await.unwrap();
        let (kind, start, end) = crate::hub::actuals::common::occurrence_fields(known.occurred);
        sqlx::query("INSERT INTO hub_actual_reviews (operation_id,report_id,policy_version,review_kind,trace_id,group_id,variant_id,variant_revision,recipient_id,recipient_revision,packs,known_operation_id,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,verification,lineage_count,scope_count,result_hash) VALUES (?,?,1,'link',?,?,?,?,?,?,2,?,?,?,?,?,0,0,?)")
            .bind(id.as_str()).bind(record.id.as_bytes().as_slice()).bind(known.trace.as_bytes().as_slice()).bind(known.lines[0].group.as_bytes().as_slice())
            .bind(known.variant.id.as_bytes().as_slice()).bind(i64::try_from(known.variant.revision.value()).unwrap())
            .bind(known.recipient.id.as_bytes().as_slice()).bind(i64::try_from(known.recipient.revision.value()).unwrap())
            .bind(operation(81).as_str()).bind(kind).bind(start).bind(end).bind(link.verification.as_str()).bind(seal.as_slice()).execute(&mut raw).await.unwrap();
        sqlx::query("UPDATE hub_actual_report_reviews SET review_operation_id=? WHERE report_id=?")
            .bind(id.as_str())
            .bind(record.id.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1")
        .bind(i64::try_from(revision.value()).unwrap())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("COMMIT").execute(&mut raw).await.unwrap();
    let counts=sqlx::query("SELECT (SELECT count(*) FROM hub_actual_reports) AS reports,(SELECT count(*) FROM hub_actual_report_reviews WHERE review_operation_id IS NULL) AS unresolved,(SELECT count(*) FROM hub_stock_events) AS events").fetch_one(&mut raw).await.unwrap();
    assert_eq!(counts.get::<i64, _>("reports"), 4096);
    assert_eq!(counts.get::<i64, _>("unresolved"), 0);
    assert_eq!(counts.get::<i64, _>("events"), 2);
    raw.close().await.unwrap();
    let db = fixture.open().await;
    assert!(
        db.hub_actual_report_operation(owner(), operation(104094))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db.hub_actual_review_operation(owner(), operation(204094))
            .await
            .unwrap()
            .is_some()
    );
    let current = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(current.group, before.group);
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(90),
            HubMutation::ActualReport(Box::new(report())),
            at(180)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let pending = db
        .hub_submission(owner(), operation(90))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    assert!(
        db.hub_actual_report_operation(owner(), operation(90))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(91),
            HubMutation::ActualReport(Box::new(report())),
            at(190)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubCapacity
    );
    assert!(
        db.hub_submission(owner(), operation(91))
            .await
            .unwrap()
            .is_none()
    );
    let command = harvestcircle_domain::hub::movement::MovementCommand {
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        group: current.group.id,
        expected_group_revision: current.group.revision,
        to: id(7),
        packs: current.group.packs,
        separated_portion: None,
        occurred: Occurrence::Known(at(190)),
        observation: note("Pending overflow claim must fence this later planned action"),
    };
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(92),
            HubMutation::Handling(Box::new(
                harvestcircle_domain::hub::movement::HandlingCommand::Move(command)
            )),
            at(190)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_submission(owner(), operation(90))
            .await
            .unwrap()
            .unwrap()
            .captured_semantics(),
        pending.captured_semantics()
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group,
        before.group
    );
    db.close().await.unwrap();
}
