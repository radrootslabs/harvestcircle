use core::num::NonZeroU64;
use std::fs;

use harvestcircle_application::{IdentityRepository, KeyMaterialProvider};
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, SafeErrorCode,
    SignerAvailability, UnixTimestamp,
};
use harvestcircle_nostr::NostrKeyMaterialProvider;
use harvestcircle_storage::{Database, verify_harvestcircle_backup};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::{
    BACKUP_STATE_MEMBER_NAME, BackupCreatedAtUnixMs, BackupManifestSha256, MigrationBuildIdentity,
};
use tempfile::{TempDir, tempdir_in};

fn tempdir() -> std::io::Result<TempDir> {
    tempdir_in(std::env::temp_dir().canonicalize()?)
}

fn runtime_context(directory: &TempDir) -> RuntimeContext {
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(
                directory
                    .path()
                    .canonicalize()
                    .expect("canonical directory"),
            ),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .expect("bootstrap"),
        ServiceId::new("harvestcircle").expect("service"),
        InstanceId::new("desktop").expect("instance"),
    )
    .expect("runtime context");
    fs::create_dir_all(directory.path().join("data")).expect("state root");
    context
}

fn build_identity() -> MigrationBuildIdentity {
    MigrationBuildIdentity::new(
        "0.1.0-alpha",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        "1.97.1",
        "test",
        "test",
        1,
        1,
        1,
        1,
        1,
    )
    .expect("build identity")
}

fn identity(index: i64) -> NostrIdentity {
    let (public_key, npub, secret, nsec) = NostrKeyMaterialProvider
        .generate()
        .expect("generated key material")
        .into_parts();
    drop((secret, nsec));
    NostrIdentity::new(
        NostrIdentityReference::verify(public_key, npub.as_str().to_owned())
            .expect("identity reference"),
        LocalKeyringBinding::new(public_key, SignerAvailability::Available),
        None,
        IdentityCreatedAt::new(UnixTimestamp::from_seconds(index).expect("time")),
        None,
    )
    .expect("identity")
}

fn prepare_backup_parent(directory: &TempDir) -> std::path::PathBuf {
    let parent = directory.path().join("backup-output");
    fs::create_dir(&parent).expect("backup parent");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .expect("backup parent mode");
    }
    parent
}

#[tokio::test]
async fn capture_verify_restore_round_trip_is_identity_bound_and_legacy_safe() {
    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let build = build_identity();
    let legacy = directory.path().join("harvestcircle.sqlite3");
    fs::write(&legacy, b"legacy-state-remains-untouched").expect("legacy sentinel");

    let mut database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    let retained = identity(1);
    database
        .insert_identity(&retained)
        .await
        .expect("retained identity");
    let expected = database.metadata().identity();

    let backup_parent = prepare_backup_parent(&directory);
    let bundle = backup_parent.join("bundle");
    let manifest = database
        .capture_online_backup(
            &bundle,
            BackupCreatedAtUnixMs::new(1_700_000_000_000).expect("capture time"),
        )
        .await
        .expect("capture");
    assert_eq!(
        fs::read_dir(&bundle)
            .expect("bundle inventory")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>(),
        [std::ffi::OsString::from(BACKUP_STATE_MEMBER_NAME)]
    );
    assert!(!manifest.protected_material_included());

    database
        .insert_identity(&identity(2))
        .await
        .expect("post-capture identity");
    assert_eq!(
        database
            .list_identities()
            .await
            .expect("live identities")
            .len(),
        2
    );

    let member_length = manifest.members()[0].byte_length();
    let verified = verify_harvestcircle_backup(
        manifest.canonical_bytes(),
        manifest.digest(),
        &bundle,
        &expected,
        NonZeroU64::new(member_length).expect("member length"),
    )
    .expect("verified backup");
    assert_eq!(
        format!("{verified:?}"),
        "VerifiedHarvestCircleBackup([redacted])"
    );

    database
        .restore_verified_backup(&context, verified, 2, &build)
        .await
        .expect("restore and recovery reopen");
    let identities = database
        .list_identities()
        .await
        .expect("restored identities");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].public_key(), retained.public_key());
    assert_eq!(database.metadata().identity(), expected);

    let state_directory = context.paths().state();
    for forbidden in [
        "state.restore-staged.sqlite",
        "state.restore-backup.sqlite",
        "state.restore-marker.v1",
        "state.restore-marker.v1.next",
    ] {
        assert!(
            !state_directory.join(forbidden).exists(),
            "retained {forbidden}"
        );
    }
    assert_eq!(
        fs::read(&legacy).expect("legacy sentinel"),
        b"legacy-state-remains-untouched"
    );
    database.close().await.expect("restored close");
}

#[tokio::test]
async fn wrong_manifest_digest_fails_before_restore_and_preserves_live_state() {
    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let build = build_identity();
    let database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    let retained = identity(1);
    database.insert_identity(&retained).await.expect("identity");
    let expected = database.metadata().identity();
    let bundle = prepare_backup_parent(&directory).join("bundle");
    let manifest = database
        .capture_online_backup(
            &bundle,
            BackupCreatedAtUnixMs::new(1_700_000_000_001).expect("capture time"),
        )
        .await
        .expect("capture");

    let error = verify_harvestcircle_backup(
        manifest.canonical_bytes(),
        BackupManifestSha256::from_bytes([0; 32]),
        &bundle,
        &expected,
        NonZeroU64::new(manifest.members()[0].byte_length()).expect("member length"),
    )
    .expect_err("wrong trusted digest");
    assert_eq!(error.code(), SafeErrorCode::StorageBackupInvalid);
    assert_eq!(
        database.list_identities().await.expect("live identities")[0].public_key(),
        retained.public_key()
    );
    database.close().await.expect("close");
}

#[tokio::test]
async fn mismatched_verified_identity_is_rejected_before_live_host_close() {
    let live_directory = tempdir().expect("live directory");
    let source_directory = tempdir().expect("source directory");
    let live_context = runtime_context(&live_directory);
    let source_context = runtime_context(&source_directory);
    let build = build_identity();

    let mut live = Database::open(&live_context, 1, 1, &build)
        .await
        .expect("live database");
    let retained = identity(1);
    live.insert_identity(&retained)
        .await
        .expect("live identity");

    let source = Database::open(&source_context, 1, 1, &build)
        .await
        .expect("source database");
    let source_identity = source.metadata().identity();
    let bundle = prepare_backup_parent(&source_directory).join("bundle");
    let manifest = source
        .capture_online_backup(
            &bundle,
            BackupCreatedAtUnixMs::new(1_700_000_000_002).expect("capture time"),
        )
        .await
        .expect("capture");
    let verified = verify_harvestcircle_backup(
        manifest.canonical_bytes(),
        manifest.digest(),
        &bundle,
        &source_identity,
        NonZeroU64::new(manifest.members()[0].byte_length()).expect("member length"),
    )
    .expect("verified source backup");

    let error = live
        .restore_verified_backup(&live_context, verified, 2, &build)
        .await
        .expect_err("mismatched source generation");
    assert_eq!(error.code(), SafeErrorCode::InvalidApplicationState);
    assert_eq!(
        live.list_identities()
            .await
            .expect("live host remains open")[0]
            .public_key(),
        retained.public_key()
    );
    live.close().await.expect("live close");
    source.close().await.expect("source close");
}
