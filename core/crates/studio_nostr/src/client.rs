use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use radroots_studio_domain::{
    PublicKey, RelayDestinationPolicy, RelayUrl, SafeError, SafeErrorCode, SafeMessage,
    select_latest_kind0,
};
use radroots_transport::{
    EventSource, FetchRequest, Target, TargetSet,
    outcome::FetchTargetState,
    source::{FetchBounds, FetchSelector},
};
use radroots_transport_nostr::{Config, NostrTransport, RelayUrlPolicy};

use radroots_studio_application::{
    BoxFuture, MAX_CONFIGURED_RELAYS, NostrClient, ProfileFetchResult,
};

pub struct SdkNostrClient {
    timeout: Duration,
}

const MAX_PROFILE_EVENTS_PER_RELAY: usize = 64;

impl SdkNostrClient {
    #[must_use]
    pub const fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl NostrClient for SdkNostrClient {
    fn fetch_profile<'a>(
        &'a self,
        public_key: PublicKey,
        relays: &'a [RelayUrl],
        deadline: Instant,
    ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>> {
        Box::pin(async move {
            if relays.is_empty() {
                return Err(invalid_relay_configuration());
            }
            if relays.len() > MAX_CONFIGURED_RELAYS {
                return Err(invalid_relay_configuration());
            }

            let author = public_key.canonical();
            let deadline = deadline.min(Instant::now() + self.timeout);
            let mut candidates = Vec::new();
            let mut successful_relays = 0usize;
            for policy in [
                RelayDestinationPolicy::Public,
                RelayDestinationPolicy::Local,
                RelayDestinationPolicy::PrivateNetwork,
            ] {
                let policy_relays = relays
                    .iter()
                    .filter(|relay| relay.policy() == policy)
                    .collect::<Vec<_>>();
                if policy_relays.is_empty() {
                    continue;
                }
                let config = Config::new(
                    canonical_policy(policy),
                    policy_relays.iter().map(|relay| relay.as_str()),
                )
                .and_then(|config| {
                    let timeout_ms =
                        timeout_millis(deadline.saturating_duration_since(Instant::now()));
                    config.with_timeouts(timeout_ms, timeout_ms, timeout_ms)
                })
                .map_err(|_| invalid_relay_configuration())?;
                let targets = policy_relays
                    .iter()
                    .map(|relay| Target::nostr_relay(relay.as_str()))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| invalid_relay_configuration())?;
                let request = FetchRequest::new(
                    format!("studio-profile-{policy:?}"),
                    TargetSet::new(targets).map_err(|_| invalid_relay_configuration())?,
                    FetchBounds::new(
                        MAX_PROFILE_EVENTS_PER_RELAY as u16,
                        unix_deadline(deadline.saturating_duration_since(Instant::now()))?,
                    )
                    .map_err(|_| invalid_relay_configuration())?,
                )
                .map_err(|_| invalid_relay_configuration())?
                .with_selector(
                    FetchSelector::all()
                        .with_kinds(vec![0])
                        .and_then(|selector| selector.with_authors(vec![author]))
                        .map_err(|_| invalid_relay_configuration())?,
                );
                let page = tokio::time::timeout_at(
                    deadline.into(),
                    NostrTransport::new(config).fetch(request),
                )
                .await
                .map_err(|_| relay_connection_failed())?
                .map_err(|_| relay_connection_failed())?;
                successful_relays += page
                    .target_outcomes()
                    .iter()
                    .filter(|outcome| {
                        matches!(
                            outcome.state(),
                            FetchTargetState::Complete | FetchTargetState::Partial
                        )
                    })
                    .count();
                for observed in page.events() {
                    candidates.push(crate::parse_verified_kind0(
                        observed.event().raw_json(),
                        public_key,
                    )?);
                }
            }
            if successful_relays == 0 {
                return Err(relay_connection_failed());
            }
            let candidate = select_latest_kind0(candidates);
            if successful_relays == relays.len() {
                Ok(ProfileFetchResult::complete(candidate))
            } else {
                Ok(ProfileFetchResult::partial(candidate))
            }
        })
    }
}

fn unix_deadline(timeout: Duration) -> Result<u64, SafeError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| relay_connection_failed())?;
    let deadline = now
        .checked_add(timeout)
        .ok_or_else(relay_connection_failed)?;
    u64::try_from(deadline.as_millis())
        .ok()
        .filter(|deadline| *deadline > 0)
        .ok_or_else(relay_connection_failed)
}

fn timeout_millis(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis())
        .unwrap_or(u64::MAX)
        .clamp(1, 120_000)
}

const fn canonical_policy(policy: RelayDestinationPolicy) -> RelayUrlPolicy {
    match policy {
        RelayDestinationPolicy::Public => RelayUrlPolicy::Public,
        RelayDestinationPolicy::Local => RelayUrlPolicy::Local,
        RelayDestinationPolicy::PrivateNetwork => RelayUrlPolicy::PrivateNetwork,
    }
}

const fn invalid_relay_configuration() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("No Nostr relay is configured."),
    )
}

