// SPDX-License-Identifier: MPL-2.0
//! UI-neutral Studio preference state.
//!
//! This module carries forward the uniquely required preference behavior from
//! source commit `6074a4745be361f21bb47d4778c74a14b2d57954`. It intentionally
//! excludes that source's process-global state, sample account, and FFI layer.

use url::Url;

pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;
const MAX_SUMMARY_BYTES: usize = 256;
const MAX_SERVER_URL_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateChannel {
    #[default]
    Stable,
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudioPreferences {
    pub allow_incoming_connections: bool,
    pub use_radroots_dns: bool,
    pub use_radroots_subnets: bool,
    pub launch_at_login: bool,
    pub hide_dock_icon: bool,
    pub vpn_on_demand_enabled: bool,
    pub run_as_exit_node: bool,
    pub allow_local_network_access: bool,
    pub automatically_check_for_updates: bool,
    pub update_channel: UpdateChannel,
    pub last_update_check_summary: String,
    pub alternate_server_url: String,
}

impl Default for StudioPreferences {
    fn default() -> Self {
        Self {
            allow_incoming_connections: true,
            use_radroots_dns: true,
            use_radroots_subnets: true,
            launch_at_login: true,
            hide_dock_icon: false,
            vpn_on_demand_enabled: false,
            run_as_exit_node: false,
            allow_local_network_access: false,
            automatically_check_for_updates: true,
            update_channel: UpdateChannel::Stable,
            last_update_check_summary: String::new(),
            alternate_server_url: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferencesState {
    schema_version: u32,
    revision: u64,
    preferences: StudioPreferences,
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            revision: 1,
            preferences: StudioPreferences::default(),
        }
    }
}

impl PreferencesState {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn preferences(&self) -> &StudioPreferences {
        &self.preferences
    }

    pub fn apply(&mut self, change: PreferenceChange) -> Result<bool, PreferencesError> {
        let mut candidate = self.preferences.clone();
        change.apply_to(&mut candidate)?;
        if candidate == self.preferences {
            return Ok(false);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(PreferencesError::RevisionExhausted)?;
        self.preferences = candidate;
        Ok(true)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreferenceChange {
    AllowIncomingConnections(bool),
    UseRadrootsDns(bool),
    UseRadrootsSubnets(bool),
    LaunchAtLogin(bool),
    HideDockIcon(bool),
    VpnOnDemandEnabled(bool),
    RunAsExitNode(bool),
    AllowLocalNetworkAccess(bool),
    AutomaticallyCheckForUpdates(bool),
    UpdateChannel(UpdateChannel),
    LastUpdateCheckSummary(String),
    AlternateServerUrl(String),
}

impl PreferenceChange {
    fn apply_to(self, preferences: &mut StudioPreferences) -> Result<(), PreferencesError> {
        match self {
            Self::AllowIncomingConnections(value) => {
                preferences.allow_incoming_connections = value;
            }
            Self::UseRadrootsDns(value) => preferences.use_radroots_dns = value,
            Self::UseRadrootsSubnets(value) => preferences.use_radroots_subnets = value,
            Self::LaunchAtLogin(value) => preferences.launch_at_login = value,
            Self::HideDockIcon(value) => preferences.hide_dock_icon = value,
            Self::VpnOnDemandEnabled(value) => preferences.vpn_on_demand_enabled = value,
            Self::RunAsExitNode(value) => preferences.run_as_exit_node = value,
            Self::AllowLocalNetworkAccess(value) => {
                preferences.allow_local_network_access = value;
            }
            Self::AutomaticallyCheckForUpdates(value) => {
                preferences.automatically_check_for_updates = value;
            }
            Self::UpdateChannel(value) => preferences.update_channel = value,
            Self::LastUpdateCheckSummary(value) => {
                preferences.last_update_check_summary = validated_summary(value)?;
            }
            Self::AlternateServerUrl(value) => {
                preferences.alternate_server_url = validated_server_url(value)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferencesError {
    InvalidSummary,
    InvalidAlternateServerUrl,
    RevisionExhausted,
}

fn validated_summary(value: String) -> Result<String, PreferencesError> {
    let value = value.trim();
    if value.len() > MAX_SUMMARY_BYTES || value.chars().any(char::is_control) {
        return Err(PreferencesError::InvalidSummary);
    }
    Ok(value.to_owned())
}

fn validated_server_url(value: String) -> Result<String, PreferencesError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.len() > MAX_SERVER_URL_BYTES || value.chars().any(char::is_control) {
        return Err(PreferencesError::InvalidAlternateServerUrl);
    }
    let url = Url::parse(value).map_err(|_| PreferencesError::InvalidAlternateServerUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(PreferencesError::InvalidAlternateServerUrl);
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_the_reviewed_boolean_policy_without_sample_identity() {
        let state = PreferencesState::default();
        assert_eq!(state.schema_version(), PREFERENCES_SCHEMA_VERSION);
        assert_eq!(state.revision(), 1);
        assert!(state.preferences().allow_incoming_connections);
        assert!(state.preferences().use_radroots_dns);
        assert!(state.preferences().use_radroots_subnets);
        assert!(state.preferences().launch_at_login);
        assert!(state.preferences().automatically_check_for_updates);
        assert_eq!(state.preferences().update_channel, UpdateChannel::Stable);
        assert!(state.preferences().last_update_check_summary.is_empty());
        assert!(state.preferences().alternate_server_url.is_empty());
    }

    #[test]
    fn revisions_advance_only_when_a_valid_canonical_value_changes() {
        let mut state = PreferencesState::default();
        assert!(
            !state
                .apply(PreferenceChange::HideDockIcon(false))
                .expect("unchanged value")
        );
        assert_eq!(state.revision(), 1);
        assert!(
            state
                .apply(PreferenceChange::HideDockIcon(true))
                .expect("changed value")
        );
        assert_eq!(state.revision(), 2);
        assert!(state.preferences().hide_dock_icon);
    }

    #[test]
    fn every_boolean_and_channel_change_updates_exactly_one_revision() {
        let mut state = PreferencesState::default();
        let changes = [
            PreferenceChange::AllowIncomingConnections(false),
            PreferenceChange::UseRadrootsDns(false),
            PreferenceChange::UseRadrootsSubnets(false),
            PreferenceChange::LaunchAtLogin(false),
            PreferenceChange::VpnOnDemandEnabled(true),
            PreferenceChange::RunAsExitNode(true),
            PreferenceChange::AllowLocalNetworkAccess(true),
            PreferenceChange::AutomaticallyCheckForUpdates(false),
            PreferenceChange::UpdateChannel(UpdateChannel::Preview),
        ];
        for (index, change) in changes.into_iter().enumerate() {
            assert!(state.apply(change).expect("valid preference change"));
            assert_eq!(state.revision(), index as u64 + 2);
        }
        let preferences = state.preferences();
        assert!(!preferences.allow_incoming_connections);
        assert!(!preferences.use_radroots_dns);
        assert!(!preferences.use_radroots_subnets);
        assert!(!preferences.launch_at_login);
        assert!(preferences.vpn_on_demand_enabled);
        assert!(preferences.run_as_exit_node);
        assert!(preferences.allow_local_network_access);
        assert!(!preferences.automatically_check_for_updates);
        assert_eq!(preferences.update_channel, UpdateChannel::Preview);
    }

    #[test]
    fn revision_overflow_is_rejected_without_mutation() {
        let mut state = PreferencesState {
            revision: u64::MAX,
            ..PreferencesState::default()
        };
        assert_eq!(
            state.apply(PreferenceChange::HideDockIcon(true)),
            Err(PreferencesError::RevisionExhausted)
        );
        assert!(!state.preferences().hide_dock_icon);
    }

    #[test]
    fn alternate_server_is_trimmed_canonical_and_credential_free() {
        let mut state = PreferencesState::default();
        state
            .apply(PreferenceChange::AlternateServerUrl(
                " https://example.com/api ".to_owned(),
            ))
            .expect("valid URL");
        assert_eq!(
            state.preferences().alternate_server_url,
            "https://example.com/api"
        );
        for invalid in [
            "http://example.com",
            "https://user@example.com",
            "https://user:password@example.com",
            "https://example.com/#fragment",
            "not a URL",
            "https://example.com/a\nb",
        ] {
            assert_eq!(
                state.apply(PreferenceChange::AlternateServerUrl(invalid.to_owned())),
                Err(PreferencesError::InvalidAlternateServerUrl)
            );
        }
        state
            .apply(PreferenceChange::AlternateServerUrl("   ".to_owned()))
            .expect("empty URL resets the override");
        assert!(state.preferences().alternate_server_url.is_empty());
        assert_eq!(
            state.apply(PreferenceChange::AlternateServerUrl(format!(
                "https://example.com/{}",
                "x".repeat(MAX_SERVER_URL_BYTES)
            ))),
            Err(PreferencesError::InvalidAlternateServerUrl)
        );
    }

    #[test]
    fn summary_is_bounded_trimmed_and_control_free() {
        let mut state = PreferencesState::default();
        state
            .apply(PreferenceChange::LastUpdateCheckSummary(
                " Checked today ".to_owned(),
            ))
            .expect("valid summary");
        assert_eq!(
            state.preferences().last_update_check_summary,
            "Checked today"
        );
        assert_eq!(
            state.apply(PreferenceChange::LastUpdateCheckSummary(
                "bad\nvalue".to_owned()
            )),
            Err(PreferencesError::InvalidSummary)
        );
        assert_eq!(
            state.apply(PreferenceChange::LastUpdateCheckSummary(
                "x".repeat(MAX_SUMMARY_BYTES + 1)
            )),
            Err(PreferencesError::InvalidSummary)
        );
    }
}
