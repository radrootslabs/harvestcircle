use super::*;
use harvestcircle_domain::hub::corrections::CorrectionObservedFacts;
use harvestcircle_domain::hub::reference::{SourceProvenance, WholePacks};
use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};

fn claim() -> CorrectionObservationCommand {
    CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: HubRecordId::from_bytes([7; 16]).unwrap(),
            corrected_packs: WholePacks::from_value(0).unwrap(),
            report: None,
            basis: SourceProvenance::new(
                "Corrected classification is zero; original must remain visible",
            )
            .unwrap(),
        },
    }
}

fn found_claim() -> CorrectionObservationCommand {
    use harvestcircle_domain::hub::{
        corrections::{FoundStockInput, PhysicalDebitKey},
        movement::PhysicalIdentification,
        reference::HubLabel,
    };
    let id = |n| HubRecordId::from_bytes([n; 16]).unwrap();
    let basis = || {
        SourceProvenance::new("Unverified independently observed remainder; review pending")
            .unwrap()
    };
    CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: id(2),
            target_group: id(3),
            basis: basis(),
            allocations: vec![
                FoundStockInput {
                    debit: PhysicalDebitKey {
                        operation: id(4),
                        group: id(5)
                    },
                    packs: WholePacks::from_value(0).unwrap(),
                    location: id(6),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Observed crates").unwrap(),
                        observation: basis()
                    },
                    cohort_evidence: basis(),
                };
                2
            ],
        },
    }
}

#[tokio::test]
async fn corrupt_observation_reservation_cannot_underreserve_or_admit_another_mutation() {
    use crate::hub::stock::tests::{Fixture, operation, owner, query, setup};
    use harvestcircle_application::hub_submission::HubMutation;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let scope = scope(&db).await;
    db.submit_hub_mutation(
        scope,
        operation(2),
        HubMutation::CorrectionObservation(Box::new(found_claim())),
        UnixTimestamp::from_seconds(150).unwrap(),
    )
    .await
    .unwrap();
    let mut raw = fixture.raw().await;
    let ddl: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='hub_correction_observations_no_update'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    assert!(
        sqlx::query("UPDATE hub_correction_observations SET closure_events=1 WHERE operation_id=?")
            .bind(operation(2).as_str())
            .execute(&mut raw)
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER hub_correction_observations_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE hub_correction_observations SET closure_events=1 WHERE operation_id=?")
            .bind(operation(2).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query(sqlx::AssertSqlSafe(ddl.clone()))
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_schema WHERE name='hub_correction_observations_no_update'"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        ddl
    );
    drop(raw);
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::CorrectionObservation(Box::new(claim())),
            UnixTimestamp::from_seconds(160).unwrap()
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
}

#[test]
fn committed_observation_payload_round_trip_preserves_zero_and_rejects_noncanonical_data() {
    let command = claim();
    let bytes = observation_payload::encode(&command).unwrap();
    assert_eq!(observation_payload::decode(&bytes).unwrap(), command);
    let mut extra = bytes.clone();
    extra.push(b' ');
    assert!(observation_payload::decode(&extra).is_err());
    let text = String::from_utf8(bytes.clone()).unwrap();
    for (before, after) in [
        ("\"schema\":1", "\"schema\":2"),
        ("\"schema\":1", "\"schema\":1,\"schema\":1"),
        ("\"schema\":1", "\"schema\":1,\"unrecognized\":0"),
        ("\"corrected_packs\":0", "\"corrected_packs\":1000001"),
        (
            "07070707070707070707070707070707",
            "00000000000000000000000000000000",
        ),
    ] {
        let changed = text.replace(before, after);
        assert_ne!(changed, text);
        assert!(observation_payload::decode(changed.as_bytes()).is_err());
    }
    assert!(observation_payload::decode(&[]).is_err());
    assert!(observation_payload::decode(&vec![b' '; observation_payload::MAX_BYTES + 1]).is_err());
}

#[test]
fn schema19_pins_match_native_measurements() {
    let checksum = MigrationChecksum::for_sql(schema::MIGRATION_SQL);
    let mut objects = crate::contract::retained_schema18_objects();
    objects.retain(|object| !schema::REPLACED_OBJECTS.contains(&object.name()));
    objects.extend(schema::schema_objects().unwrap());
    let digest = SchemaVersionCatalog::computed_digest(19, objects).unwrap();
    println!("CANDIDATE19_CHECKSUM={:?}", checksum.as_bytes());
    println!("CANDIDATE19_DIGEST={:?}", digest.as_bytes());
    assert_eq!(checksum, schema::MIGRATION_CHECKSUM);
    assert_eq!(digest.as_bytes(), &schema::SCHEMA_DIGEST);
    assert!(!schema::schema_objects().unwrap().is_empty());
}

#[tokio::test]
async fn candidate_schema19_executes_only_in_fresh_owned_fixture_and_retains_foreign_keys() {
    let fixture = crate::hub::stock::tests::Fixture::new();
    let db = fixture.open().await;
    let mut raw = fixture.raw().await;
    // A registered forward migration in a fresh owned fixture. This is separate
    // from historical upgrade acceptance; no retained world or pending request.
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert!(violations.is_empty());
    let objects: i64=sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'hub_correction%' OR name LIKE 'hub_actual_correction%' OR name='hub_actual_review_correction_scope'").fetch_one(&mut raw).await.unwrap();
    assert_eq!(
        objects as usize + schema::REPLACED_OBJECTS.len(),
        schema::schema_objects().unwrap().len()
    );
    drop(raw);
    drop(db);
}

async fn scope(db: &Database) -> harvestcircle_application::hub_submission::HubSubmissionScope {
    use harvestcircle_application::SessionGeneration;
    let actor = crate::hub::stock::tests::owner();
    let now = UnixTimestamp::from_seconds(140).unwrap();
    let runtime = db.begin_hub_runtime(now).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(actor), now)
        .await
        .unwrap();
    harvestcircle_application::hub_submission::HubSubmissionScope {
        runtime,
        generation,
        actor,
    }
}

