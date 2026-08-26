use core::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harvestcircle_application::{
    BoxFuture, MAX_CONFIGURED_RELAYS, NostrClient, ProfileFetchResult,
};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, select_latest_kind0};
use radroots_transport::outcome::FetchTargetState;
use radroots_transport::source::{FetchBounds, FetchSelector};
use radroots_transport::{EventSource, FetchRequest, TargetSet};
use radroots_transport_nostr::{Config, NostrTransport, RelayEndpoint, RelayProfile};

const MAX_PROFILE_EVENTS_PER_FETCH: u16 = 64;

pub struct SdkNostrClient {
    timeout: Duration,
    next_request: AtomicU64,
}

impl fmt::Debug for SdkNostrClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SdkNostrClient")
            .field("timeout", &self.timeout)
            .field("transport", &"[sealed]")
            .finish()
    }
}

impl SdkNostrClient {
    #[must_use]
    pub const fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            next_request: AtomicU64::new(1),
        }
    }

    fn request_id(&self) -> Result<String, SafeError> {
        let sequence = self
            .next_request
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| relay_connection_failed())?;
        Ok(format!("harvest-profile-{sequence}"))
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
            if relays.is_empty() || relays.len() > MAX_CONFIGURED_RELAYS {
                return Err(invalid_relay_configuration());
            }
            let readable = relays
                .iter()
                .filter(|endpoint| endpoint.access().can_read())
                .cloned()
                .collect::<Vec<_>>();
            if readable.is_empty() {
                return Err(invalid_relay_configuration());
            }

            let profile = RelayProfile::explicit(profile_kind(relays)?, relays.iter().cloned())
                .map_err(|_| invalid_relay_configuration())?;
            let timeout_ms = u64::try_from(self.timeout.as_millis())
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(invalid_relay_configuration)?;
            let config = Config::from_profile(profile)
                .with_timeouts(timeout_ms, timeout_ms, timeout_ms)
                .and_then(|config| config.with_max_connections(readable.len()))
                .map_err(|_| invalid_relay_configuration())?;
            let transport = NostrTransport::new(config);

            let targets = readable
                .iter()
                .map(|endpoint| endpoint.url().to_target())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| invalid_relay_configuration())?;
            let targets = TargetSet::new(targets).map_err(|_| invalid_relay_configuration())?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(relay_connection_failed());
            }
            let deadline_unix_ms = current_unix_ms()?
                .checked_add(
                    u64::try_from(remaining.as_millis()).map_err(|_| relay_connection_failed())?,
                )
                .ok_or_else(relay_connection_failed)?;
            let bounds = FetchBounds::new(MAX_PROFILE_EVENTS_PER_FETCH, deadline_unix_ms)
                .map_err(|_| invalid_relay_configuration())?;
            let author = radroots_identity::PublicKey::from_bytes(*public_key.as_bytes())
                .map_err(|_| invalid_relay_configuration())?;
            let selector = FetchSelector::all()
                .with_kinds(vec![0])
                .and_then(|selector| selector.with_authors(vec![author]))
                .map_err(|_| invalid_relay_configuration())?;
            let request = FetchRequest::new(self.request_id()?, targets, bounds)
                .map_err(|_| invalid_relay_configuration())?
                .with_selector(selector);
            let page = transport
                .fetch(request)
                .await
                .map_err(|_| relay_connection_failed())?;

            let successful = page
                .target_outcomes()
                .iter()
                .filter(|outcome| outcome.state() == FetchTargetState::Complete)
                .count();
            if successful == 0 {
                return Err(relay_connection_failed());
            }
            let mut candidates = Vec::with_capacity(page.events().len());
            for observed in page.events() {
                candidates.push(crate::parse_verified_kind0(
                    observed.event().raw_json(),
                    public_key,
                )?);
            }
            let candidate = select_latest_kind0(candidates);
            if successful == readable.len() {
                Ok(ProfileFetchResult::complete(candidate))
            } else {
                Ok(ProfileFetchResult::partial(candidate))
            }
        })
    }
}

fn profile_kind(
    relays: &[RelayEndpoint],
) -> Result<radroots_transport_nostr::RelayProfileKind, SafeError> {
    let has_local = relays
        .iter()
        .any(|relay| relay.policy() == radroots_transport_nostr::RelayUrlPolicy::Local);
    let has_private = relays
        .iter()
        .any(|relay| relay.policy() == radroots_transport_nostr::RelayUrlPolicy::PrivateNetwork);
    let has_public = relays
        .iter()
        .any(|relay| relay.policy() == radroots_transport_nostr::RelayUrlPolicy::Public);
    match (has_local, has_private, has_public) {
        (true, false, false) => Ok(radroots_transport_nostr::RelayProfileKind::Simulator),
        (false, false, true) => Ok(radroots_transport_nostr::RelayProfileKind::Public),
        (false, true, _) => Ok(radroots_transport_nostr::RelayProfileKind::Device),
        _ => Err(invalid_relay_configuration()),
    }
}

