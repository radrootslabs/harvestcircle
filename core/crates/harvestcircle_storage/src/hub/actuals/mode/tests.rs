#[test]
fn count_aware_result_purposes_cannot_be_empty_physical20_or_cross_kind_results() {
    use super::Capture;
    let base = [7u8; 32];
    for old in [Capture::Legacy, Capture::Physical20] {
        assert_eq!(old.departure_result(base), base);
        assert_eq!(old.link_result(base), base);
    }
    assert_ne!(Capture::Count21.departure_result(base), base);
    assert_ne!(Capture::Count21.link_result(base), base);
    assert_ne!(
        Capture::Count21.departure_result(base),
        Capture::Count21.link_result(base)
    );
    assert_ne!(
        Capture::Count21.departure_result(base),
        Capture::Count21.departure_result([8u8; 32])
    );
}

#[test]
fn native20_schema_pins_bind_the_additive_epoch_and_exact_review_guard() {
    use crate::hub::actuals::interpretation_schema as schema;
    use radroots_service_sqlite::{MigrationChecksum, SchemaVersionCatalog};
    let checksum = MigrationChecksum::for_sql(schema::MIGRATION_SQL);
    let mut objects = crate::contract::retained_schema19_objects();
    objects.retain(|object| !schema::REPLACED_OBJECTS.contains(&object.name()));
    objects.extend(schema::schema_objects().unwrap());
    let digest = SchemaVersionCatalog::computed_digest(20, objects).unwrap();
    println!("NATIVE20_CHECKSUM={:?}", checksum.as_bytes());
    println!("NATIVE20_DIGEST={:?}", digest.as_bytes());
    assert_eq!(checksum, schema::MIGRATION_CHECKSUM);
    assert_eq!(digest.as_bytes(), &schema::SCHEMA_DIGEST);
}

#[tokio::test]
async fn native20_new_hub_and_first_actual_bind_qualified_empty_meanings_to_exact_admission() {
    use crate::hub::{
        corrections::projection_native_tests,
        stock::tests::{Fixture, owner},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, operation, _) = projection_native_tests::actual_debit(&db).await;
    let original = db
        .hub_actual_review_operation(owner(), operation.clone())
        .await
        .unwrap()
        .unwrap();
    let mut raw = fixture.raw().await;
    let epoch: (i64,i64,i64) = sqlx::query_as("SELECT epoch,legacy_stock_revision,legacy_catalog_revision FROM hub_actual_interpretation_epochs").fetch_one(&mut raw).await.unwrap();
    assert_eq!(epoch, (1, 1, 1));
    let marker: (i64,i64,Vec<u8>,Vec<u8>) = sqlx::query_as("SELECT m.stock_revision,m.catalog_revision,m.capture_hash,s.capture_hash FROM hub_actual_interpretation_markers m JOIN hub_submissions s ON s.operation_id=m.operation_id WHERE m.operation_id=?").bind(operation.as_str()).fetch_one(&mut raw).await.unwrap();
    assert_eq!(marker.0 as u64, original.revision.value());
    assert_eq!(marker.1 as u64, original.catalog_revision.value());
    assert_eq!(marker.2, marker.3);
    let capture: Vec<u8> =
        sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
            .bind(operation.as_str())
            .fetch_one(&mut raw)
            .await
            .unwrap();
    let text = String::from_utf8(capture).unwrap();
    assert!(text.contains("\"interpretations\":"));
    assert!(text.contains("\"records\":[]"));
    drop(raw);
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_actual_review_operation(owner(), operation)
            .await
            .unwrap()
            .unwrap(),
        original
    );
    reopened.close().await.unwrap();
}
mod faults;

