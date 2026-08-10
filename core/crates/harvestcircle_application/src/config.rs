use std::collections::HashSet;

use harvestcircle_domain::{
    RelayDestinationPolicy, RelayEndpoint, SafeError, SafeErrorCode, SafeMessage,
};

use crate::RelayConfiguration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayEndpointInput {
    url: String,
    destination: RelayDestinationPolicy,
    read: bool,
    write: bool,
}

impl RelayEndpointInput {
    #[must_use]
    pub fn new(
        url: impl Into<String>,
        destination: RelayDestinationPolicy,
        read: bool,
        write: bool,
    ) -> Self {
        Self {
            url: url.into(),
            destination,
            read,
            write,
        }
    }
}

/// Validates explicitly classified relay endpoints supplied by a platform host.
///
/// # Errors
///
/// Returns a safe configuration error when an entry is invalid or no relay was
/// explicitly supplied.
pub fn relay_configuration_from_endpoints(
    values: &[RelayEndpointInput],
) -> Result<RelayConfiguration, SafeError> {
    if values.is_empty() {
        return Err(invalid_configuration());
    }
    let mut seen = HashSet::new();
    let mut endpoints = Vec::with_capacity(values.len());
    for value in values {
        let endpoint =
            RelayEndpoint::parse(&value.url, value.destination, value.read, value.write)?;
        if !seen.insert(endpoint.url().as_str().to_owned()) {
            return Err(invalid_configuration());
        }
        endpoints.push(endpoint);
    }
    RelayConfiguration::new(endpoints)
}

const fn invalid_configuration() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("The Nostr relay configuration is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_domain::{RelayDestinationPolicy, SafeErrorCode};

    use super::{RelayEndpointInput, relay_configuration_from_endpoints};

    #[test]
    fn relay_config_requires_explicit_input_and_supports_mixed_destinations() {
        let error = relay_configuration_from_endpoints(&[]).expect_err("input required");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);

        let development = relay_configuration_from_endpoints(&[
            RelayEndpointInput::new(
                "ws://localhost:8080",
                RelayDestinationPolicy::Local,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "wss://relay.example",
                RelayDestinationPolicy::Public,
                true,
                true,
            ),
        ])
        .expect("explicit development relay");
        assert_eq!(
            development.relays()[0].url().as_str(),
            "ws://localhost:8080/"
        );
        assert_eq!(
            development.relays()[1].destination(),
            RelayDestinationPolicy::Public
        );
    }

    #[test]
    fn relay_config_rejects_normalized_duplicates_and_capability_free_entries() {
        for values in [
            vec![
                RelayEndpointInput::new(
                    "wss://relay.one",
                    RelayDestinationPolicy::Public,
                    true,
                    false,
                ),
                RelayEndpointInput::new(
                    "wss://RELAY.one/",
                    RelayDestinationPolicy::Public,
                    false,
                    true,
                ),
            ],
            vec![RelayEndpointInput::new(
                "wss://relay.one",
                RelayDestinationPolicy::Public,
                false,
                false,
            )],
        ] {
            assert!(relay_configuration_from_endpoints(&values).is_err());
        }
    }

    #[test]
    fn relay_config_rejects_any_invalid_comma_separated_entry() {
        let error = relay_configuration_from_endpoints(&[
            RelayEndpointInput::new(
                "wss://relay.one",
                RelayDestinationPolicy::Public,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "https://not-a-relay.test",
                RelayDestinationPolicy::Public,
                true,
                true,
            ),
        ])
        .expect_err("invalid entry");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }
}