const fn relay_connection_failed() -> SafeError {
    SafeError::new(
        SafeErrorCode::RelayConnectionFailed,
        SafeMessage::new("The Nostr relays could not be reached."),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_studio_domain::{PublicKey, RelayDestinationPolicy, RelayUrl, SafeErrorCode};

    use radroots_studio_application::NostrClient;

    use crate::SdkNostrClient;

    #[tokio::test]
    async fn sdk_client_fetches_verified_profile_from_ephemeral_local_relay() {
        let relay = MockRelay::run().await.expect("local relay");
        let relay_url = relay.url().await;
        let keys = Keys::generate();
        let publisher = Client::new(keys.clone());
        publisher
            .add_relay(relay_url.clone())
            .await
            .expect("add relay");
        publisher.connect().await;
        publisher.wait_for_connection(Duration::from_secs(2)).await;
        publisher
            .send_event_builder(EventBuilder::metadata(
                &Metadata::new().name("Farmer").display_name("Farm Account"),
            ))
            .await
            .expect("publish metadata");

        let adapter = SdkNostrClient::new(Duration::from_secs(2));
        let domain_relay = RelayUrl::parse(relay_url.as_str(), RelayDestinationPolicy::Local)
            .expect("domain relay URL");
        let public_key =
            PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key");
        let fetched = adapter
            .fetch_profile(
                public_key,
                &[domain_relay],
                std::time::Instant::now() + Duration::from_secs(2),
            )
            .await
            .expect("fetch profile");
        let (profile, completeness) = fetched.into_parts();
        let profile = profile.expect("published profile");

        assert_eq!(profile.author(), public_key);
        assert_eq!(profile.metadata().preferred_name(), Some("Farm Account"));
        assert_eq!(
            completeness,
            radroots_studio_application::RelayFetchCompleteness::Complete
        );
        publisher.shutdown().await;
        relay.shutdown();
    }

    #[tokio::test]
    async fn sdk_client_rejects_empty_configuration_without_network_access() {
        let error = SdkNostrClient::new(Duration::from_millis(10))
            .fetch_profile(
                PublicKey::from_bytes([7; 32]).expect("valid public key"),
                &[],
                std::time::Instant::now() + Duration::from_millis(10),
            )
            .await
            .expect_err("empty relay list");

        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);

        let relay = RelayUrl::parse("wss://relay.example.test", RelayDestinationPolicy::Public)
            .expect("relay URL");
        let too_many = vec![relay; radroots_studio_application::MAX_CONFIGURED_RELAYS + 1];
        let error = SdkNostrClient::new(Duration::from_millis(10))
            .fetch_profile(
                PublicKey::from_bytes([7; 32]).expect("valid public key"),
                &too_many,
                std::time::Instant::now() + Duration::from_millis(10),
            )
            .await
            .expect_err("oversized relay list");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }

    #[tokio::test]
    async fn sdk_client_fails_when_no_configured_relay_completes() {
        let relay = RelayUrl::parse("ws://127.0.0.1:1", RelayDestinationPolicy::Local)
            .expect("unavailable relay");
        let error = SdkNostrClient::new(Duration::from_millis(25))
            .fetch_profile(
                PublicKey::from_bytes([7; 32]).expect("valid public key"),
                &[relay],
                std::time::Instant::now() + Duration::from_millis(50),
            )
            .await
            .expect_err("all relays unavailable");
        assert_eq!(error.code(), SafeErrorCode::RelayConnectionFailed);
    }

    #[tokio::test]
    async fn sdk_client_reports_partial_when_one_configured_relay_is_unavailable() {
        let relay = MockRelay::run().await.expect("local relay");
        let relay_url = relay.url().await;
        let keys = Keys::generate();
        let publisher = Client::new(keys.clone());
        publisher
            .add_relay(relay_url.clone())
            .await
            .expect("add relay");
        publisher.connect().await;
        publisher
            .send_event_builder(EventBuilder::metadata(&Metadata::new().name("Partial")))
            .await
            .expect("publish metadata");

        let configured = [
            RelayUrl::parse(relay_url.as_str(), RelayDestinationPolicy::Local).expect("live relay"),
            RelayUrl::parse("ws://127.0.0.1:1", RelayDestinationPolicy::Local)
                .expect("unavailable relay"),
        ];
        let fetched = SdkNostrClient::new(Duration::from_millis(250))
            .fetch_profile(
                PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key"),
                &configured,
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .await
            .expect("partial fetch");
        let (candidate, completeness) = fetched.into_parts();
        assert!(candidate.is_some());
        assert_eq!(
            completeness,
            radroots_studio_application::RelayFetchCompleteness::Partial
        );
        publisher.shutdown().await;
        relay.shutdown();
    }

    #[test]
    fn studio_policy_maps_exactly_to_the_canonical_transport_policy() {
        assert_eq!(
            super::canonical_policy(RelayDestinationPolicy::Public),
            radroots_transport_nostr::RelayUrlPolicy::Public
        );
        assert_eq!(
            super::canonical_policy(RelayDestinationPolicy::Local),
            radroots_transport_nostr::RelayUrlPolicy::Local
        );
        assert_eq!(
            super::canonical_policy(RelayDestinationPolicy::PrivateNetwork),
            radroots_transport_nostr::RelayUrlPolicy::PrivateNetwork
        );
        assert_eq!(super::timeout_millis(Duration::ZERO), 1);
        assert_eq!(
            super::timeout_millis(Duration::from_secs(1_000_000)),
            120_000
        );
        assert!(super::unix_deadline(Duration::from_secs(1)).is_ok());
        assert_eq!(
            super::invalid_relay_configuration().code(),
            SafeErrorCode::InvalidRelayConfiguration
        );
        assert_eq!(
            super::relay_connection_failed().code(),
            SafeErrorCode::RelayConnectionFailed
        );
    }
}