#[tokio::test]
async fn zero_observation_preserves_stock_work_original_and_planned_action_fence_after_reopen() {
    use crate::hub::stock::tests::{Fixture, command, now, operation, owner, query, setup};
    use harvestcircle_application::hub_submission::HubMutation;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let original = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let scope = scope(&db).await;
    let input = claim();
    let saved = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CorrectionObservation(Box::new(input.clone())),
            UnixTimestamp::from_seconds(150).unwrap(),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::CorrectionObserved(saved) = saved else {
        panic!("expected observation")
    };
    assert_eq!(saved.original.observation, input);
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.revision.value(), before.revision.value() + 1);
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    let mut planned = command(&catalog);
    planned.expected_stock_revision = after.revision;
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::Receive(Box::new(planned)),
            UnixTimestamp::from_seconds(160).unwrap()
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::HubConflict
    );
    let replay = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CorrectionObservation(Box::new(input)),
            UnixTimestamp::from_seconds(170).unwrap(),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::CorrectionObserved(replay) = replay else {
        panic!("expected original")
    };
    assert_eq!(replay, saved);
    db.close().await.unwrap();
    drop(db);
    let db = fixture.open().await;
    assert_eq!(
        db.hub_correction_observation_original(owner(), operation(2))
            .await
            .unwrap(),
        *saved
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows,
        before.rows
    );
}

#[tokio::test]
async fn backwards_recording_clock_keeps_raw_pending_and_never_records_a_rejection() {
    use crate::hub::stock::tests::{Fixture, command, now, operation, owner, setup};
    use harvestcircle_application::hub_submission::HubMutation;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let scope = scope(&db).await;
    let mut raw = fixture.raw().await;
    let error = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CorrectionObservation(Box::new(claim())),
            UnixTimestamp::from_seconds(99).unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        harvestcircle_domain::SafeErrorCode::HubOutcomeUnknown
    );
    let (pending,roots,rejected):(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM hub_submissions WHERE kind='correction_observe'),(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_observe'),(SELECT count(*) FROM hub_submission_rejections)").fetch_one(&mut raw).await.unwrap();
    assert_eq!((pending, roots, rejected), (1, 0, 0));
    let body: Vec<u8> = sqlx::query_scalar(
        "SELECT capture_body FROM hub_submissions WHERE kind='correction_observe'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    let metadata =
        harvestcircle_application::hub_submission::inspect_hub_submission(&body).unwrap();
    assert_eq!(metadata.kind, "correction_observe");
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CorrectionObservation(Box::new(claim())),
            UnixTimestamp::from_seconds(160).unwrap()
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::HubOutcomeUnknown
    );
    let roots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_stock_operations WHERE kind='correction_observe'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    assert_eq!(roots, 0);
    drop(raw);
    db.close().await.unwrap();
    drop(db);
    let db = fixture.open().await;
    let pending = db
        .hub_submission(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    assert_eq!(pending.captured_semantics(), body);
}

#[tokio::test]
async fn supersession_keeps_original_fact_and_transfers_one_current_reservation() {
    use crate::hub::stock::tests::{Fixture, operation, owner, setup};
    use harvestcircle_application::hub_submission::HubMutation;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let scope = scope(&db).await;
    db.submit_hub_mutation(
        scope,
        operation(2),
        HubMutation::CorrectionObservation(Box::new(claim())),
        UnixTimestamp::from_seconds(150).unwrap(),
    )
    .await
    .unwrap();
    let mut changed = claim();
    changed.previous = Some(crate::hub::actuals::common::operation_id(&operation(2)).unwrap());
    if let CorrectionObservedFacts::CountClassification {
        corrected_packs, ..
    } = &mut changed.facts
    {
        *corrected_packs = WholePacks::from_value(1).unwrap();
    }
    db.submit_hub_mutation(
        scope,
        operation(3),
        HubMutation::CorrectionObservation(Box::new(changed.clone())),
        UnixTimestamp::from_seconds(160).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_correction_observation_original(owner(), operation(2))
            .await
            .unwrap()
            .original
            .observation,
        claim()
    );
    assert_eq!(
        db.hub_correction_observation_original(owner(), operation(3))
            .await
            .unwrap()
            .original
            .observation,
        changed
    );
    let mut raw = fixture.raw().await;
    let unresolved:i64=sqlx::query_scalar("SELECT count(*) FROM hub_correction_observations p WHERE NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=p.operation_id)").fetch_one(&mut raw).await.unwrap();
    assert_eq!(unresolved, 1);
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(4),
            HubMutation::CorrectionObservation(Box::new(changed)),
            UnixTimestamp::from_seconds(170).unwrap()
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::HubOutcomeUnknown
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_correction_observations")
        .fetch_one(&mut raw)
        .await
        .unwrap();
    assert_eq!(rows, 2);
}
