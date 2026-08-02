use std::time::Duration;

use nostr::{Filter, JsonUtil, Kind, PublicKey as NostrPublicKey};
use nostr_sdk::ClientBuilder;
use radroots_studio_domain::{
    Kind0ProfileCandidate, PublicKey, RelayUrl, SafeError, SafeErrorCode, SafeMessage,
    select_latest_kind0,
};

use crate::{BoxFuture, NostrClient};

pub struct SdkNostrClient {
    timeout: Duration,
}

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
    ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
        Box::pin(async move {
            if relays.is_empty() {
                return Err(invalid_relay_configuration());
            }

            let client = ClientBuilder::new().build();
            for relay in relays {
                client
                    .add_relay(relay.as_str())
                    .await
                    .map_err(|_| relay_connection_failed())?;
            }
            client.connect().await;
            client.wait_for_connection(self.timeout).await;

            let author = NostrPublicKey::from_slice(public_key.as_bytes())
                .map_err(|_| profile_refresh_failed())?;
            let filter = Filter::new().author(author).kind(Kind::Metadata).limit(64);
            let fetched = client.fetch_events(filter, self.timeout).await;
            client.shutdown().await;
            let events = fetched.map_err(|_| relay_connection_failed())?;

            let mut candidates = Vec::with_capacity(events.len());
            for event in events.iter() {
                candidates.push(radroots_studio_nostr::parse_verified_kind0(
                    &event.as_json(),
                    public_key,
                )?);
            }
            Ok(select_latest_kind0(candidates))
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

const fn profile_refresh_failed() -> SafeError {
    SafeError::new(
        SafeErrorCode::ProfileRefreshFailed,
        SafeMessage::new("The Nostr profile could not be refreshed."),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_studio_domain::{PublicKey, RelayUrl, SafeErrorCode};

    use crate::{NostrClient, SdkNostrClient};

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
        let domain_relay = RelayUrl::parse(relay_url.as_str()).expect("domain relay URL");
        let public_key = PublicKey::from_bytes(keys.public_key().to_bytes());
        let profile = adapter
            .fetch_profile(public_key, &[domain_relay])
            .await
            .expect("fetch profile")
            .expect("published profile");

        assert_eq!(profile.author(), public_key);
        assert_eq!(profile.metadata().preferred_name(), Some("Farm Account"));
        publisher.shutdown().await;
        relay.shutdown();
    }

    #[tokio::test]
    async fn sdk_client_rejects_empty_configuration_without_network_access() {
        let error = SdkNostrClient::new(Duration::from_millis(10))
            .fetch_profile(PublicKey::from_bytes([1; 32]), &[])
            .await
            .expect_err("empty relay list");

        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }
}