async fn unlinked_world() -> (
    crate::hub::stock::tests::Fixture,
    crate::Database,
    harvestcircle_application::hub_submission::HubSubmissionScope,
    harvestcircle_domain::hub::actuals::ActualDuplicateLink,
) {
    use crate::hub::{
        corrections::projection_native_tests,
        stock::tests::{Fixture, id, operation, owner},
    };
    use harvestcircle_application::{
        SessionGeneration,
        hub_submission::{HubMutation, HubSubmissionScope},
    };
    use harvestcircle_domain::{
        UnixTimestamp,
        hub::{
            actuals::{ActualDuplicateLink, ActualReportCommand, ActualReportSources},
            reference::{SourceProvenance, WholePacks},
            stock::Occurrence,
        },
    };
    let at = |n| UnixTimestamp::from_seconds(n).unwrap();
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) = projection_native_tests::actual_debit(&db).await;
    let runtime = db.begin_hub_runtime(at(170)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(170),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let note =
        SourceProvenance::new("Independently linked fictional original; zero new physical effect")
            .unwrap();
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(18000),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(before.variant),
                recipient: Some(id(3)),
                packs: Some(WholePacks::from_value(2).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note.clone(),
            })),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("raw link report")
    };
    let command = ActualDuplicateLink {
        report: report.original.id,
        known_operation: crate::hub::actuals::common::operation_id(&target).unwrap(),
        known_trace: before.trace,
        known_variant: before.variant,
        verification: note,
    };
    (fixture, db, scope, command)
}

#[tokio::test]
async fn native20_selected_link_bad_trace_variant_or_named_nondeparture_remain_invalid_input_without_writes()
 {
    use crate::hub::stock::tests::{id, operation, owner};
    use harvestcircle_domain::{SafeErrorCode, UnixTimestamp};
    let (fixture, db, scope, command) = unlinked_world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    for fault in 0..3 {
        let mut wrong = command.clone();
        match fault {
            0 => wrong.known_trace = id(99),
            1 => wrong.known_variant = id(99),
            2 => {
                wrong.known_operation =
                    crate::hub::actuals::common::operation_id(&operation(18000)).unwrap()
            }
            _ => unreachable!(),
        }
        assert_eq!(
            db.hub_actual_link_preview(
                owner(),
                operation(18001),
                wrong,
                UnixTimestamp::from_seconds(190).unwrap()
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubInvalidInput
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    use harvestcircle_application::hub_submission::HubMutation;
    use harvestcircle_domain::hub::{
        actuals::{ActualReportCommand, ActualReportSources},
        reference::{SourceProvenance, WholePacks},
        stock::Occurrence,
    };
    db.submit_hub_mutation(
        scope,
        operation(18001),
        HubMutation::ActualLink(Box::new(command.clone())),
        UnixTimestamp::from_seconds(190).unwrap(),
    )
    .await
    .unwrap();
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(18002),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(command.known_variant),
                recipient: None,
                packs: Some(WholePacks::from_value(2).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: SourceProvenance::new(
                    "Another fictional report, not another physical departure",
                )
                .unwrap(),
            })),
            UnixTimestamp::from_seconds(200).unwrap(),
        )
        .await
        .unwrap()
    else {
        panic!("second raw report")
    };
    let mut wrong = command;
    wrong.report = report.original.id;
    wrong.known_operation = crate::hub::actuals::common::operation_id(&operation(18001)).unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_link_preview(
            owner(),
            operation(18003),
            wrong,
            UnixTimestamp::from_seconds(210).unwrap()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native20_damaged_selected_membership_is_corruption_before_scope_classification() {
    use crate::hub::stock::tests::{id, operation, owner};
    use harvestcircle_domain::{SafeErrorCode, UnixTimestamp};
    use sqlx::{AssertSqlSafe, Connection};
    for fault in 0..2 {
        let (fixture, db, _, command) = unlinked_world().await;
        let guard = if fault == 0 {
            "hub_actual_reviews_no_update"
        } else {
            "hub_receipts_no_update"
        };
        let mut raw = fixture.raw().await;
        let ddl: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=?")
            .bind(guard)
            .fetch_one(&mut raw)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {guard}")))
            .execute(&mut raw)
            .await
            .unwrap();
        // Both references remain FK-valid. Native original verification must
        // reject a party in the selected review or population's variant slot.
        if fault == 0 {
            sqlx::query("UPDATE hub_actual_reviews SET variant_revision=recipient_revision,variant_id=recipient_id WHERE operation_id=?")
                .bind(operation(81).as_str()).execute(&mut raw).await.unwrap();
        } else {
            sqlx::query("UPDATE hub_receipts SET variant_id=?,variant_revision=1 WHERE trace_id=?")
                .bind(id(3).as_bytes().as_slice())
                .bind(command.known_trace.as_bytes().as_slice())
                .execute(&mut raw)
                .await
                .unwrap();
        }
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_actual_link_preview(
                owner(),
                operation(18001),
                command,
                UnixTimestamp::from_seconds(190).unwrap()
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
