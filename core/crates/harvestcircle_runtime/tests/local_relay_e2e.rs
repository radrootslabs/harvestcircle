use std::time::Duration;

use harvestcircle_application::{
    Clock, DurableRequestId, InMemorySecretStore, ProfileLoadState, ProfileRepository,
    RelayConfiguration, RelayConnectionState, SecretStore, SessionState,
};
use harvestcircle_domain::{SecretKeyInput, UnixTimestamp};
use harvestcircle_nostr::SdkNostrClient;
use harvestcircle_runtime::PersistentAppCore;
use nostr::{EventBuilder, Keys, Metadata};
use nostr_relay_builder::MockRelay;
use nostr_sdk::Client;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use radroots_transport_nostr::{RelayAccess, RelayEndpoint, RelayUrlPolicy};

const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(100).expect("fixed timestamp")
    }
}

#[tokio::test]
async fn local_relay_e2e_imports_activates_refreshes_and_caches_profile() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(directory.path().canonicalize().expect("canonical root")),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .expect("bootstrap"),
        ServiceId::new("harvestcircle").expect("service"),
        InstanceId::new("desktop").expect("instance"),
    )
    .expect("context");
    std::fs::create_dir_all(directory.path().join("data")).expect("state root");
    let build = MigrationBuildIdentity::new(
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
    .expect("build");
    let local_relay = MockRelay::run().await.expect("local relay");
    let relay_url = local_relay.url().await;
    let keys = Keys::parse(SECRET_HEX).expect("known secret key");
    let publisher = Client::new(keys);
    publisher
        .add_relay(relay_url.clone())
        .await
        .expect("publisher relay");
    publisher.connect().await;
    publisher.wait_for_connection(Duration::from_secs(2)).await;
    publisher
        .send_event_builder(EventBuilder::metadata(
            &Metadata::new()
                .name("farmer")
                .display_name("Farm Identity")
                .about("Local food profile"),
        ))
        .await
        .expect("publish profile");

    let relay = RelayEndpoint::new(
        relay_url.as_str(),
        RelayUrlPolicy::Local,
        RelayAccess::ReadWrite,
    )
    .expect("relay endpoint");
    let adapter = PersistentAppCore::open(
        &context,
        RelayConfiguration::new(vec![relay]).expect("relay configuration"),
        100_000,
        100,
        &build,
    )
    .await
    .expect("persistent adapter");
    let secrets = InMemorySecretStore::default();
    adapter
        .bootstrap(&secrets, &FixedClock)
        .await
        .expect("bootstrap");
    let imported = adapter
        .import_secret_key_durable(
            &DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000201").expect("request"),
            adapter.core().snapshot().revision().value(),
            SecretKeyInput::parse(SECRET_HEX.to_owned()).expect("secret input"),
            &secrets,
            &FixedClock,
        )
        .await
        .expect("import identity");
    let public_key = imported.identity().public_key();
    assert!(secrets.contains(public_key).expect("credential exists"));
    adapter
        .activate_identity(public_key, &secrets, &FixedClock)
        .await
        .expect("activate identity");

    let refreshed = adapter
        .core()
        .refresh_active_profile(
            adapter.database(),
            &SdkNostrClient::new(Duration::from_secs(2)),
            &FixedClock,
            std::time::Instant::now() + Duration::from_secs(2),
        )
        .await
        .expect("refresh profile");

    assert_eq!(refreshed.session(), SessionState::Active);
    let active = refreshed.active_identity().expect("active identity");
    assert_eq!(active.relay_state(), RelayConnectionState::Connected);
    assert_eq!(active.profile_state(), ProfileLoadState::Fresh);
    assert_eq!(
        active.profile().and_then(|profile| profile.display_name()),
        Some("Farm Identity")
    );
    let cached = adapter
        .database()
        .load_profile(public_key)
        .await
        .expect("load cache")
        .expect("cached profile");
    assert_eq!(
        cached.candidate().metadata().preferred_name(),
        Some("Farm Identity")
    );
    let public_debug = format!("{refreshed:?}");
    assert!(!public_debug.contains(SECRET_HEX));
    assert!(!public_debug.contains("nsec1"));
    adapter.close().await.expect("close");
    publisher.shutdown().await;
    local_relay.shutdown();
}
