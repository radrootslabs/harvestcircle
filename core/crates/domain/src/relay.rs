//! Validated Nostr relay values.

use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use url::{Host, Url};

use crate::{SafeError, SafeErrorCode, SafeMessage};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelayUrl(String);

impl RelayUrl {
    /// Parses and normalizes an allowed WebSocket relay URL.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration error for empty or malformed input,
    /// forbidden schemes, credentials, fragments, or non-loopback `ws://`.
    pub fn parse(value: &str) -> Result<Self, SafeError> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            return Err(invalid_relay());
        }

        let parsed = Url::parse(trimmed).map_err(|_| invalid_relay())?;
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid_relay());
        }

        match parsed.scheme() {
            "wss" => {}
            "ws" if is_loopback(&parsed) => {}
            _ => return Err(invalid_relay()),
        }

        if parsed.host().is_none() {
            return Err(invalid_relay());
        }

        Ok(Self(parsed.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for RelayUrl {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RelayUrl {
    type Err = SafeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Parses relay values and removes duplicates without changing first-seen order.
///
/// # Errors
///
/// Returns the first safe relay validation error.
pub fn normalize_relay_urls<I, S>(values: I) -> Result<Vec<RelayUrl>, SafeError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut relays = Vec::new();
    for value in values {
        let relay = RelayUrl::parse(value.as_ref())?;
        if seen.insert(relay.clone()) {
            relays.push(relay);
        }
    }
    Ok(relays)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost",
        Some(Host::Ipv4(address)) => address.octets()[0] == 127,
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

const fn invalid_relay() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("The Nostr relay URL is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use super::{RelayUrl, normalize_relay_urls};
    use crate::SafeErrorCode;

    #[test]
    fn relay_accepts_secure_remote_and_loopback_development_urls() {
        for (input, expected) in [
            (" wss://Relay.Example/path ", "wss://relay.example/path"),
            ("ws://localhost:8080", "ws://localhost:8080/"),
            ("ws://127.42.1.9:8080", "ws://127.42.1.9:8080/"),
            ("ws://[::1]:8080", "ws://[::1]:8080/"),
        ] {
            let relay = RelayUrl::parse(input).expect("allowed relay");
            assert_eq!(relay.as_str(), expected);
            assert_eq!(relay.to_string(), expected);
        }
    }

    #[test]
    fn relay_rejects_non_websocket_credentials_fragments_and_remote_plaintext() {
        for input in [
            "",
            "https://relay.example",
            "http://localhost:8080",
            "wss://user:password@relay.example",
            "wss://relay.example/#fragment",
            "ws://relay.example",
            "ws://192.168.1.2:8080",
            "ws://localhost.evil.example:8080",
            "wss://relay.example/\nunsafe",
        ] {
            let error = RelayUrl::parse(input).expect_err("forbidden relay");
            assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
        }
    }

    #[test]
    fn relay_deduplication_preserves_normalized_first_seen_order() {
        let relays = normalize_relay_urls([
            "wss://relay.example",
            " wss://second.example/path ",
            "wss://RELAY.example/",
            "wss://second.example/path",
        ])
        .expect("valid relays");

        assert_eq!(
            relays.iter().map(RelayUrl::as_str).collect::<Vec<_>>(),
            vec!["wss://relay.example/", "wss://second.example/path"]
        );
    }
}
