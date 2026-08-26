use std::fs;

use harvestcircle_application::IdentityRepository;
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
    SignerAvailability, UnixTimestamp,
};
use harvestcircle_storage::Database;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use tempfile::{TempDir, tempdir_in};

fn tempdir() -> std::io::Result<TempDir> {
    tempdir_in(std::env::temp_dir().canonicalize()?)
}

const SECRET_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SECRET_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
fn assert_redacted(bytes: &[u8]) {
    assert!(
        !bytes
            .windows(SECRET_HEX.len())
            .any(|value| value == SECRET_HEX.as_bytes())
    );
    assert!(
        !bytes
            .windows(SECRET_NSEC.len())
            .any(|value| value == SECRET_NSEC.as_bytes())
    );
    assert!(!bytes.windows(5).any(|value| value == b"nsec1"));
}

fn runtime_context(directory: &TempDir) -> RuntimeContext {
    RuntimeContext::resolve(
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
    .expect("runtime context")
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

#[tokio::test]
async fn redaction_guards_sqlite_schema_and_non_secret_records() {
    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let path = context.paths().state().join("state.sqlite");
    {
        let database = Database::open(&context, 1, 1, &build_identity())
            .await
            .expect("database");
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        let identity = NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
        .expect("identity");
        database.insert_identity(&identity).await.expect("identity");
        database.close().await.expect("close");
    }
    assert_redacted(&fs::read(path).expect("database bytes"));
}
