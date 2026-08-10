//! Public identity metadata and lifecycle values.

use crate::time::UnixTimestamp;
use crate::{Npub, PublicKey, SafeError, SafeErrorCode, SafeMessage};

const MAX_IDENTITY_LABEL_CHARS: usize = 80;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NostrIdentityReference {
    public_key: PublicKey,
    npub: Npub,
}

impl NostrIdentityReference {
    /// Constructs one canonical Nostr identity reference and derives its npub.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key error if canonical NIP-19 encoding fails.
    pub fn derive(public_key: PublicKey) -> Result<Self, SafeError> {
        Ok(Self {
            public_key,
            npub: Npub::derive(public_key)?,
        })
    }

    /// Reconstitutes persisted identity only when its public forms agree.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key error for a mismatched or malformed npub.
    pub fn verify(public_key: PublicKey, npub: String) -> Result<Self, SafeError> {
        Ok(Self {
            public_key,
            npub: Npub::verify(public_key, npub)?,
        })
    }

    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    #[must_use]
    pub const fn npub(&self) -> &Npub {
        &self.npub
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalKeyringBinding {
    identity: PublicKey,
    availability: SignerAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SignerBinding {
    LocalKeyring(LocalKeyringBinding),
}

impl SignerBinding {
    #[must_use]
    pub const fn identity(self) -> PublicKey {
        match self {
            Self::LocalKeyring(binding) => binding.identity(),
        }
    }

    #[must_use]
    pub const fn availability(self) -> SignerAvailability {
        match self {
            Self::LocalKeyring(binding) => binding.availability(),
        }
    }

    #[must_use]
    pub const fn local_keyring(self) -> LocalKeyringBinding {
        match self {
            Self::LocalKeyring(binding) => binding,
        }
    }
}

impl From<LocalKeyringBinding> for SignerBinding {
    fn from(binding: LocalKeyringBinding) -> Self {
        Self::LocalKeyring(binding)
    }
}

impl LocalKeyringBinding {
    #[must_use]
    pub const fn new(identity: PublicKey, availability: SignerAvailability) -> Self {
        Self {
            identity,
            availability,
        }
    }

    #[must_use]
    pub const fn identity(self) -> PublicKey {
        self.identity
    }

    #[must_use]
    pub const fn availability(self) -> SignerAvailability {
        self.availability
    }

    #[must_use]
    pub const fn repair_action(self) -> Option<SignerRepairAction> {
        match self.availability {
            SignerAvailability::Available => None,
            SignerAvailability::CredentialMissing => Some(SignerRepairAction::ImportCredential),
            SignerAvailability::StoreUnavailable => Some(SignerRepairAction::RetryCredentialStore),
        }
    }

    /// Records a missing credential after a successful store lookup.
    ///
    /// # Errors
    ///
    /// Returns a safe state error unless the binding was previously available.
    pub fn mark_credential_missing(&mut self) -> Result<(), SafeError> {
        self.transition(
            SignerAvailability::Available,
            SignerAvailability::CredentialMissing,
        )
    }

    pub fn mark_store_unavailable(&mut self) {
        self.availability = SignerAvailability::StoreUnavailable;
    }

    /// Completes an explicit credential repair.
    ///
    /// # Errors
    ///
    /// Returns a safe state error unless a credential was missing.
    pub fn repair_credential(&mut self) -> Result<(), SafeError> {
        self.transition(
            SignerAvailability::CredentialMissing,
            SignerAvailability::Available,
        )
    }

    /// Resolves a recovered store lookup to its observed credential state.
    ///
    /// # Errors
    ///
    /// Returns a safe state error unless the credential store was unavailable.
    pub fn resolve_store_recovery(&mut self, credential_present: bool) -> Result<(), SafeError> {
        if self.availability != SignerAvailability::StoreUnavailable {
            return Err(invalid_identity_metadata());
        }
        self.availability = if credential_present {
            SignerAvailability::Available
        } else {
            SignerAvailability::CredentialMissing
        };
        Ok(())
    }

    fn transition(
        &mut self,
        expected: SignerAvailability,
        next: SignerAvailability,
    ) -> Result<(), SafeError> {
        if self.availability != expected {
            return Err(invalid_identity_metadata());
        }
        self.availability = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerAvailability {
    Available,
    CredentialMissing,
    StoreUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerRepairAction {
    ImportCredential,
    RetryCredentialStore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityLabel(String);

impl IdentityLabel {
    /// Trims and validates an optional human-assigned identity label value.
    ///
    /// # Errors
    ///
    /// Returns a safe metadata error when the resulting label is empty, too
    /// long, or contains a control character.
    pub fn parse(value: &str) -> Result<Self, SafeError> {
        let normalized = value.trim();
        if normalized.is_empty()
            || normalized.chars().count() > MAX_IDENTITY_LABEL_CHARS
            || normalized.chars().any(char::is_control)
        {
            return Err(invalid_identity_metadata());
        }
        Ok(Self(normalized.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IdentityCreatedAt(UnixTimestamp);

impl IdentityCreatedAt {
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
pub struct NostrIdentity {
    identity: NostrIdentityReference,
    signer_binding: SignerBinding,
    label: Option<IdentityLabel>,
    created_at: IdentityCreatedAt,
    last_used_at: Option<UnixTimestamp>,
}

impl NostrIdentity {
    /// Creates an identity summary whose identity and signer binding refer to the same identity.
    ///
    /// # Errors
    ///
    /// Returns an invalid-identity-metadata error when the signer binding belongs to a different
    /// public key.
    pub fn new(
        identity: NostrIdentityReference,
        signer_binding: impl Into<SignerBinding>,
        label: Option<IdentityLabel>,
        created_at: IdentityCreatedAt,
        last_used_at: Option<UnixTimestamp>,
    ) -> Result<Self, SafeError> {
        let signer_binding = signer_binding.into();
        if identity.public_key() != signer_binding.identity() {
            return Err(invalid_identity_metadata());
        }
        Ok(Self {
            identity,
            signer_binding,
            label,
            created_at,
            last_used_at,
        })
    }

    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.identity.public_key()
    }

    #[must_use]
    pub fn npub(&self) -> &Npub {
        self.identity.npub()
    }

    #[must_use]
    pub const fn signer_binding(&self) -> SignerBinding {
        self.signer_binding
    }

    #[must_use]
    pub fn label(&self) -> Option<&IdentityLabel> {
        self.label.as_ref()
    }

    #[must_use]
    pub const fn created_at(&self) -> IdentityCreatedAt {
        self.created_at
    }

    #[must_use]
    pub const fn last_used_at(&self) -> Option<UnixTimestamp> {
        self.last_used_at
    }

    #[must_use]
    pub fn with_binding_availability(&self, availability: SignerAvailability) -> Self {
        Self {
            identity: self.identity.clone(),
            signer_binding: SignerBinding::LocalKeyring(LocalKeyringBinding::new(
                self.public_key(),
                availability,
            )),
            label: self.label.clone(),
            created_at: self.created_at,
            last_used_at: self.last_used_at,
        }
    }

    #[must_use]
    pub fn with_last_used_at(&self, last_used_at: UnixTimestamp) -> Self {
        Self {
            identity: self.identity.clone(),
            signer_binding: self.signer_binding,
            label: self.label.clone(),
            created_at: self.created_at,
            last_used_at: Some(last_used_at),
        }
    }

    #[must_use]
    pub fn display_label(&self) -> String {
        self.label
            .as_ref()
            .map_or_else(|| self.npub().short(), |label| label.as_str().to_owned())
    }
}

const fn invalid_identity_metadata() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidIdentityMetadata,
        SafeMessage::new("The identity metadata is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use crate::PublicKey;
    use crate::time::UnixTimestamp;

    use super::{
        IdentityCreatedAt, IdentityLabel, LocalKeyringBinding, NostrIdentity,
        NostrIdentityReference, SignerAvailability, SignerBinding, SignerRepairAction,
    };

    const DERIVED_NPUB: &str = "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7";
    const MISMATCHED_NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn public_key() -> PublicKey {
        PublicKey::from_bytes([7_u8; 32]).expect("valid public key")
    }

    fn identity(label: Option<IdentityLabel>) -> NostrIdentity {
        let public_key = public_key();
        NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            label,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(10).expect("valid time")),
            None,
        )
        .expect("identity")
    }

    #[test]
    fn identity_label_is_trimmed_bounded_and_control_free() {
        let label = IdentityLabel::parse("  Farm identity  ").expect("valid label");
        assert_eq!(label.as_str(), "Farm identity");

        for invalid in ["", "   ", "line\nbreak", &"x".repeat(81)] {
            assert!(IdentityLabel::parse(invalid).is_err());
        }
    }

    #[test]
    fn identity_display_prefers_label_then_shortened_npub() {
        let labelled = identity(Some(IdentityLabel::parse("Farm").expect("valid label")));
        let unlabelled = identity(None);

        assert_eq!(labelled.display_label(), "Farm");
        assert_eq!(unlabelled.display_label(), "npub1qurswpc…rsnvjvl7");
    }

    #[test]
    fn local_identity_summary_contains_public_metadata_only() {
        let identity = identity(None);
        let debug = format!("{identity:?}");

        assert_eq!(
            identity.signer_binding().availability(),
            SignerAvailability::Available
        );
        assert!(identity.label().is_none());
        assert!(identity.last_used_at().is_none());
        assert_eq!(identity.created_at().timestamp().as_seconds(), 10);
        assert_eq!(identity.public_key(), public_key());
        assert_eq!(identity.npub().as_str(), DERIVED_NPUB);
        assert!(!debug.contains("nsec1"));
        assert!(!debug.contains(&"11".repeat(32)));
    }

    #[test]
    fn nostr_identity_reference_derives_npub_and_rejects_mismatched_persisted_forms() {
        let public_key = public_key();
        let identity = NostrIdentityReference::derive(public_key).expect("identity");
        assert_eq!(identity.public_key(), public_key);
        assert_eq!(identity.npub().as_str(), DERIVED_NPUB);
        assert_eq!(
            NostrIdentityReference::verify(public_key, DERIVED_NPUB.to_owned()).expect("verified"),
            identity
        );
        assert!(NostrIdentityReference::verify(public_key, MISMATCHED_NPUB.to_owned()).is_err());
        assert!(
            NostrIdentityReference::verify(
                PublicKey::from_hex(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
                )
                .expect("second public key"),
                MISMATCHED_NPUB.to_owned()
            )
            .is_err()
        );
    }

    #[test]
    fn local_keyring_binding_carries_only_canonical_identity_reference() {
        let public_key = public_key();
        let identity = NostrIdentityReference::derive(public_key).expect("identity");
        let binding = LocalKeyringBinding::new(public_key, SignerAvailability::Available);

        assert_eq!(binding.identity(), identity.public_key());
        assert!(!format!("{binding:?}").contains("nsec1"));
    }

    #[test]
    fn signer_binding_rejects_an_identity_mismatch() {
        let identity = NostrIdentityReference::derive(public_key()).expect("identity");
        let other = PublicKey::from_bytes([8_u8; 32]).expect("other public key");
        let binding = SignerBinding::LocalKeyring(LocalKeyringBinding::new(
            other,
            SignerAvailability::Available,
        ));

        let error = NostrIdentity::new(
            identity,
            binding,
            None,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(10).expect("valid time")),
            None,
        )
        .expect_err("mismatched identity and binding");

        assert_eq!(error.code(), crate::SafeErrorCode::InvalidIdentityMetadata);
    }

    #[test]
    fn local_keyring_binding_repair_transitions_are_typed_and_fail_closed() {
        let public_key = public_key();
        let mut binding = LocalKeyringBinding::new(public_key, SignerAvailability::Available);
        assert_eq!(binding.repair_action(), None);
        assert!(binding.repair_credential().is_err());

        binding
            .mark_credential_missing()
            .expect("missing credential");
        assert_eq!(
            binding.repair_action(),
            Some(SignerRepairAction::ImportCredential)
        );
        binding.repair_credential().expect("repair");

        binding.mark_store_unavailable();
        assert_eq!(
            binding.repair_action(),
            Some(SignerRepairAction::RetryCredentialStore)
        );
        binding
            .resolve_store_recovery(false)
            .expect("store recovery");
        assert_eq!(
            binding.availability(),
            SignerAvailability::CredentialMissing
        );
        assert!(binding.resolve_store_recovery(true).is_err());
    }
}
