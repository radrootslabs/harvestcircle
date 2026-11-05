//! A supplementary current20 predecessor, never a replacement historical image.
use super::*;
use crate::hub::corrections::upgrade_tests as inspect;
use sqlx::{Connection, Row};
use std::{fs, path::PathBuf};

const SOURCE20: &str = "3407520594c89459b01828a2166f32231c4eda07";
const FEATURE20: &str = "populated20-qualified-interpretation-predecessor";

fn build20() -> MigrationBuildIdentity {
    MigrationBuildIdentity::new(
        "0.1.0-alpha",
        SOURCE20,
        LIB,
        "1.99.0",
        "aarch64-apple-darwin",
        FEATURE20,
        1,
        20,
        1,
        1,
        1,
    )
    .unwrap()
}

fn objects20() -> Vec<radroots_service_sqlite::SchemaObject> {
    let mut objects = crate::contract::retained_schema19_objects();
    let schema = crate::hub::actuals::interpretation_schema::schema_objects().unwrap();
    objects.retain(|object| {
        !crate::hub::actuals::interpretation_schema::REPLACED_OBJECTS.contains(&object.name())
    });
    objects.extend(schema);
    assert_eq!(objects.len(), 265);
    let digest =
        radroots_service_sqlite::SchemaVersionCatalog::computed_digest(20, objects.clone())
            .unwrap();
    assert_eq!(
        digest.as_bytes(),
        &crate::hub::actuals::interpretation_schema::SCHEMA_DIGEST
    );
    assert_eq!(
        radroots_service_sqlite::MigrationChecksum::for_sql(
            crate::hub::actuals::interpretation_schema::MIGRATION_SQL
        ),
        crate::hub::actuals::interpretation_schema::MIGRATION_CHECKSUM
    );
    objects
}

async fn baseline20(db: &Database) -> serde_json::Value {
    let mut actuals = Vec::new();
    for identity in [81, LINK, 30102, 30105] {
        let original = db
            .hub_actual_review_operation(owner(), operation(identity))
            .await
            .unwrap()
            .unwrap();
        actuals.push(actual_value(&original));
    }
    // Departure plans intentionally do not expose captured interpretation
    // material. Their full native getter above verifies the admission; retain
    // the exact capture as separate evidence rather than inventing a DTO field.
    let capture: Vec<u8> = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
                    .bind(operation(81).as_str())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| crate::db::storage_unavailable())
            })
        })
        .await
        .unwrap();
    let text = std::str::from_utf8(&capture).unwrap();
    assert!(text.contains("\"interpretations\":"));
    assert!(text.contains("\"records\":[]"));
    assert!(actuals[1]["plan"]["interpretation"].is_object());
    let first = db
        .hub_actual_review_operation(owner(), operation(30102))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Duplicate(first) = first.original else {
        panic!("Unknown link")
    };
    let first = first.interpretation.as_ref().unwrap();
    assert_eq!(first.view().unwrap().original_packs.value(), 2);
    assert_eq!(first.view().unwrap().effective_packs.value(), 0);
    assert!(matches!(
        harvestcircle_domain::hub::corrections::current_recipient_attribution(
            &first.recipient_original,
            &first.recipient_history
        )
        .unwrap(),
        AmendedRecipient::Unknown
    ));
    let last = db
        .hub_actual_review_operation(owner(), operation(30105))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Duplicate(last) = last.original else {
        panic!("Known link")
    };
    let last = last.interpretation.as_ref().unwrap();
    assert_eq!(last.view().unwrap().effective_packs.value(), 0);
    assert!(
        matches!(harvestcircle_domain::hub::corrections::current_recipient_attribution(&last.recipient_original, &last.recipient_history).unwrap(), AmendedRecipient::Known(party) if party.id == id(4))
    );
    let trace = last.original.trace;
    let known = db
        .hub_actual_known_interpretations(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert!(known.unresolved_actual && !known.uncertain_intent);
    assert_eq!(known.interpretations.records.len(), 1);
    assert_eq!(known.interpretations.records[0].recipient_history.len(), 4);
    assert_eq!(
        known.interpretations.records[0]
            .view()
            .unwrap()
            .effective_packs
            .value(),
        0
    );
    let additional = db
        .hub_additional_quantity_original(owner(), operation(ADDITIONAL))
        .await
        .unwrap()
        .unwrap();
    let first_found = db
        .hub_found_correction_original(owner(), operation(FOUND_ORIGINAL + 1))
        .await
        .unwrap()
        .unwrap();
    let second_found = db
        .hub_found_correction_original(owner(), operation(FOUND_ADDITIONAL + 1))
        .await
        .unwrap()
        .unwrap();
    let mut sources = Vec::new();
    for identity in [FIRST_SOURCE, SECOND_SOURCE] {
        let source = db
            .hub_source_attribution_original(owner(), operation(identity))
            .await
            .unwrap()
            .unwrap();
        sources.push(serde_json::to_value(source.original).unwrap());
    }
    let mut recipients = Vec::new();
    for identity in [UNKNOWN_RECIPIENT, KNOWN_RECIPIENT, 30100, 30103] {
        let recipient = db
            .hub_recipient_attribution_original(owner(), operation(identity))
            .await
            .unwrap()
            .unwrap();
        recipients.push(serde_json::to_value(recipient.original).unwrap());
    }
    let unresolved = db
        .hub_actual_report_operation(owner(), operation(UNRESOLVED))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        db.hub_submission(owner(), operation(UNRESOLVED))
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::HubSubmissionStatus::Recorded
    ));
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        20
    );
    serde_json::json!({"actuals":actuals,"first_actual_capture":capture,"known":known.interpretations,
        "additional":additional.original,"first_found":first_found.original,"first_found_impact":first_found.impact,
        "second_found":second_found.original,"second_found_impact":second_found.impact,
        "sources":sources,"recipients":recipients,"unresolved_raw":unresolved.original,
        "stock_revision":page.revision,"catalog_revision":page.catalog_revision,"on_hand":20,
        "reference_and_receipt_setup_pre_session":true,"later_mutations_use_native_admission":true})
}

