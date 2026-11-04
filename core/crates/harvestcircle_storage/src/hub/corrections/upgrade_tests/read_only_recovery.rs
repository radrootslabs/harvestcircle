//! Read-only completion after the consumed upgrade's post-close WAL assertion.
//! Never opens a writable service, begins a runtime or decodes saved input.
use super::*;

const CURRENT: &str = "7964a03bbf5d1162e893c98b4d156cff401a3156e55bd45ebca06f9cc886cdb7";
const FAILED_LOG: &str = "a1aeac07a85346cc0a8126f2a6000da5a3f729da23f68caf9051bbbc795a3fc2";
const FAILED_SOURCE: &str = "55d97a724655ba7e3fc26362bd166dc1a2e6d85cbb5bb11bfc8775225c86f42a";
const FAILED_BINARY: &str = "5576b61faec649f8263adea25a02bf70e6c7dc7164c4a8a7cc9cd1c9d31d847e";
const ATTEMPT_ARTIFACTS: &str = "42b0e8498f678974b80b5b372cbc92422b94e42d3c89cf860629bba185ff8072";

pub(super) fn closed_wal(path: &Path) -> bool {
    match fs::symlink_metadata(path.with_extension("sqlite-wal")) {
        Ok(metadata) => metadata.is_file() && metadata.len() == 0,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

#[test]
fn closed_native_wal_accepts_absent_or_empty_but_never_nonempty_or_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let state = temporary.path().join("state.sqlite");
    let wal = state.with_extension("sqlite-wal");
    assert!(closed_wal(&state));
    fs::write(&wal, []).unwrap();
    assert!(closed_wal(&state));
    fs::write(&wal, [1]).unwrap();
    assert!(!closed_wal(&state));
    let directory_state = temporary.path().join("directory.sqlite");
    fs::create_dir(directory_state.with_extension("sqlite-wal")).unwrap();
    assert!(!closed_wal(&directory_state));
    #[cfg(unix)]
    {
        let empty = temporary.path().join("empty");
        fs::write(&empty, []).unwrap();
        let linked_state = temporary.path().join("linked.sqlite");
        std::os::unix::fs::symlink(empty, linked_state.with_extension("sqlite-wal")).unwrap();
        assert!(!closed_wal(&linked_state));
    }
}

fn closed_state(path: &Path) {
    assert_eq!(digest(path), CURRENT);
    // A clean native close may remove both sidecars. Never create, checkpoint,
    // truncate or delete them to make a historical inspector succeed.
    assert!(closed_wal(path));
    assert!(!path.with_extension("sqlite-shm").exists());
}

#[tokio::test]
#[ignore = "One-use read-only continuation of consumed19 upgrade; no service open, runtime begin or saved execution"]
async fn retained_published19_read_only_recovery_preserves18_and_completes_inventory() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 19);
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let input = retained
        .join("harvestcircle-schema18-before-corrections-v1")
        .canonicalize()
        .unwrap();
    let output = retained
        .join("harvestcircle-schema19-before-actual-interpretations-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(input.parent(), Some(retained.as_path()));
    assert_eq!(output.parent(), Some(retained.as_path()));
    let witness_file = input.join("witness.json");
    assert_eq!(digest(&witness_file), WITNESS);
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(witness["schema"], 18);
    assert_eq!(witness["original_sha256"], ORIGINAL);
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        profile.starts_with(&retained)
            && state.starts_with(&profile)
            && archive.starts_with(&input)
    );
    closed_state(&state);
    assert_eq!(digest(&archive), ORIGINAL);
    let upgrade_claim = input.join("actual-schema19-upgrade-1.claim");
    assert_eq!(
        fs::read(&upgrade_claim).unwrap(),
        b"One-use actual18->19 native upgrade and published19 retention.\n"
    );
    let diagnostics = output.join("first-attempt-diagnostic");
    assert_eq!(
        digest(&output.join("first-attempt-artifacts.json")),
        ATTEMPT_ARTIFACTS
    );
    let failed_log = diagnostics.join("actual19-upgrade-once.log");
    assert_eq!(digest(&failed_log), FAILED_LOG);
    assert_eq!(
        fs::read_to_string(diagnostics.join("actual19-upgrade-once-exit.txt"))
            .unwrap()
            .trim(),
        "101"
    );
    assert_eq!(
        digest(&diagnostics.join(
            "sources/core/crates/harvestcircle_storage/src/hub/corrections/upgrade_tests.rs"
        )),
        FAILED_SOURCE
    );
    assert_eq!(
        digest(&diagnostics.join("actual19-upgrade-consumer-executable")),
        FAILED_BINARY
    );
    let objects18 = crate::contract::retained_schema18_objects();
    let objects19 = published19_objects();
    let claim = output.join("read-only-recovery-1.claim");
    let archive19 = output.join("original-schema19.sqlite");
    let result_file = output.join("witness.json");
    assert!(!claim.exists() && !archive19.exists() && !result_file.exists());
    write_new(
        &claim,
        b"Read-only continuation only; consumed upgrade must never rerun.\n",
    );
    let mut old = reader(&archive).await;
    verify_objects(&mut old, &objects18).await;
    health(&mut old).await;
    let before = inventory(&mut old).await;
    assert_eq!(before, witness["tables"]);
    assert_eq!(before.as_array().unwrap().len(), 57);
    let originals18 = original_rows(&mut old, 18).await;
    let originals17 = original_rows(&mut old, 17).await;
    let old_objects = all_objects(&mut old).await;
    assert_eq!(old_objects.len(), 201);
    old.close().await.unwrap();
    let mut current = reader(&state).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_all(&mut current)
            .await
            .unwrap(),
        vec![19],
        "exact raw singleton metadata version after native close"
    );
    // Includes every old migration row and all service metadata except the
    // intentionally advanced schema version; the full inventory covers all else.
    assert_eq!(original_rows(&mut current, 18).await, originals18);
    assert_eq!(original_rows(&mut current, 17).await, originals17);
    let after = inventory(&mut current).await;
    assert_eq!(after.as_array().unwrap().len(), 69);
    for table in before.as_array().unwrap() {
        let name = table["table"].as_str().unwrap();
        let observed = after
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["table"] == name)
            .unwrap();
        if !matches!(name, "schema_migrations" | "radroots_service_metadata") {
            assert_eq!(observed, table, "unchanged original {name}");
        }
    }
    for table in after.as_array().unwrap() {
        if !before
            .as_array()
            .unwrap()
            .iter()
            .any(|old| old["table"] == table["table"])
        {
            assert_eq!(table["rows"], 0);
        }
    }
    verify_objects(&mut current, &objects19).await;
    let new_objects = all_objects(&mut current).await;
    assert_eq!(new_objects.len(), 262);
    for (name, sql) in &old_objects {
        if !super::super::schema::REPLACED_OBJECTS.contains(&name.as_str()) {
            assert_eq!(
                &new_objects.iter().find(|(n, _)| n == name).unwrap().1,
                sql,
                "unchanged old service/host object {name}"
            );
        }
    }
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut current)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .iter()
            .map(|r| r.get::<i64, _>("version"))
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
        super::super::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(latest.get::<i64, _>("applied_at_unix_s"), 106);
    for (field, value) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", SOURCE),
        ("lib_revision", LIB),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        ("feature_profile", FEATURE),
    ] {
        assert_eq!(latest.get::<String, _>(field), value);
    }
    for (field, value) in [
        ("config_contract_version", 1),
        ("state_contract_version", 19),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(latest.get::<i64, _>(field), value);
    }
    let submissions = sqlx::query("SELECT * FROM hub_submissions")
        .fetch_all(&mut current)
        .await
        .unwrap();
    assert_eq!(submissions.len(), 1);
    let pending = &submissions[0];
    assert_eq!(
        pending.get::<String, _>("operation_id"),
        operation(4).as_str()
    );
    assert_eq!(pending.get::<String, _>("kind"), "count_observe");
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(pending.get::<Vec<u8>, _>("capture_body"))
        ),
        CAPTURE
    );
    let runtime: Vec<u8> = sqlx::query_scalar("SELECT runtime_id FROM hub_runtime_session")
        .fetch_one(&mut current)
        .await
        .unwrap();
    assert_ne!(runtime, pending.get::<Vec<u8>, _>("runtime_id"));
    for query in [
        "SELECT count(*) FROM hub_history WHERE operation_id=?",
        "SELECT count(*) FROM hub_reference_versions WHERE operation_id=?",
        "SELECT count(*) FROM hub_stock_operations WHERE operation_id=?",
        "SELECT count(*) FROM hub_submission_rejections WHERE operation_id=?",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .bind(operation(4).as_str())
                .fetch_one(&mut current)
                .await
                .unwrap(),
            0
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sum(packs) FROM hub_stock_positions")
            .fetch_one(&mut current)
            .await
            .unwrap(),
        28
    );
    health(&mut current).await;
    current.close().await.unwrap();
    closed_state(&state);
    assert_eq!(digest(&archive), ORIGINAL);
    write_new(&archive19, &fs::read(&state).unwrap());
    assert_eq!(digest(&archive19), CURRENT);
    let mut copied = reader(&archive19).await;
    assert_eq!(inventory(&mut copied).await, after);
    verify_objects(&mut copied, &objects19).await;
    health(&mut copied).await;
    copied.close().await.unwrap();
    closed_state(&state);
    let result = serde_json::json!({"schema":19,"actual_predecessor_schema":18,
        "predecessor_witness":witness_file,"predecessor_witness_sha256":WITNESS,
        "actual_predecessor_archive_sha256":ORIGINAL,"state_file":state,"profile_source_root":profile,
        "original_archive":archive19,"original_sha256":CURRENT,"tables":after,
        "original_tables_before":before,"service_schema_objects":256,"shared_host_schema_objects":6,
        "schema_objects":new_objects,"pending_operation":operation(4).as_str(),"pending_capture_sha256":CAPTURE,
        "runtime_and_pending_unchanged":true,"native19_reopen_originals_verified_before_collector_failure":true,
        "all_migration_build_identity_fields_checked":true,"migration_source_commit":SOURCE,
        "native_upgrade_command_passed":false,"native_upgrade_exit":101,"native_upgrade_log_sha256":FAILED_LOG,
        "failed_upgrade_source_sha256":FAILED_SOURCE,"failed_upgrade_binary_sha256":FAILED_BINARY,
        "failure_phase":"after native19 reopen and close: collector assumed WAL file exists",
        "read_only_remaining_checks_passed":true,"no_writable_service_open_by_recovery":true,
        "no_runtime_begin_or_saved_input_execution":true,"personal_identity_or_keyring_data":false,
        "absence_is_no_effect_proof":false,"upgrade_claim":upgrade_claim,"read_only_recovery_claim":claim,
        "consumed_do_not_rerun":true});
    write_new(&result_file, &serde_json::to_vec_pretty(&result).unwrap());
    println!(
        "Read-only actual19 continuation completed all retained18 inventory/catalog/ledger checks; no upgrade rerun"
    );
}
