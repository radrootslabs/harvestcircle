//! One-use retention uses ordinary native commands, never historical SQL images.
use super::*;
use crate::hub::corrections::upgrade_tests as inspect;
use sqlx::{Connection, Row};
use std::{fs, path::PathBuf};

fn retained_root() -> PathBuf {
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    project.join("retained_evidence").canonicalize().unwrap()
}

async fn retain_world(output: PathBuf) {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 19);
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    assert!(!output.exists());
    let objects = inspect::published19_objects();
    fs::create_dir(&output).unwrap();
    inspect::write_new(
        &output.join("retention.claim"),
        b"One-use fresh native19 predecessor; preserve profile on every failure.\n",
    );
    let directory = tempfile::tempdir_in(&output).unwrap();
    let mut fixture = Fixture::from_directory(directory);
    fixture.build = build();
    // Keeping ownership before the first native open preserves even a partial
    // new fixture. No profile is moved or copied into another runtime namespace.
    let (context, build, profile) = fixture.retain();
    assert!(profile.starts_with(&output));
    let state = context.paths().state().join("state.sqlite");
    assert!(state.starts_with(&profile));
    inspect::write_new(&output.join("profile-custody.json"), &serde_json::to_vec_pretty(&serde_json::json!({
        "profile_source_root":profile,"state_file":state,"producer_source_base":SOURCE,
        "lib_revision":LIB,"rust_version":"1.99.0","target":"aarch64-apple-darwin",
        "feature_profile":"populated19-native-interpretation-predecessor","state_contract_version":19,
        "source_harness_may_be_uncommitted":true,"before_first_native_open":true,
        "partial_profile_must_be_preserved":true
    })).unwrap());
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    let baseline = Box::pin(drive_native19(&db)).await;
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 500, &build).await.unwrap();
    assert_eq!(Box::pin(native_baseline(&db)).await, baseline);
    db.close().await.unwrap();
    let wal = state.with_extension("sqlite-wal");
    match fs::symlink_metadata(&wal) {
        Ok(m) => assert!(m.is_file() && m.len() == 0),
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
    }
    let mut raw = inspect::reader(&state).await;
    inspect::verify_objects(&mut raw, &objects).await;
    inspect::health(&mut raw).await;
    let tables = inspect::inventory(&mut raw).await;
    assert_eq!(tables.as_array().unwrap().len(), 69);
    let schema_objects = inspect::all_objects(&mut raw).await;
    assert_eq!(schema_objects.len(), 262);
    let metadata: Vec<i64> =
        sqlx::query_scalar("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_all(&mut raw)
            .await
            .unwrap();
    assert_eq!(metadata, vec![19]);
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .iter()
            .map(|row| row.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=19).collect::<Vec<_>>()
    );
    let latest = ledger.last().unwrap();
    assert_eq!(
        latest.get::<String, _>("name"),
        "add_linked_correction_ledger"
    );
    assert_eq!(
        latest.get::<Vec<u8>, _>("checksum"),
        crate::hub::corrections::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(latest.get::<i64, _>("applied_at_unix_s"), 100);
    for (field, expected) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", SOURCE),
        ("lib_revision", LIB),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        (
            "feature_profile",
            "populated19-native-interpretation-predecessor",
        ),
    ] {
        assert_eq!(latest.get::<String, _>(field), expected);
    }
    for (field, expected) in [
        ("config_contract_version", 1),
        ("state_contract_version", 19),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(latest.get::<i64, _>(field), expected);
    }
    raw.close().await.unwrap();
    let state_hash = inspect::digest(&state);
    let archive = output.join("original-schema19.sqlite");
    inspect::write_new(&archive, &fs::read(&state).unwrap());
    assert_eq!(inspect::digest(&archive), state_hash);
    let mut copied = inspect::reader(&archive).await;
    assert_eq!(inspect::inventory(&mut copied).await, tables);
    inspect::verify_objects(&mut copied, &objects).await;
    inspect::health(&mut copied).await;
    copied.close().await.unwrap();
    assert_eq!(inspect::digest(&state), state_hash);
    inspect::write_new(&output.join("witness.json"),&serde_json::to_vec_pretty(&serde_json::json!({
        "schema":19,"producer_source_base":SOURCE,"lib_revision":LIB,
        "source_harness_may_be_uncommitted":true,"profile_source_root":profile,"state_file":state,
        "original_archive":archive,"original_sha256":state_hash,"tables":tables,"schema_objects":schema_objects,
        "native_original_baseline":baseline,"service_schema_objects":256,"shared_host_schema_objects":6,
        "ordinary_native_commands_only":true,"sql_preload_or_clone":false,
        "personal_identity_or_keyring_data":false,"saved_pending_decoding_or_execution":false,
        "native19_originals_reopen_equal":true,"legacy_link_interpretation_absent":true,
        "historical_pending19_world_replaced":false,"consumed_do_not_rerun":true
    })).unwrap());
    println!(
        "Populated native19 actual/link/source/recipient/quantity originals retained once for forward20"
    );
}

#[tokio::test]
#[ignore = "Consumed failed v1 retention; preserve its profile, sidecars and unknown Found admission; never rerun"]
async fn retained_populated19_actual_link_and_attribution_originals_for_forward20() {
    retain_world(
        retained_root().join("harvestcircle-populated19-before-actual-interpretations-v1"),
    )
    .await;
}

#[tokio::test]
#[ignore = "Explicit disposable complete caller qualification for the independent v2 native19 predecessor"]
async fn disposable_large_caller_qualifies_heap_bound_native19_retention() {
    let temporary = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    retain_world(
        temporary
            .path()
            .join("disposable-complete-native19-retainer"),
    )
    .await;
}

#[tokio::test]
#[ignore = "Creates one independent v2 native19 predecessor; never replaces or clears the failed v1 profile"]
async fn retained_independent_populated19_after_heap_bound_caller() {
    retain_world(
        retained_root().join("harvestcircle-populated19-before-actual-interpretations-v2"),
    )
    .await;
}
