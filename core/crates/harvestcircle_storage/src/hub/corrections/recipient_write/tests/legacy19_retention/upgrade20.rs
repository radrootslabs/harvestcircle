//! One-use actual19 consumers. Never begin a runtime or execute saved input.
use super::*;
use crate::hub::corrections::upgrade_tests::{
    all_objects, digest, health, inventory, published19_objects, reader, verify_objects, write_new,
};
use crate::hub::stock::upgrade_tests::original_rows;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BASE: &str = "8ad710f423a4134e3f34bb9a8b2551c1e9fef907";
const FEATURE: &str = "actual19-to20-original-preservation";
const SPARSE: &str = "harvestcircle-schema19-before-actual-interpretations-v1";
const POPULATED: &str = "harvestcircle-populated19-before-actual-interpretations-v2";
const FAILED: &str = "harvestcircle-populated19-before-actual-interpretations-v1";
mod gate;

fn build20() -> MigrationBuildIdentity {
    MigrationBuildIdentity::new(
        "0.1.0-alpha",
        BASE,
        LIB,
        "1.99.0",
        "aarch64-apple-darwin",
        FEATURE,
        1,
        20,
        1,
        1,
        1,
    )
    .unwrap()
}
fn context(profile: &Path, state: &Path) -> RuntimeContext {
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(profile.to_path_buf()),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .unwrap(),
        ServiceId::new("harvestcircle").unwrap(),
        InstanceId::new("desktop").unwrap(),
    )
    .unwrap();
    assert_eq!(
        crate::HarvestCircleStorageContract::from_runtime_context(&context)
            .unwrap()
            .paths()
            .state_database(),
        state
    );
    context
}
fn closed(state: &Path) {
    match fs::symlink_metadata(state.with_extension("sqlite-wal")) {
        Ok(meta) => assert!(meta.is_file() && meta.len() == 0),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
    }
}
fn objects20() -> Vec<radroots_service_sqlite::SchemaObject> {
    let mut objects = published19_objects();
    objects.retain(|o| {
        !crate::hub::actuals::interpretation_schema::REPLACED_OBJECTS.contains(&o.name())
    });
    objects.extend(crate::hub::actuals::interpretation_schema::schema_objects().unwrap());
    assert_eq!(objects.len(), 265);
    assert_eq!(
        radroots_service_sqlite::SchemaVersionCatalog::computed_digest(20, objects.clone())
            .unwrap()
            .as_bytes(),
        &crate::hub::actuals::interpretation_schema::SCHEMA_DIGEST
    );
    objects
}
async fn metadata_version(raw: &mut SqliteConnection, version: i64) {
    let versions: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT singleton,state_schema_version FROM radroots_service_metadata ORDER BY singleton",
    )
    .fetch_all(raw)
    .await
    .unwrap();
    assert_eq!(versions, vec![(1, version)]);
}
async fn migrations(raw: &mut SqliteConnection) -> Vec<(i64, String, Vec<u8>)> {
    sqlx::query_as("SELECT version,name,checksum FROM schema_migrations ORDER BY version")
        .fetch_all(raw)
        .await
        .unwrap()
}
async fn cut(raw: &mut SqliteConnection) -> Vec<(Vec<u8>, i64, i64)> {
    sqlx::query_as("SELECT s.hub_id,s.revision,COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=s.hub_id),1) FROM hub_stock_state s ORDER BY s.hub_id").fetch_all(raw).await.unwrap()
}
async fn latest20(raw: &mut SqliteConnection) {
    metadata_version(raw, 20).await;
    let row = sqlx::query("SELECT * FROM schema_migrations WHERE version=20")
        .fetch_one(raw)
        .await
        .unwrap();
    assert_eq!(
        row.get::<String, _>("name"),
        "bind_actual_interpretation_epochs"
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("checksum"),
        crate::hub::actuals::interpretation_schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(row.get::<i64, _>("applied_at_unix_s"), 1000);
    for (field, value) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", BASE),
        ("lib_revision", LIB),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        ("feature_profile", FEATURE),
    ] {
        assert_eq!(row.get::<String, _>(field), value);
    }
    for (field, value) in [
        ("config_contract_version", 1),
        ("state_contract_version", 20),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(row.get::<i64, _>(field), value);
    }
}
async fn originals(db: &Database, name: &str) -> serde_json::Value {
    if name == POPULATED {
        return native_baseline(db).await;
    }
    let runtime = db.hub_runtime_session().await.unwrap().map(|s|serde_json::json!({"runtime":s.runtime,"generation":s.generation.value(),"actor":s.actor}));
    let pending = db
        .hub_submission(owner(), operation(if name == SPARSE { 4 } else { 30011 }))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    let expected = if name == SPARSE {
        "dd71489003a223647d2530c8662627bb823c2fb403b565984c3e681c4e48c7fa"
    } else {
        "7c63ef33c918daec42880f99b850032bb5930d271245c8a65b786225cd386d82"
    };
    assert_eq!(
        format!("{:x}", Sha256::digest(pending.captured_semantics())),
        expected
    );
    let pending_value = serde_json::json!({"operation":pending.operation.as_str(),"scope":pending.scope,"kind":pending.kind,"capture_sha256":expected,"pending":true});
    if name == SPARSE {
        assert_eq!(pending.kind, "count_observe");
        let receipt = db
            .hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .unwrap();
        let opening = db
            .hub_opening_operation(owner(), operation(2))
            .await
            .unwrap()
            .unwrap();
        let count = db
            .hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.original.trace.original_packs.value(), 20);
        assert_eq!(receipt.original.put_away.remaining().value(), 20);
        assert_eq!(opening.original.population.original_packs.value(), 8);
        assert_eq!(count.original.counted_packs.value(), 18);
        assert_eq!(
            db.hub_stock_page(owner(), query())
                .await
                .unwrap()
                .rows
                .iter()
                .map(|r| r.position.packs.value())
                .sum::<u32>(),
            28
        );
        serde_json::json!({"runtime":runtime,"pending":pending_value,"receipt_original_packs":receipt.original.trace.original_packs.value(),"opening_original_packs":opening.original.population.original_packs.value(),"counted_packs":count.original.counted_packs.value(),"full_typed_originals_compared_across_native_reopen":true})
    } else {
        assert_eq!(pending.kind, "correction_found");
        assert_eq!(
            serde_json::to_value(pending.scope.runtime).unwrap(),
            "b98cd66d5302b20892b8634c01be1903"
        );
        assert_eq!(pending.scope.generation.value(), 1);
        assert!(
            db.hub_found_correction_original(owner(), operation(30011))
                .await
                .unwrap()
                .is_none()
        );
        let actual = db
            .hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap()
            .unwrap();
        let link = db
            .hub_actual_review_operation(owner(), operation(LINK))
            .await
            .unwrap()
            .unwrap();
        let additional = db
            .hub_additional_quantity_original(owner(), operation(ADDITIONAL))
            .await
            .unwrap()
            .unwrap();
        let actual = actual_value(&actual);
        let link = actual_value(&link);
        assert!(actual["plan"].get("interpretations").is_none());
        assert!(link["plan"].get("interpretation").is_none());
        serde_json::json!({"runtime":runtime,"pending":pending_value,"actual":actual,"link":link,"additional":additional.original,"missing_root_is_no_effect_proof":false})
    }
}
async fn sparse_probes(
    db: &Database,
) -> (
    crate::HubStockReceipt,
    crate::HubOpeningReceipt,
    crate::HubCountObservationReceipt,
) {
    (
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .unwrap(),
        db.hub_opening_operation(owner(), operation(2))
            .await
            .unwrap()
            .unwrap(),
        db.hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap()
            .unwrap(),
    )
}
async fn native_reopen(native_context: &RuntimeContext, name: &str) -> serde_json::Value {
    let db = Database::open(native_context, 99, 1000, &build20())
        .await
        .unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 20);
    let sparse = if name == SPARSE {
        Some(sparse_probes(&db).await)
    } else {
        None
    };
    let runtime = db.hub_runtime_session().await.unwrap();
    let pending = if name != POPULATED {
        db.hub_submission(owner(), operation(if name == SPARSE { 4 } else { 30011 }))
            .await
            .unwrap()
    } else {
        None
    };
    let original = Box::pin(originals(&db, name)).await;
    db.close().await.unwrap();
    let db = Database::open(native_context, 99, 1100, &build20())
        .await
        .unwrap();
    assert_eq!(Box::pin(originals(&db, name)).await, original);
    assert_eq!(db.hub_runtime_session().await.unwrap(), runtime);
    if name == SPARSE {
        assert_eq!(Some(sparse_probes(&db).await), sparse);
    }
    if name != POPULATED {
        assert!(
            db.hub_submission(owner(), operation(if name == SPARSE { 4 } else { 30011 }))
                .await
                .unwrap()
                == pending
        );
    }
    db.close().await.unwrap();
    original
}

