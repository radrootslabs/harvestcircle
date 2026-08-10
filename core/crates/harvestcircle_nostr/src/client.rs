use std::time::{Duration, Instant};

use harvestcircle_domain::{
    PublicKey, RelayEndpoint, SafeError, SafeErrorCode, SafeMessage, select_latest_kind0,
};
use nostr::{Filter, JsonUtil, Kind, PublicKey as NostrPublicKey};
use nostr_sdk::Client;

use harvestcircle_application::{
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
        relays: &'a [RelayEndpoint],
        deadline: Instant,
    ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>> {
        Box::pin(async move {
            let readable_relays = relays
                .iter()
                .filter(|endpoint| endpoint.can_read())
                .collect::<Vec<_>>();
            if readable_relays.is_empty() {
                return Err(invalid_relay_configuration());
            }
            if relays.len() > MAX_CONFIGURED_RELAYS {
                return Err(invalid_relay_configuration());
            }

            let author = NostrPublicKey::from_slice(public_key.as_bytes())
                .map_err(|_| invalid_relay_configuration())?;
            let deadline = deadline.min(Instant::now() + self.timeout);
            let mut candidates = Vec::new();
            let mut successful_relays = 0usize;
            let filter = Filter::new()
                .author(author)
                .kind(Kind::Metadata)
                .limit(MAX_PROFILE_EVENTS_PER_RELAY);
            for relay in &readable_relays {
                let relay_url = relay.url().as_str();
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let client = Client::default();
                let result = match tokio::time::timeout_at(deadline.into(), async {
                    client
                        .add_relay(relay_url)
                        .await
                        .map_err(|_| relay_connection_failed())?;
                    client
                        .try_connect_relay(relay_url, remaining)
                        .await
                        .map_err(|_| relay_connection_failed())?;
                    client
                        .fetch_events_from([relay_url], filter.clone(), remaining)
                        .await
                        .map_err(|_| relay_connection_failed())
                })
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(relay_connection_failed()),
                };
                client.shutdown().await;
                if let Ok(events) = result {
                    successful_relays += 1;
                    for event in events {
                        candidates.push(crate::parse_verified_kind0(&event.as_json(), public_key)?);
                    }
                }
            }
            if successful_relays == 0 {
                return Err(relay_connection_failed());
            }
            let candidate = select_latest_kind0(candidates);
            if successful_relays == readable_relays.len() {
                Ok(ProfileFetchResult::complete(candidate))
            } else {
                Ok(ProfileFetchResult::partial(candidate))
            }
        })
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

    use harvestcircle_domain::{
        PublicKey, RelayDestinationPolicy, RelayEndpoint, RelayUrl, SafeErrorCode,
    };
    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;

    use harvestcircle_application::NostrClient;

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
                &Metadata::new().name("Farmer").display_name("Farm Identity"),
            ))
            .await
            .expect("publish metadata");

        let adapter = SdkNostrClient::new(Duration::from_secs(2));
        let domain_relay = endpoint(relay_url.as_str(), RelayDestinationPolicy::Local);
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
        assert_eq!(profile.metadata().preferred_name(), Some("Farm Identity"));
        assert_eq!(
            completeness,
            harvestcircle_application::RelayFetchCompleteness::Complete
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

        let write_only = RelayEndpoint::parse(
            "wss://relay.example.test",
            RelayDestinationPolicy::Public,
            false,
            true,
        )
        .expect("write-only relay");
        let error = SdkNostrClient::new(Duration::from_millis(10))
            .fetch_profile(
                PublicKey::from_bytes([7; 32]).expect("valid public key"),
                &[write_only],
                std::time::Instant::now() + Duration::from_millis(10),
            )
            .await
            .expect_err("read capability required");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);

        let relay = endpoint("wss://relay.example.test", RelayDestinationPolicy::Public);
        let too_many = vec![relay; harvestcircle_application::MAX_CONFIGURED_RELAYS + 1];
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
        let relay = endpoint("ws://127.0.0.1:1", RelayDestinationPolicy::Local);
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
            endpoint(relay_url.as_str(), RelayDestinationPolicy::Local),
            endpoint("ws://127.0.0.1:1", RelayDestinationPolicy::Local),
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
            harvestcircle_application::RelayFetchCompleteness::Partial
        );
        publisher.shutdown().await;
        relay.shutdown();
    }

    #[test]
    fn harvestcircle_relay_policy_remains_domain_owned_and_fail_closed() {
        assert!(RelayUrl::parse("wss://relay.example", RelayDestinationPolicy::Public).is_ok());
        assert!(RelayUrl::parse("ws://127.0.0.1:7777", RelayDestinationPolicy::Local).is_ok());
        assert!(
            RelayUrl::parse(
                "wss://10.0.0.1:7777",
                RelayDestinationPolicy::PrivateNetwork
            )
            .is_ok()
        );
        assert!(RelayUrl::parse("ws://relay.example", RelayDestinationPolicy::Public).is_err());
        assert_eq!(
            super::invalid_relay_configuration().code(),
            SafeErrorCode::InvalidRelayConfiguration
        );
        assert_eq!(
            super::relay_connection_failed().code(),
            SafeErrorCode::RelayConnectionFailed
        );
    }

    fn endpoint(value: &str, destination: RelayDestinationPolicy) -> RelayEndpoint {
        RelayEndpoint::parse(value, destination, true, true).expect("relay endpoint")
    }
}
