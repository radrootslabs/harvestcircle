use std::fs;

use harvestcircle_application::{
    Clock, DurableRequestId, IdentityNamespaceRepository, IdentityPreferenceKey,
    InMemorySecretStore, RelayConfiguration, SessionState,
};
use harvestcircle_domain::{SecretKeyInput, UnixTimestamp};
use harvestcircle_runtime::PersistentAppCore;
use harvestcircle_storage::HarvestCircleStorageContract;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use tempfile::tempdir;

const SECRET_A: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
const SECRET_B: &str = "0101010101010101010101010101010101010101010101010101010101010101";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(200).expect("fixed timestamp")
    }
}

fn context(root: &std::path::Path) -> RuntimeContext {
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(root.canonicalize().expect("canonical root")),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .expect("bootstrap"),
        ServiceId::new("harvestcircle").expect("service"),
        InstanceId::new("desktop").expect("instance"),
    )
    .expect("context");
    fs::create_dir_all(root.join("data")).expect("state root");
    context
}

fn build() -> MigrationBuildIdentity {
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
    .expect("build")
}

#[tokio::test]
async fn restart_restores_selection_and_keeps_identity_namespaces_isolated() {
    let directory = tempdir().expect("temporary directory");
    let context = context(directory.path());
    let build = build();
    let path = HarvestCircleStorageContract::from_runtime_context(&context)
        .expect("contract")
        .paths()
        .state_database()
        .to_path_buf();
    let secrets = InMemorySecretStore::default();
    let (owner_a, owner_b);

    {
        let adapter = PersistentAppCore::open(
            &context,
            RelayConfiguration::default(),
            200_000,
            200,
            &build,
        )
        .await
        .expect("persistent adapter");
        adapter
            .bootstrap(&secrets, &FixedClock)
            .await
            .expect("bootstrap");
        owner_a = adapter
            .import_secret_key_durable(
                &DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000101").expect("request"),
                adapter.core().snapshot().revision().value(),
                SecretKeyInput::parse(SECRET_A.to_owned()).expect("secret A"),
                &secrets,
                &FixedClock,
            )
            .await
            .expect("identity A")
            .identity()
            .public_key();
        owner_b = adapter
            .import_secret_key_durable(
                &DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000102").expect("request"),
                adapter.core().snapshot().revision().value(),
                SecretKeyInput::parse(SECRET_B.to_owned()).expect("secret B"),
                &secrets,
                &FixedClock,
            )
            .await
            .expect("identity B")
            .identity()
            .public_key();
        adapter
            .database()
            .set_value(owner_a, IdentityPreferenceKey::NamespaceProbe, "identity-a")
            .await
            .expect("namespace A");
        adapter
            .database()
            .set_value(owner_b, IdentityPreferenceKey::NamespaceProbe, "identity-b")
            .await
            .expect("namespace B");
        adapter.select_identity(owner_b).await.expect("select B");
        adapter.close().await.expect("close");
    }

    let reopened = PersistentAppCore::open(
        &context,
        RelayConfiguration::default(),
        200_000,
        200,
        &build,
    )
    .await
    .expect("reopen adapter");
    let restored = reopened
        .bootstrap(&secrets, &FixedClock)
        .await
        .expect("restore");
    assert_eq!(restored.identities().len(), 2);
    assert_eq!(restored.selected_identity(), Some(owner_b));
    assert_eq!(restored.session(), SessionState::SignedOut);
    assert_eq!(
        reopened
            .database()
            .get_value(owner_a, IdentityPreferenceKey::NamespaceProbe)
            .await
            .expect("read A"),
        Some("identity-a".to_owned())
    );
    assert_eq!(
        reopened
            .database()
            .get_value(owner_b, IdentityPreferenceKey::NamespaceProbe)
            .await
            .expect("read B"),
        Some("identity-b".to_owned())
    );

    reopened.close().await.expect("close reopened");
    let database = fs::read(path).expect("database bytes");
    assert!(
        !database
            .windows(SECRET_A.len())
            .any(|bytes| bytes == SECRET_A.as_bytes())
    );
    assert!(
        !database
            .windows(SECRET_B.len())
            .any(|bytes| bytes == SECRET_B.as_bytes())
    );
    assert!(!database.windows(5).any(|bytes| bytes == b"nsec1"));
}