async fn drive20(db: &Database) -> serde_json::Value {
    let (_, cuts, scope) = Box::pin(drive_native_observed(db, true)).await;
    let trace = cuts.last().unwrap().records[0].original.trace;
    let target = crate::hub::actuals::common::operation_id(&operation(81)).unwrap();
    let unknown = command(
        db,
        target,
        None,
        Some(crate::hub::actuals::common::operation_id(&operation(KNOWN_RECIPIENT)).unwrap()),
    )
    .await;
    db.submit_hub_mutation(
        scope,
        operation(30100),
        HubMutation::RecipientAttribution(Box::new(unknown)),
        at(400),
    )
    .await
    .unwrap();
    let first = Box::pin(link(db, scope, trace, 30101, None, 410)).await;
    let known = command(
        db,
        target,
        Some(id(4)),
        Some(crate::hub::actuals::common::operation_id(&operation(30100)).unwrap()),
    )
    .await;
    db.submit_hub_mutation(
        scope,
        operation(30103),
        HubMutation::RecipientAttribution(Box::new(known)),
        at(430),
    )
    .await
    .unwrap();
    let second = Box::pin(link(db, scope, trace, 30104, Some(id(4)), 440)).await;
    for original in [first, second] {
        assert_eq!(
            db.hub_actual_review_operation(owner(), original.operation.clone())
                .await
                .unwrap(),
            Some(original)
        );
    }
    Box::pin(baseline20(db)).await
}