fn current_unix_ms() -> Result<u64, SafeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or_else(relay_connection_failed)
}

const fn invalid_relay_configuration() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("The Nostr relay configuration is invalid."),
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

    use harvestcircle_application::{NostrClient, RelayFetchCompleteness};
    use harvestcircle_domain::{PublicKey, SafeErrorCode};
    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_transport_nostr::{RelayAccess, RelayEndpoint, RelayUrlPolicy};

    use crate::SdkNostrClient;

    #[tokio::test]
    async fn governed_transport_fetches_verified_profile_from_local_relay() {
        let relay = MockRelay::run().await.expect("local relay");
        let relay_url = relay.url().await;
        let keys = Keys::generate();
        let publisher = Client::new(keys.clone());
        publisher.add_relay(relay_url.clone()).await.expect("relay");
        publisher.connect().await;
        publisher.wait_for_connection(Duration::from_secs(2)).await;
        publisher
            .send_event_builder(EventBuilder::metadata(
                &Metadata::new().name("Farmer").display_name("Farm Identity"),
            ))
            .await
            .expect("publish metadata");

        let adapter = SdkNostrClient::new(Duration::from_secs(2));
        let public_key = PublicKey::from_bytes(keys.public_key().to_bytes()).expect("public key");
        let fetched = adapter
            .fetch_profile(
                public_key,
                &[endpoint(relay_url.as_str(), RelayUrlPolicy::Local)],
                std::time::Instant::now() + Duration::from_secs(2),
            )
            .await
            .expect("profile");
        let (profile, completeness) = fetched.into_parts();
        assert_eq!(
            profile
                .expect("published profile")
                .metadata()
                .preferred_name(),
            Some("Farm Identity")
        );
        assert_eq!(completeness, RelayFetchCompleteness::Complete);
        publisher.shutdown().await;
        relay.shutdown();
    }

    #[tokio::test]
    async fn governed_transport_rejects_invalid_profiles_before_network_access() {
        let adapter = SdkNostrClient::new(Duration::from_millis(10));
        let public_key = PublicKey::from_bytes([7; 32]).expect("public key");
        let empty = adapter
            .fetch_profile(
                public_key,
                &[],
                std::time::Instant::now() + Duration::from_millis(10),
            )
            .await
            .expect_err("empty relay list");
        assert_eq!(empty.code(), SafeErrorCode::InvalidRelayConfiguration);

        let mixed = [
            endpoint("ws://127.0.0.1:7777", RelayUrlPolicy::Local),
            endpoint("wss://relay.example", RelayUrlPolicy::Public),
        ];
        let error = adapter
            .fetch_profile(
                public_key,
                &mixed,
                std::time::Instant::now() + Duration::from_millis(10),
            )
            .await
            .expect_err("mixed trust profiles");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }

    #[tokio::test]
    async fn governed_transport_fails_when_no_relay_completes() {
        let error = SdkNostrClient::new(Duration::from_millis(25))
            .fetch_profile(
                PublicKey::from_bytes([7; 32]).expect("public key"),
                &[endpoint("ws://127.0.0.1:1", RelayUrlPolicy::Local)],
                std::time::Instant::now() + Duration::from_millis(50),
            )
            .await
            .expect_err("unavailable relay");
        assert_eq!(error.code(), SafeErrorCode::RelayConnectionFailed);
    }

    #[test]
    fn debug_and_policy_are_safe_and_lib_owned() {
        let adapter = SdkNostrClient::new(Duration::from_secs(1));
        assert_eq!(
            format!("{adapter:?}"),
            "SdkNostrClient { timeout: 1s, transport: \"[sealed]\" }"
        );
        assert!(
            RelayEndpoint::new(
                "wss://relay.example",
                RelayUrlPolicy::Public,
                RelayAccess::ReadOnly,
            )
            .is_ok()
        );
        assert!(
            RelayEndpoint::new(
                "ws://relay.example",
                RelayUrlPolicy::Public,
                RelayAccess::ReadOnly,
            )
            .is_err()
        );
    }

    fn endpoint(value: &str, policy: RelayUrlPolicy) -> RelayEndpoint {
        RelayEndpoint::new(value, policy, RelayAccess::ReadWrite).expect("relay endpoint")
    }
}
