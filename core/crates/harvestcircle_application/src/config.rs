use harvestcircle_domain::{
    RelayDestinationPolicy, SafeError, SafeErrorCode, SafeMessage, normalize_relay_urls,
};

use crate::RelayConfiguration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayRuntimeMode {
    Development,
    Packaged,
}

/// Validates relay URLs supplied by a platform host without reading process state.
///
/// # Errors
///
/// Returns a safe configuration error when an entry is invalid or no relay was
/// explicitly supplied.
pub fn relay_configuration_from_urls(
    values: &[String],
    mode: RelayRuntimeMode,
) -> Result<RelayConfiguration, SafeError> {
    if values.is_empty() {
        return Err(invalid_configuration());
    }
    let policy = match mode {
        RelayRuntimeMode::Development => RelayDestinationPolicy::Local,
        RelayRuntimeMode::Packaged => RelayDestinationPolicy::Public,
    };
    let normalized = normalize_relay_urls(values.iter().map(String::as_str), policy)?;
    if normalized.is_empty() {
        return Err(invalid_configuration());
    }
    RelayConfiguration::new(normalized)
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

    use super::{RelayRuntimeMode, relay_configuration_from_urls};

    #[test]
    fn relay_config_requires_explicit_input_in_every_mode() {
        for mode in [RelayRuntimeMode::Development, RelayRuntimeMode::Packaged] {
            let error = relay_configuration_from_urls(&[], mode).expect_err("input required");
            assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
        }

        let development = relay_configuration_from_urls(
            &["ws://localhost:8080".to_owned()],
            RelayRuntimeMode::Development,
        )
        .expect("explicit development relay");
        assert_eq!(development.relays()[0].as_str(), "ws://localhost:8080/");
    }

    #[test]
    fn relay_config_trims_deduplicates_and_preserves_order() {
        let configuration = relay_configuration_from_urls(
            &[
                " wss://relay.one ".to_owned(),
                "wss://relay.two".to_owned(),
                "wss://relay.one/ ".to_owned(),
            ],
            RelayRuntimeMode::Packaged,
        )
        .expect("configuration");
        let relays = configuration
            .relays()
            .iter()
            .map(harvestcircle_domain::RelayUrl::as_str)
            .collect::<Vec<_>>();
        assert_eq!(relays, ["wss://relay.one/", "wss://relay.two/"]);
    }

    #[test]
    fn relay_config_rejects_any_invalid_comma_separated_entry() {
        let error = relay_configuration_from_urls(
            &[
                "wss://relay.one".to_owned(),
                "https://not-a-relay.test".to_owned(),
            ],
            RelayRuntimeMode::Packaged,
        )
        .expect_err("invalid entry");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }
}