async fn retain20(output: PathBuf) {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 20);
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    assert!(!output.exists());
    let objects = objects20();
    fs::create_dir(&output).unwrap();
    inspect::write_new(&output.join("retention.claim"), b"One-use supplemental published20 qualified originals; preserve every partial file on failure.\n");
    let directory = tempfile::tempdir_in(&output).unwrap();
    let mut fixture = Fixture::from_directory(directory);
    fixture.build = build20();
    let (context, build, profile) = fixture.retain();
    let state = context.paths().state().join("state.sqlite");
    assert!(state.starts_with(&profile) && profile.starts_with(&output));
    inspect::write_new(
        &output.join("profile-custody.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "producer_source_base":SOURCE20,"lib_revision":LIB,"feature_profile":FEATURE20,
            "profile_source_root":profile,"state_file":state,"before_first_native_open":true,
            "partial_profile_must_be_preserved":true,"source_harness_may_be_uncommitted":true,
            "no_existing_historical_world_replaced":true
        }))
        .unwrap(),
    );
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    let baseline = Box::pin(drive20(&db)).await;
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 500, &build).await.unwrap();
    assert_eq!(Box::pin(baseline20(&db)).await, baseline);
    db.close().await.unwrap();
    match fs::symlink_metadata(state.with_extension("sqlite-wal")) {
        Ok(metadata) => assert!(metadata.is_file() && metadata.len() == 0),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
    }
    let mut raw = inspect::reader(&state).await;
    inspect::verify_objects(&mut raw, &objects).await;
    inspect::health(&mut raw).await;
    let tables = inspect::inventory(&mut raw).await;
    let schema_objects = inspect::all_objects(&mut raw).await;
    assert_eq!(tables.as_array().unwrap().len(), 71);
    assert_eq!(schema_objects.len(), 271);
    let schema: Vec<i64> =
        sqlx::query_scalar("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_all(&mut raw)
            .await
            .unwrap();
    assert_eq!(schema, vec![20]);
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .iter()
            .map(|row| row.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=20).collect::<Vec<_>>()
    );
    let latest = ledger.last().unwrap();
    assert_eq!(
        latest.get::<String, _>("name"),
        "bind_actual_interpretation_epochs"
    );
    assert_eq!(
        latest.get::<Vec<u8>, _>("checksum"),
        crate::hub::actuals::interpretation_schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(latest.get::<i64, _>("applied_at_unix_s"), 100);
    for (field, expected) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", SOURCE20),
        ("lib_revision", LIB),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        ("feature_profile", FEATURE20),
    ] {
        assert_eq!(latest.get::<String, _>(field), expected);
    }
    for (field, expected) in [
        ("config_contract_version", 1),
        ("state_contract_version", 20),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(latest.get::<i64, _>(field), expected);
    }
    let reviews: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_reviews")
        .fetch_one(&mut raw)
        .await
        .unwrap();
    let markers: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_interpretation_markers")
        .fetch_one(&mut raw)
        .await
        .unwrap();
    assert_eq!((reviews, markers), (4, 4));
    let unresolved: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews marker JOIN hub_actual_reports report ON report.report_id=marker.report_id WHERE report.operation_id=? AND marker.review_operation_id IS NULL")
        .bind(operation(UNRESOLVED).as_str()).fetch_one(&mut raw).await.unwrap();
    assert_eq!(unresolved, 1);
    raw.close().await.unwrap();
    let hash = inspect::digest(&state);
    let archive = output.join("original-schema20.sqlite");
    inspect::write_new(&archive, &fs::read(&state).unwrap());
    let mut copied = inspect::reader(&archive).await;
    inspect::verify_objects(&mut copied, &objects).await;
    inspect::health(&mut copied).await;
    assert_eq!(inspect::inventory(&mut copied).await, tables);
    copied.close().await.unwrap();
    assert_eq!(inspect::digest(&state), hash);
    assert_eq!(inspect::digest(&archive), hash);
    inspect::write_new(&output.join("witness.json"), &serde_json::to_vec_pretty(&serde_json::json!({
        "schema":20,"producer_source_base":SOURCE20,"lib_revision":LIB,"feature_profile":FEATURE20,
        "source_harness_may_be_uncommitted":true,"profile_source_root":profile,"state_file":state,
        "original_archive":archive,"original_sha256":hash,"tables":tables,"schema_objects":schema_objects,
        "native_original_baseline":baseline,"service_schema_objects":265,"shared_host_schema_objects":6,
        "all_four_reviews_have_native_interpretation_markers":true,
        "qualified_empty_and_nonempty_and_zero_unknown_known_originals":true,
        "native20_originals_reopen_equal":true,"ordinary_native_commands_only":true,
        "reference_and_receipt_setup_pre_session":true,"later_mutations_use_native_admission":true,
        "sql_terminal_preload_or_historical_clone":false,"personal_identity_or_keyring_data":false,
        "saved_pending_decoding_or_execution":false,"old_historical_worlds_replaced":false,
        "unresolved_raw_operation":operation(UNRESOLVED).as_str(),"unresolved_raw_unchanged":true,
        "absence_is_no_effect_proof":false,"consumed_do_not_rerun":true
    })).unwrap());
    println!(
        "Supplemental published20 qualified actual/link/Found/source/recipient originals retained once"
    );
}

#[tokio::test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn disposable_published20_qualified_retainer_checks_complete_caller_and_reopen() {
    let temporary = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    Box::pin(retain20(
        temporary.path().join("disposable-qualified20-retainer"),
    ))
    .await;
}

#[tokio::test]
#[ignore = "One-use supplemental published20 retention; never reset or rerun a consumed profile"]
async fn retain_actual_published20_qualified_originals_before_forward_schema() {
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let root = project.join("retained_evidence").canonicalize().unwrap();
    Box::pin(retain20(
        root.join("harvestcircle-populated20-qualified-before-counts-v1"),
    ))
    .await;
}
