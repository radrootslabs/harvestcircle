//! Validated Nostr relay values.

use std::fmt::{self, Display, Formatter};

use url::{Host, Url};

use crate::{SafeError, SafeErrorCode, SafeMessage};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RelayDestinationPolicy {
    Public,
    Local,
    PrivateNetwork,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelayUrl {
    value: String,
    policy: RelayDestinationPolicy,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelayEndpoint {
    url: RelayUrl,
    read: bool,
    write: bool,
}

impl RelayEndpoint {
    /// Validates one explicitly classified relay endpoint and its capabilities.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration error when the URL and destination conflict
    /// or when the endpoint has neither read nor write capability.
    pub fn parse(
        value: &str,
        destination: RelayDestinationPolicy,
        read: bool,
        write: bool,
    ) -> Result<Self, SafeError> {
        if !read && !write {
            return Err(invalid_relay());
        }
        Ok(Self {
            url: RelayUrl::parse(value, destination)?,
            read,
            write,
        })
    }

    #[must_use]
    pub const fn url(&self) -> &RelayUrl {
        &self.url
    }

    #[must_use]
    pub const fn destination(&self) -> RelayDestinationPolicy {
        self.url.policy()
    }

    #[must_use]
    pub const fn can_read(&self) -> bool {
        self.read
    }

    #[must_use]
    pub const fn can_write(&self) -> bool {
        self.write
    }
}

impl RelayUrl {
    /// Parses and normalizes an allowed WebSocket relay URL.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration error for empty or malformed input,
    /// forbidden schemes, credentials, fragments, or non-loopback `ws://`.
    pub fn parse(value: &str, policy: RelayDestinationPolicy) -> Result<Self, SafeError> {
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

        match (policy, parsed.scheme()) {
            (RelayDestinationPolicy::Public, "wss") if is_public_destination(&parsed) => {}
            (RelayDestinationPolicy::PrivateNetwork, "wss") if is_private_network(&parsed) => {}
            (RelayDestinationPolicy::Local, "ws" | "wss") if is_loopback(&parsed) => {}
            _ => return Err(invalid_relay()),
        }

        if parsed.host().is_none() {
            return Err(invalid_relay());
        }

        Ok(Self {
            value: parsed.to_string(),
            policy,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn policy(&self) -> RelayDestinationPolicy {
        self.policy
    }
}

impl Display for RelayUrl {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost",
        Some(Host::Ipv4(address)) => address.octets()[0] == 127,
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn is_private_network(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_private() || address.is_link_local(),
        Some(Host::Ipv6(address)) => {
            let first = address.segments()[0];
            first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
        Some(Host::Domain(_)) | None => false,
    }
}

fn is_public_destination(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain != "localhost" && !domain.ends_with(".local"),
        Some(Host::Ipv4(address)) => {
            !address.is_loopback()
                && !address.is_private()
                && !address.is_link_local()
                && !address.is_unspecified()
        }
        Some(Host::Ipv6(address)) => {
            let first = address.segments()[0];
            !address.is_loopback()
                && !address.is_unspecified()
                && first & 0xfe00 != 0xfc00
                && first & 0xffc0 != 0xfe80
        }
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
    use super::{RelayDestinationPolicy, RelayEndpoint, RelayUrl};
    use crate::SafeErrorCode;

    #[test]
    fn relay_accepts_secure_remote_and_loopback_development_urls() {
        for (input, policy, expected) in [
            (
                " wss://Relay.Example/path ",
                RelayDestinationPolicy::Public,
                "wss://relay.example/path",
            ),
            (
                "ws://localhost:8080",
                RelayDestinationPolicy::Local,
                "ws://localhost:8080/",
            ),
            (
                "ws://127.42.1.9:8080",
                RelayDestinationPolicy::Local,
                "ws://127.42.1.9:8080/",
            ),
            (
                "ws://[::1]:8080",
                RelayDestinationPolicy::Local,
                "ws://[::1]:8080/",
            ),
        ] {
            let relay = RelayUrl::parse(input, policy).expect("allowed relay");
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
            let error = RelayUrl::parse(input, RelayDestinationPolicy::Public)
                .expect_err("forbidden relay");
            assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
        }
    }

    #[test]
    fn relay_endpoint_requires_a_direction_capability() {
        let endpoint = RelayEndpoint::parse(
            "wss://relay.example",
            RelayDestinationPolicy::Public,
            true,
            false,
        )
        .expect("read endpoint");
        assert!(endpoint.can_read());
        assert!(!endpoint.can_write());
        assert_eq!(endpoint.destination(), RelayDestinationPolicy::Public);
        assert!(
            RelayEndpoint::parse(
                "wss://relay.example",
                RelayDestinationPolicy::Public,
                false,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn relay_destination_policy_is_explicit_and_fail_closed() {
        assert!(RelayUrl::parse("ws://localhost:8080", RelayDestinationPolicy::Public).is_err());
        assert!(RelayUrl::parse("wss://relay.example", RelayDestinationPolicy::Local).is_err());
        assert!(RelayUrl::parse("wss://10.0.0.4", RelayDestinationPolicy::Public).is_err());
        assert!(
            RelayUrl::parse(
                "wss://relay.example",
                RelayDestinationPolicy::PrivateNetwork
            )
            .is_err()
        );
        let private = RelayUrl::parse("wss://10.0.0.4", RelayDestinationPolicy::PrivateNetwork)
            .expect("explicit private network");
        assert_eq!(private.policy(), RelayDestinationPolicy::PrivateNetwork);
    }
}
