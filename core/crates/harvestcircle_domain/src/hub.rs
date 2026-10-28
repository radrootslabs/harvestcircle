//! Local demo-hub identity and ownership policy.
//!
//! Actor keys here must come from the native runtime's current authenticated
//! session. Equality is a policy decision, not proof of credential possession.
//! Persistence and native session checks must protect the complete command.

pub mod movement;
pub mod portions;
pub mod reference;
pub mod stock;
pub mod treatment;

use crate::PublicKey;

/// Opaque identity minted by the host; no path, label or public key is its ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HubId([u8; 16]);

impl HubId {
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, HubPolicyError> {
        if bytes == [0; 16] {
            return Err(HubPolicyError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Revision fits the positive SQLite INTEGER range without lossy coercion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubRevision(u64);

impl HubRevision {
    pub const INITIAL: Self = Self(1);
    pub const MAXIMUM: u64 = i64::MAX as u64;

    pub fn from_value(value: u64) -> Result<Self, HubPolicyError> {
        if value == 0 || value > Self::MAXIMUM {
            return Err(HubPolicyError::InvalidRevision);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, HubPolicyError> {
        Self::from_value(self.0 + 1).map_err(|_| HubPolicyError::RevisionExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubPolicyError {
    InvalidIdentity,
    InvalidRevision,
    IdentityRequired,
    NotOwner,
    StaleRevision,
    UnchangedOwner,
    RevisionExhausted,
}

/// Hub ownership is independent of removable account rows and signer metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubOwnership {
    id: HubId,
    owner: PublicKey,
    revision: HubRevision,
}

impl HubOwnership {
    #[must_use]
    pub const fn new(id: HubId, owner: PublicKey) -> Self {
        Self::restore(id, owner, HubRevision::INITIAL)
    }

    /// Reconstruct validated persisted policy; does not authorize a session.
    #[must_use]
    pub const fn restore(id: HubId, owner: PublicKey, revision: HubRevision) -> Self {
        Self {
            id,
            owner,
            revision,
        }
    }

    #[must_use]
    pub const fn id(self) -> HubId {
        self.id
    }

    #[must_use]
    pub const fn owner(self) -> PublicKey {
        self.owner
    }

    #[must_use]
    pub const fn revision(self) -> HubRevision {
        self.revision
    }

    pub fn authorize(self, actor: Option<PublicKey>) -> Result<(), HubPolicyError> {
        match actor {
            None => Err(HubPolicyError::IdentityRequired),
            Some(actor) if actor != self.owner => Err(HubPolicyError::NotOwner),
            Some(_) => Ok(()),
        }
    }

    /// Computes a handover after native verification of both local credentials.
    /// The adapter must persist ownership and its audit event atomically, and
    /// must never accept the actor or replacement readiness from a UI claim.
    pub fn transfer(
        self,
        actor: Option<PublicKey>,
        expected: HubRevision,
        replacement: PublicKey,
    ) -> Result<Self, HubPolicyError> {
        self.authorize(actor)?;
        if expected != self.revision {
            return Err(HubPolicyError::StaleRevision);
        }
        if replacement == self.owner {
            return Err(HubPolicyError::UnchangedOwner);
        }
        Ok(Self::restore(self.id, replacement, self.revision.next()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: u8) -> PublicKey {
        let encoded = match value {
            2 => "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            3 => "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
            _ => panic!("unknown public test identity"),
        };
        PublicKey::from_hex(encoded).expect("public key")
    }

    fn ownership() -> HubOwnership {
        HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), key(2))
    }

    #[test]
    fn identity_and_revisions_reject_invalid_persisted_values() {
        assert_eq!(
            HubId::from_bytes([0; 16]),
            Err(HubPolicyError::InvalidIdentity)
        );
        for value in [0, HubRevision::MAXIMUM + 1, u64::MAX] {
            assert_eq!(
                HubRevision::from_value(value),
                Err(HubPolicyError::InvalidRevision)
            );
        }
        assert_eq!(ownership().id().as_bytes(), &[1; 16]);
        assert_eq!(
            HubRevision::from_value(HubRevision::MAXIMUM)
                .unwrap()
                .value(),
            HubRevision::MAXIMUM
        );
    }

    #[test]
    fn missing_and_different_actors_have_no_owner_authority() {
        let policy = ownership();
        assert_eq!(
            policy.authorize(None),
            Err(HubPolicyError::IdentityRequired)
        );
        assert_eq!(
            policy.authorize(Some(key(3))),
            Err(HubPolicyError::NotOwner)
        );
        assert_eq!(policy.authorize(Some(key(2))), Ok(()));
    }

    #[test]
    fn handover_preserves_hub_identity_and_revokes_previous_owner() {
        let original = ownership();
        let changed = original
            .transfer(Some(key(2)), original.revision(), key(3))
            .unwrap();
        assert_eq!(changed.id(), original.id());
        assert_eq!(changed.owner(), key(3));
        assert_eq!(changed.revision().value(), 2);
        assert_eq!(
            changed.authorize(Some(key(2))),
            Err(HubPolicyError::NotOwner)
        );
        assert_eq!(changed.authorize(Some(key(3))), Ok(()));
        assert_eq!(original.owner(), key(2));
    }

    #[test]
    fn stale_handover_and_non_owner_cannot_change_policy() {
        let policy = ownership();
        let stale = HubRevision::from_value(2).unwrap();
        assert_eq!(
            policy.transfer(Some(key(2)), stale, key(3)),
            Err(HubPolicyError::StaleRevision)
        );
        assert_eq!(
            policy.transfer(Some(key(3)), stale, key(3)),
            Err(HubPolicyError::NotOwner)
        );
        assert_eq!(
            policy.transfer(None, stale, key(3)),
            Err(HubPolicyError::IdentityRequired)
        );
        assert_eq!(policy.revision(), HubRevision::INITIAL);
    }

    #[test]
    fn noop_and_exhausted_handover_have_no_success_result() {
        let policy = ownership();
        assert_eq!(
            policy.transfer(Some(key(2)), policy.revision(), key(2)),
            Err(HubPolicyError::UnchangedOwner)
        );
        let full = HubOwnership::restore(
            policy.id(),
            policy.owner(),
            HubRevision::from_value(HubRevision::MAXIMUM).unwrap(),
        );
        assert_eq!(
            full.transfer(Some(key(2)), full.revision(), key(3)),
            Err(HubPolicyError::RevisionExhausted)
        );
        assert_eq!(full.owner(), key(2));
    }
}
