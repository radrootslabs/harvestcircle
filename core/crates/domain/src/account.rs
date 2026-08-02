//! Public account metadata and lifecycle values.

use crate::time::UnixTimestamp;
use crate::{Npub, PublicKey, SafeError, SafeErrorCode, SafeMessage};

const MAX_ACCOUNT_LABEL_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerKind {
    LocalSecret,
    WatchOnly,
    RemoteNip46,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAvailability {
    Available,
    CredentialMissing,
    StoreUnavailable,
    NotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLabel(String);

impl AccountLabel {
    /// Trims and validates an optional human-assigned account label value.
    ///
    /// # Errors
    ///
    /// Returns a safe metadata error when the resulting label is empty, too
    /// long, or contains a control character.
    pub fn parse(value: &str) -> Result<Self, SafeError> {
        let normalized = value.trim();
        if normalized.is_empty()
            || normalized.chars().count() > MAX_ACCOUNT_LABEL_CHARS
            || normalized.chars().any(char::is_control)
        {
            return Err(invalid_account_metadata());
        }
        Ok(Self(normalized.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AccountCreatedAt(UnixTimestamp);

impl AccountCreatedAt {
    #[must_use]
    pub const fn new(timestamp: UnixTimestamp) -> Self {
        Self(timestamp)
    }

    #[must_use]
    pub const fn timestamp(self) -> UnixTimestamp {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSummary {
    public_key: PublicKey,
    npub: Npub,
    signer_kind: SignerKind,
    key_availability: KeyAvailability,
    label: Option<AccountLabel>,
    created_at: AccountCreatedAt,
    last_used_at: Option<UnixTimestamp>,
}

impl AccountSummary {
    #[must_use]
    pub const fn new(
        public_key: PublicKey,
        npub: Npub,
        signer_kind: SignerKind,
        key_availability: KeyAvailability,
        label: Option<AccountLabel>,
        created_at: AccountCreatedAt,
        last_used_at: Option<UnixTimestamp>,
    ) -> Self {
        Self {
            public_key,
            npub,
            signer_kind,
            key_availability,
            label,
            created_at,
            last_used_at,
        }
    }

    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    #[must_use]
    pub fn npub(&self) -> &Npub {
        &self.npub
    }

    #[must_use]
    pub const fn signer_kind(&self) -> SignerKind {
        self.signer_kind
    }

    #[must_use]
    pub const fn key_availability(&self) -> KeyAvailability {
        self.key_availability
    }

    #[must_use]
    pub fn label(&self) -> Option<&AccountLabel> {
        self.label.as_ref()
    }

    #[must_use]
    pub const fn created_at(&self) -> AccountCreatedAt {
        self.created_at
    }

    #[must_use]
    pub const fn last_used_at(&self) -> Option<UnixTimestamp> {
        self.last_used_at
    }

    #[must_use]
    pub fn with_key_availability(&self, key_availability: KeyAvailability) -> Self {
        Self {
            public_key: self.public_key,
            npub: self.npub.clone(),
            signer_kind: self.signer_kind,
            key_availability,
            label: self.label.clone(),
            created_at: self.created_at,
            last_used_at: self.last_used_at,
        }
    }

    #[must_use]
    pub fn with_last_used_at(&self, last_used_at: UnixTimestamp) -> Self {
        Self {
            public_key: self.public_key,
            npub: self.npub.clone(),
            signer_kind: self.signer_kind,
            key_availability: self.key_availability,
            label: self.label.clone(),
            created_at: self.created_at,
            last_used_at: Some(last_used_at),
        }
    }

    #[must_use]
    pub fn display_label(&self) -> String {
        self.label
            .as_ref()
            .map_or_else(|| self.npub.short(), |label| label.as_str().to_owned())
    }
}

const fn invalid_account_metadata() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidAccountMetadata,
        SafeMessage::new("The account metadata is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use crate::time::UnixTimestamp;
    use crate::{Npub, PublicKey};

    use super::{AccountCreatedAt, AccountLabel, AccountSummary, KeyAvailability, SignerKind};

    const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn account(label: Option<AccountLabel>) -> AccountSummary {
        AccountSummary::new(
            PublicKey::from_bytes([7_u8; 32]),
            Npub::from_encoded(NPUB.to_owned()).expect("valid npub"),
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            label,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(10).expect("valid time")),
            None,
        )
    }

    #[test]
    fn account_label_is_trimmed_bounded_and_control_free() {
        let label = AccountLabel::parse("  Farm account  ").expect("valid label");
        assert_eq!(label.as_str(), "Farm account");

        for invalid in ["", "   ", "line\nbreak", &"x".repeat(81)] {
            assert!(AccountLabel::parse(invalid).is_err());
        }
    }

    #[test]
    fn account_display_prefers_label_then_shortened_npub() {
        let labelled = account(Some(AccountLabel::parse("Farm").expect("valid label")));
        let unlabelled = account(None);

        assert_eq!(labelled.display_label(), "Farm");
        assert_eq!(unlabelled.display_label(), "npub10elfcs4…8qzvjptg");
    }

    #[test]
    fn local_account_summary_contains_public_metadata_only() {
        let account = account(None);
        let debug = format!("{account:?}");

        assert_eq!(account.signer_kind(), SignerKind::LocalSecret);
        assert_eq!(account.key_availability(), KeyAvailability::Available);
        assert!(account.label().is_none());
        assert!(account.last_used_at().is_none());
        assert_eq!(account.created_at().timestamp().as_seconds(), 10);
        assert_eq!(account.public_key(), PublicKey::from_bytes([7_u8; 32]));
        assert_eq!(account.npub().as_str(), NPUB);
        assert!(!debug.contains("nsec1"));
        assert!(!debug.contains(&"11".repeat(32)));
    }
}
