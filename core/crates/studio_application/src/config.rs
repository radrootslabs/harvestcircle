use radroots_studio_domain::{
    RelayDestinationPolicy, SafeError, SafeErrorCode, SafeMessage, normalize_relay_urls,
};

use crate::RelayConfiguration;

pub const RELAY_ENVIRONMENT_VARIABLE: &str = "RADROOTS_NOSTR_RELAYS";
const DEVELOPMENT_RELAY: &str = "ws://localhost:8080";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayRuntimeMode {
    Development,
    Packaged,
}

/// Reads the process relay configuration once through the Rust-owned boundary.
///
/// # Errors
///
/// Returns a safe configuration error for missing Unicode or invalid relay data.
pub fn relay_configuration_from_environment(
    mode: RelayRuntimeMode,
) -> Result<RelayConfiguration, SafeError> {
    let value = match std::env::var(RELAY_ENVIRONMENT_VARIABLE) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => return Err(invalid_configuration()),
    };
    relay_configuration_from_value(value.as_deref(), mode)
}

/// Parses an injected comma-separated relay list without mutating process state.
///
/// # Errors
///
/// Returns a safe configuration error when an entry is invalid or packaged mode
/// has no configured relay.
pub fn relay_configuration_from_value(
    value: Option<&str>,
    mode: RelayRuntimeMode,
) -> Result<RelayConfiguration, SafeError> {
    let configured = value.unwrap_or_default().trim();
    let (source, policy) = if configured.is_empty() {
        match mode {
            RelayRuntimeMode::Development => (DEVELOPMENT_RELAY, RelayDestinationPolicy::Local),
            RelayRuntimeMode::Packaged => return Err(invalid_configuration()),
        }
    } else {
        (configured, RelayDestinationPolicy::Public)
    };
    let normalized = normalize_relay_urls(source.split(',').map(str::trim), policy)?;
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
    use radroots_studio_domain::SafeErrorCode;

    use super::{RelayRuntimeMode, relay_configuration_from_value};

    #[test]
    fn relay_config_uses_localhost_fallback_only_for_development() {
        for value in [None, Some(""), Some("   ")] {
            let development = relay_configuration_from_value(value, RelayRuntimeMode::Development)
                .expect("development fallback");
            assert_eq!(development.relays()[0].as_str(), "ws://localhost:8080/");
            let packaged = relay_configuration_from_value(value, RelayRuntimeMode::Packaged)
                .expect_err("packaged configuration required");
            assert_eq!(packaged.code(), SafeErrorCode::InvalidRelayConfiguration);
        }
    }

    #[test]
    fn relay_config_trims_deduplicates_and_preserves_order() {
        let configuration = relay_configuration_from_value(
            Some(" wss://relay.one ,wss://relay.two,wss://relay.one/ "),
            RelayRuntimeMode::Packaged,
        )
        .expect("configuration");
        let relays = configuration
            .relays()
            .iter()
            .map(radroots_studio_domain::RelayUrl::as_str)
            .collect::<Vec<_>>();
        assert_eq!(relays, ["wss://relay.one/", "wss://relay.two/"]);
    }

    #[test]
    fn relay_config_rejects_any_invalid_comma_separated_entry() {
        let error = relay_configuration_from_value(
            Some("wss://relay.one,https://not-a-relay.test"),
            RelayRuntimeMode::Packaged,
        )
        .expect_err("invalid entry");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }
}