#[tokio::test]
async fn first20_heavy_native_reopen_caller_qualifies_on_disposable_default_stack() {
    let mut fixture = Fixture::new();
    fixture.build = build20();
    let db = fixture.open().await;
    let (expected, _, _) = Box::pin(drive_native_observed(&db, false)).await;
    let actual = db
        .hub_actual_review_operation(owner(), operation(81))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = actual.original else {
        panic!("physical original")
    };
    let funding = db
        .hub_correction_target_funding(
            owner(),
            crate::HubCorrectionFundingQuery {
                target_operation: operation(81),
                original_group: plan.reduction.before.id,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(funding.quantity.effective().value(), 0);
    assert_eq!(funding.debits.len(), 2);
    assert_eq!(
        funding
            .debits
            .iter()
            .map(|d| d.recovered().value())
            .sum::<u32>(),
        4
    );
    assert!(
        funding
            .debits
            .iter()
            .all(|d| d.available().unwrap().value() == 0)
    );
    db.close().await.unwrap();
    assert_eq!(
        Box::pin(native_reopen(&fixture.context, POPULATED)).await,
        expected
    );
}

#[tokio::test]
async fn first20_inspector_checks_real_build_fields_catalog_inventory_and_clean_reopen() {
    let fixture = Fixture::new();
    let db = Database::open(&fixture.context, 99, 1000, &build20())
        .await
        .unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 20);
    db.close().await.unwrap();
    let state = fixture.context.paths().state().join("state.sqlite");
    closed(&state);
    let mut raw = reader(&state).await;
    latest20(&mut raw).await;
    health(&mut raw).await;
    let before = inventory(&mut raw).await;
    assert_eq!(before.as_array().unwrap().len(), 71);
    let objects = objects20();
    verify_objects(&mut raw, &objects).await;
    assert_eq!(all_objects(&mut raw).await.len(), 271);
    raw.close().await.unwrap();
    let db = Database::open(&fixture.context, 99, 1100, &build20())
        .await
        .unwrap();
    db.close().await.unwrap();
    let mut raw = reader(&state).await;
    assert_eq!(inventory(&mut raw).await, before);
    raw.close().await.unwrap();
}

async fn consume(name: &str, output_name: &str) {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 20);
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let input = retained.join(name).canonicalize().unwrap();
    assert_eq!(input.parent(), Some(retained.as_path()));
    let failed = name == FAILED;
    let witness_path = input.join(if failed {
        "profile-custody.json"
    } else {
        "witness.json"
    });
    let witness_hash = match name {
        SPARSE => "bf4d95a38a6e66590b9e0f2887bd104affdd703d7fe9c5a8e037fcf61df58b52",
        POPULATED => "5ee2787b641d1a38efa3d616c6fb927eb426090acd374e4c8cde45a63c6ea7cc",
        FAILED => "9b2fe35e10dbda52498218c4e76bc8177154dcb21288abda280e1be40a9732a6",
        _ => panic!("not a designated original world"),
    };
    assert_eq!(digest(&witness_path), witness_hash);
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_path).unwrap()).unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(profile.starts_with(&retained) && state.starts_with(&profile));
    let accepted_profile = match name {
        SPARSE => retained.join("harvestcircle-schema11-before-counts-v1/.tmpl3JIz8"),
        POPULATED => input.join(".tmptF8Cge"),
        FAILED => input.join(".tmpB5sWYp"),
        _ => unreachable!(),
    };
    assert_eq!(profile, accepted_profile);
    assert_eq!(
        state,
        profile.join("data/services/harvestcircle/desktop/state.sqlite")
    );
    assert!(fs::symlink_metadata(&state).unwrap().is_file());
    let lock_path = state.with_extension("lock");
    assert!(fs::symlink_metadata(&lock_path).unwrap().is_file());
    let writer_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    writer_lock
        .try_lock()
        .expect("original native writer must be closed");
    let objects20 = objects20();
    let output = retained.join(output_name);
    let claim = input.join("actual-schema20-upgrade-1.claim");
    assert!(!output.exists() && !claim.exists());
    let native_context = context(&profile, &state);
    let qualified = PathBuf::from(
        std::env::var_os("HCF20_QUALIFIED_INPUTS")
            .expect("retain qualified inputs before any original open"),
    )
    .canonicalize()
    .unwrap();
    assert!(qualified.starts_with(&retained));
    let qualified_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&qualified).unwrap()).unwrap();
    assert_eq!(
        qualified_manifest["executable_sha256"].as_str().unwrap(),
        digest(&std::env::current_exe().unwrap())
    );
    assert_eq!(qualified_manifest["source_base"], BASE);
    gate::verify(&qualified, name).expect(
        "every source/input/artifact seal and exact consumer must still match before the claim",
    );
    // Only immutable, closed originals or a WAL-aware disposable forensic copy
    // are inspected before the claim. Original failed WAL/SHM/lock stay untouched.
    let (read_path, archive, physical) = if failed {
        let pristine = input.join("first-stack-overflow-diagnostic");
        assert_eq!(
            digest(&pristine.join("profile-files.json")),
            "6df71ed80e9b66535e5474e7a6fa62c191d3ef9440860d5b2bea974f19967a46"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(pristine.join("profile-files.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["files"].as_array().unwrap().len(), 4);
        let copy = tempfile::tempdir_in(project.join("tmp/general")).unwrap();
        for entry in manifest["files"].as_array().unwrap() {
            let relative = entry["path"].as_str().unwrap();
            assert!(relative.starts_with("data/services/harvestcircle/desktop/state."));
            let original = profile.join(relative);
            let preserved = pristine.join("profile-snapshot").join(relative);
            assert_eq!(digest(&original), entry["sha256"].as_str().unwrap());
            assert_eq!(digest(&preserved), entry["sha256"].as_str().unwrap());
            let target = copy.path().join(Path::new(relative).file_name().unwrap());
            write_new(&target, &fs::read(original).unwrap());
        }
        let copy = copy.keep();
        (copy.join("state.sqlite"), None, manifest)
    } else {
        assert_eq!(witness["schema"], 19);
        let expected = if name == SPARSE {
            "7964a03bbf5d1162e893c98b4d156cff401a3156e55bd45ebca06f9cc886cdb7"
        } else {
            "c7d5dfe5266458f7ef6feed20ca19016784e5b3f2524b549a81243d3c420fb79"
        };
        let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
            .canonicalize()
            .unwrap();
        assert_eq!(archive, input.join("original-schema19.sqlite"));
        assert!(fs::symlink_metadata(&archive).unwrap().is_file());
        assert_eq!(digest(&state), expected);
        assert_eq!(digest(&archive), expected);
        closed(&state);
        (
            archive.clone(),
            Some(archive),
            serde_json::json!({"main_sha256":expected}),
        )
    };
    let mut raw = if failed {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&read_path)
                .create_if_missing(false)
                .read_only(true),
        )
        .await
        .unwrap()
    } else {
        reader(&read_path).await
    };
    health(&mut raw).await;
    metadata_version(&mut raw, 19).await;
    verify_objects(&mut raw, &published19_objects()).await;
    let before = inventory(&mut raw).await;
    assert_eq!(before.as_array().unwrap().len(), 69);
    if !failed {
        assert_eq!(before, witness["tables"]);
    }
    let before_originals = original_rows(&mut raw, 19).await;
    let before_objects = all_objects(&mut raw).await;
    assert_eq!(before_objects.len(), 262);
    let before_ledger = migrations(&mut raw).await;
    assert_eq!(
        before_ledger.iter().map(|r| r.0).collect::<Vec<_>>(),
        (2..=19).collect::<Vec<_>>()
    );
    let activation_cut = cut(&mut raw).await;
    raw.close().await.unwrap();
    // Check the failed world's real files again after forensic WAL recovery.
    if failed {
        for entry in physical["files"].as_array().unwrap() {
            assert_eq!(
                digest(&profile.join(entry["path"].as_str().unwrap())),
                entry["sha256"].as_str().unwrap()
            );
        }
    } else {
        assert_eq!(digest(&state), witness["original_sha256"].as_str().unwrap());
        assert_eq!(
            digest(archive.as_ref().unwrap()),
            witness["original_sha256"].as_str().unwrap()
        );
        closed(&state);
    }
    write_new(&claim,b"One-use actual19->20 native migration. Failure also consumes attempt. No saved input execution.\n");
    fs::create_dir(&output).unwrap();
    write_new(&output.join("before.json"),&serde_json::to_vec_pretty(&serde_json::json!({"tables":before,"objects":before_objects,"physical":physical,"activation_cut":activation_cut,"read_only_input":read_path,"qualified_inputs":qualified,"qualified_inputs_sha256":digest(&qualified)})).unwrap());
    writer_lock.unlock().unwrap();
    drop(writer_lock);
    let native_originals = Box::pin(native_reopen(&native_context, name)).await;
    if name == POPULATED {
        assert_eq!(native_originals, witness["native_original_baseline"]);
    }
    closed(&state);
    let mut raw = reader(&state).await;
    health(&mut raw).await;
    assert_eq!(original_rows(&mut raw, 19).await, before_originals);
    let after = inventory(&mut raw).await;
    assert_eq!(after.as_array().unwrap().len(), 71);
    for old in before.as_array().unwrap() {
        if !matches!(
            old["table"].as_str().unwrap(),
            "schema_migrations" | "radroots_service_metadata"
        ) {
            assert_eq!(
                after
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["table"] == old["table"])
                    .unwrap(),
                old
            );
        }
    }
    let epoch: Vec<(Vec<u8>,i64,i64)> = sqlx::query_as("SELECT hub_id,legacy_stock_revision,legacy_catalog_revision FROM hub_actual_interpretation_epochs WHERE epoch=1 ORDER BY hub_id").fetch_all(&mut raw).await.unwrap();
    assert_eq!(epoch, activation_cut);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hub_actual_interpretation_markers")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        0
    );
    verify_objects(&mut raw, &objects20).await;
    let after_objects = all_objects(&mut raw).await;
    assert_eq!(after_objects.len(), 271);
    for (name, sql) in &before_objects {
        if name != "hub_actual_review_guard" {
            assert_eq!(
                &after_objects.iter().find(|(n, _)| n == name).unwrap().1,
                sql
            );
        }
    }
    let ledger = migrations(&mut raw).await;
    assert_eq!(&ledger[..before_ledger.len()], before_ledger.as_slice());
    assert_eq!(ledger.len(), 19);
    latest20(&mut raw).await;
    raw.close().await.unwrap();
    let archive20 = output.join("original-schema20.sqlite");
    write_new(&archive20, &fs::read(&state).unwrap());
    assert_eq!(digest(&state), digest(&archive20));
    let mut copied = reader(&archive20).await;
    health(&mut copied).await;
    assert_eq!(inventory(&mut copied).await, after);
    verify_objects(&mut copied, &objects20).await;
    copied.close().await.unwrap();
    if let Some(archive) = archive {
        assert_eq!(
            digest(&archive),
            witness["original_sha256"].as_str().unwrap()
        );
    }
    let mut physical_after = Vec::new();
    for path in [
        state.clone(),
        state.with_extension("sqlite-wal"),
        state.with_extension("sqlite-shm"),
        lock_path,
    ] {
        let value = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                assert!(meta.is_file());
                serde_json::json!({"path":path,"bytes":meta.len(),"sha256":digest(&path)})
            }
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                serde_json::json!({"path":path,"absent":true})
            }
        };
        physical_after.push(value);
    }
    if failed {
        for entry in physical["files"].as_array().unwrap() {
            let preserved = input
                .join("first-stack-overflow-diagnostic/profile-snapshot")
                .join(entry["path"].as_str().unwrap());
            assert_eq!(digest(&preserved), entry["sha256"].as_str().unwrap());
        }
    }
    write_new(&output.join("physical-after.json"),&serde_json::to_vec_pretty(&serde_json::json!({"files":physical_after,"native_migration_may_checkpoint_original_wal":failed,"pristine_failed_bundle_unchanged":failed,"no_manual_checkpoint_or_sidecar_removal":true})).unwrap());
    write_new(&output.join("witness.json"),&serde_json::to_vec_pretty(&serde_json::json!({"schema":20,"actual_predecessor_schema":19,"input":input,"predecessor_witness":witness_path,"predecessor_witness_sha256":digest(&witness_path),"state_file":state,"profile_source_root":profile,"original_archive":archive20,"original_sha256":digest(&archive20),"tables":after,"schema_objects":after_objects,"native_original_baseline":native_originals,"old_rows_and_migration_build_identities_unchanged":true,"all_new_migration_build_identity_fields_checked":true,"legacy_originals_have_no_interpretation_markers":true,"runtime_and_pending_unchanged":true,"no_runtime_begin_or_saved_input_execution":true,"absence_is_no_effect_proof":false,"source_base":BASE,"source_harness_may_be_uncommitted":true,"claim":claim,"consumed_do_not_rerun":true})).unwrap());
}

#[tokio::test]
#[ignore = "One-use sparse original19->20; never rerun or replace retained world"]
async fn retained_sparse19_first20_preserves_all_originals_and_pending() {
    Box::pin(consume(
        SPARSE,
        "harvestcircle-sparse20-after-actual-interpretations-v1",
    ))
    .await;
}
#[tokio::test]
#[ignore = "One-use populated original19->20; never rerun or replace retained world"]
async fn retained_populated19_first20_preserves_legacy_meaning_and_pending() {
    Box::pin(consume(
        POPULATED,
        "harvestcircle-populated20-after-actual-interpretations-v2",
    ))
    .await;
}
#[tokio::test]
#[ignore = "One-use failed WAL-bearing original19->20; preserve unknown and pristine bundle"]
async fn retained_failed19_first20_preserves_wal_world_and_unknown() {
    Box::pin(consume(
        FAILED,
        "harvestcircle-failed20-after-actual-interpretations-v1",
    ))
    .await;
}
