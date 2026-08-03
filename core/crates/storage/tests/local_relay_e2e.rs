use std::time::Duration;

use nostr::{EventBuilder, Keys, Metadata};
use nostr_relay_builder::MockRelay;
use nostr_sdk::Client;
use radroots_studio_application::{
    Clock, InMemorySecretStore, ProfileLoadState, ProfileRepository, RelayConfiguration,
    RelayConnectionState, SdkNostrClient, SecretStore, SessionState,
};
use radroots_studio_domain::{RelayUrl, SecretKeyInput, UnixTimestamp};
use radroots_studio_storage::PersistentAppCore;

const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(100).expect("fixed timestamp")
    }
}

#[tokio::test]
async fn local_relay_e2e_imports_activates_refreshes_and_caches_profile() {
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
                .display_name("Farm Account")
                .about("Local food profile"),
        ))
        .await
        .expect("publish profile");

    let relay = RelayUrl::parse(relay_url.as_str()).expect("relay URL");
    let adapter = PersistentAppCore::in_memory(RelayConfiguration::new(vec![relay]))
        .expect("persistent adapter");
    let secrets = InMemorySecretStore::default();
    adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
    let imported = adapter
        .import_secret_key(
            SecretKeyInput::parse(SECRET_HEX.to_owned()).expect("secret input"),
            &secrets,
            &FixedClock,
        )
        .expect("import account");
    let public_key = imported.account().public_key();
    assert!(secrets.contains(public_key).expect("credential exists"));
    adapter
        .activate_account(public_key, &secrets, &FixedClock)
        .expect("activate account");

    let refreshed = adapter
        .core()
        .refresh_active_profile(
            adapter.database(),
            &SdkNostrClient::new(Duration::from_secs(2)),
            &FixedClock,
        )
        .await
        .expect("refresh profile");

    assert_eq!(refreshed.session(), SessionState::Active);
    let active = refreshed.active_account().expect("active account");
    assert_eq!(active.relay_state(), RelayConnectionState::Connected);
    assert_eq!(active.profile_state(), ProfileLoadState::Fresh);
    assert_eq!(
        active.profile().and_then(|profile| profile.display_name()),
        Some("Farm Account")
    );
    let cached = adapter
        .database()
        .load_profile(public_key)
        .expect("load cache")
        .expect("cached profile");
    assert_eq!(
        cached.candidate().metadata().preferred_name(),
        Some("Farm Account")
    );
    let public_debug = format!("{refreshed:?}");
    assert!(!public_debug.contains(SECRET_HEX));
    assert!(!public_debug.contains("nsec1"));
    publisher.shutdown().await;
    local_relay.shutdown();
}
