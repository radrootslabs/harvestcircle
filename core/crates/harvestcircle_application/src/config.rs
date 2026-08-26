use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};
use radroots_transport_nostr::{RelayAccess, RelayEndpoint, RelayUrlPolicy};

use crate::RelayConfiguration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayEndpointInput {
    url: String,
    destination: RelayUrlPolicy,
    read: bool,
    write: bool,
}

impl RelayEndpointInput {
    #[must_use]
    pub fn new(url: String, destination: RelayUrlPolicy, read: bool, write: bool) -> Self {
        Self {
            url,
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
    if values.is_empty() || values.len() > crate::MAX_CONFIGURED_RELAYS {
        return Err(invalid_configuration());
    }
    let mut endpoints = Vec::with_capacity(values.len());
    for value in values {
        let access = match (value.read, value.write) {
            (true, false) => RelayAccess::ReadOnly,
            (true, true) => RelayAccess::ReadWrite,
            (false, _) => return Err(invalid_configuration()),
        };
        let endpoint = RelayEndpoint::new(&value.url, value.destination, access)
            .map_err(|_| invalid_configuration())?;
        if endpoints
            .iter()
            .any(|existing: &RelayEndpoint| existing.url() == endpoint.url())
        {
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
    use harvestcircle_domain::SafeErrorCode;
    use radroots_transport_nostr::RelayUrlPolicy;

    use super::{RelayEndpointInput, relay_configuration_from_endpoints};

    #[test]
    fn relay_config_requires_explicit_input_and_one_governed_profile() {
        let error = relay_configuration_from_endpoints(&[]).expect_err("input required");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);

        let development = relay_configuration_from_endpoints(&[
            RelayEndpointInput::new(
                "ws://localhost:8080".to_owned(),
                RelayUrlPolicy::Local,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "ws://127.0.0.1:8081".to_owned(),
                RelayUrlPolicy::Local,
                true,
                true,
            ),
        ])
        .expect("explicit development relay");
        assert_eq!(
            development.relays()[0].url().as_str(),
            "ws://localhost:8080"
        );
        assert_eq!(development.relays()[1].policy(), RelayUrlPolicy::Local);

        let mixed = [
            RelayEndpointInput::new(
                "ws://localhost:8080".to_owned(),
                RelayUrlPolicy::Local,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "wss://relay.example".to_owned(),
                RelayUrlPolicy::Public,
                true,
                true,
            ),
        ];
        assert!(relay_configuration_from_endpoints(&mixed).is_err());
    }

    #[test]
    fn relay_config_rejects_normalized_duplicates_and_capability_free_entries() {
        for values in [
            vec![
                RelayEndpointInput::new(
                    "wss://relay.one".to_owned(),
                    RelayUrlPolicy::Public,
                    true,
                    false,
                ),
                RelayEndpointInput::new(
                    "wss://RELAY.one/".to_owned(),
                    RelayUrlPolicy::Public,
                    false,
                    true,
                ),
            ],
            vec![RelayEndpointInput::new(
                "wss://relay.one".to_owned(),
                RelayUrlPolicy::Public,
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
                "wss://relay.one".to_owned(),
                RelayUrlPolicy::Public,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "https://not-a-relay.test".to_owned(),
                RelayUrlPolicy::Public,
                true,
                true,
            ),
        ])
        .expect_err("invalid entry");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }
}
