use std::num::NonZeroU64;
use std::sync::Mutex;
use std::time::Duration;

use radroots_studio_domain::{
    AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, LocalSignerBinding,
    Nsec, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, UnixTimestamp,
};
use radroots_studio_nostr::generate_local_keypair;

pub const GENERATED_KEY_STAGE_TTL: Duration = Duration::from_mins(5);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecoveryStageId(NonZeroU64);

impl RecoveryStageId {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedKeyStageView {
    account: AccountSummary,
    expires_at: UnixTimestamp,
}

impl GeneratedKeyStageView {
    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }

    #[must_use]
    pub const fn expires_at(&self) -> UnixTimestamp {
        self.expires_at
    }
}

pub struct StagedGeneratedKey {
    id: RecoveryStageId,
    account: AccountSummary,
    secret: SecretKeyInput,
    expected_revision: u64,
    expires_at: UnixTimestamp,
}

impl StagedGeneratedKey {
    #[must_use]
    pub fn view(&self) -> GeneratedKeyStageView {
        GeneratedKeyStageView {
            account: self.account.clone(),
            expires_at: self.expires_at,
        }
    }

    #[must_use]
    pub const fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    #[must_use]
    pub const fn id(&self) -> RecoveryStageId {
        self.id
    }

    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }

    #[must_use]
    pub fn into_commit_parts(self) -> (AccountSummary, SecretKeyInput) {
        (self.account, self.secret)
    }
}

pub struct GeneratedKeyRecoveryHandle {
    id: RecoveryStageId,
    view: GeneratedKeyStageView,
    recovery_nsec: Mutex<Option<Nsec>>,
}

impl GeneratedKeyRecoveryHandle {
    fn new(id: RecoveryStageId, view: GeneratedKeyStageView, recovery_nsec: Nsec) -> Self {
        Self {
            id,
            view,
            recovery_nsec: Mutex::new(Some(recovery_nsec)),
        }
    }

    #[must_use]
    pub const fn id(&self) -> RecoveryStageId {
        self.id
    }

    #[must_use]
    pub const fn view(&self) -> &GeneratedKeyStageView {
        &self.view
    }

    /// Returns the generated recovery value exactly once.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error after the value was already consumed.
    pub fn take_recovery_nsec(&self) -> Result<Nsec, SafeError> {
        self.recovery_nsec
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(recovery_not_available)
    }
}

#[derive(Default)]
pub struct GeneratedKeyStage {
    pending: Option<StagedGeneratedKey>,
}

impl GeneratedKeyStage {
    /// Replaces an expired stage or creates the only active generated-key stage.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict while an unexpired recovery stage is active.
    pub fn begin(
        &mut self,
        id: RecoveryStageId,
        expected_revision: u64,
        now: UnixTimestamp,
    ) -> Result<GeneratedKeyRecoveryHandle, SafeError> {
        self.expire(now);
        if self.pending.is_some() {
            return Err(recovery_in_progress());
        }
        let generated = generate_local_keypair()?;
        let (public_key, npub, secret, recovery_nsec) = generated.into_parts();
        let account = AccountSummary::new(
            AccountIdentity::verify(public_key, npub.as_str().to_owned())?,
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            None,
            AccountCreatedAt::new(now),
            None,
        )?;
        let ttl =
            i64::try_from(GENERATED_KEY_STAGE_TTL.as_secs()).map_err(|_| invalid_stage_expiry())?;
        let expires_at = now
            .as_seconds()
            .checked_add(ttl)
            .and_then(UnixTimestamp::from_seconds)
            .ok_or_else(invalid_stage_expiry)?;
        let pending = StagedGeneratedKey {
            id,
            account,
            secret,
            expected_revision,
            expires_at,
        };
        let view = pending.view();
        self.pending = Some(pending);
        Ok(GeneratedKeyRecoveryHandle::new(id, view, recovery_nsec))
    }

    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }

    pub fn expire(&mut self, now: UnixTimestamp) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now >= pending.expires_at)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }

    #[must_use]
    pub const fn pending(&self) -> Option<&StagedGeneratedKey> {
        self.pending.as_ref()
    }

    /// Consumes the active, unexpired stage for its commit boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when no live stage remains.
    pub fn take(
        &mut self,
        id: RecoveryStageId,
        now: UnixTimestamp,
    ) -> Result<StagedGeneratedKey, SafeError> {
        self.expire(now);
        if self.pending.as_ref().map(StagedGeneratedKey::id) != Some(id) {
            return Err(recovery_not_available());
        }
        self.pending.take().ok_or_else(recovery_not_available)
    }
}

const fn recovery_in_progress() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("A generated-key recovery step is already in progress."),
    )
}

const fn recovery_not_available() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The generated-key recovery step is no longer available."),
    )
}

const fn invalid_stage_expiry() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The generated-key recovery expiry is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use radroots_studio_domain::UnixTimestamp;

    use super::{GENERATED_KEY_STAGE_TTL, GeneratedKeyStage, RecoveryStageId};

    fn time(seconds: i64) -> UnixTimestamp {
        UnixTimestamp::from_seconds(seconds).expect("time")
    }

    fn id(value: u64) -> RecoveryStageId {
        RecoveryStageId::new(NonZeroU64::new(value).expect("id"))
    }

    #[test]
    fn stage_is_exclusive_cancelable_and_never_publishes_secret_debug() {
        let mut stage = GeneratedKeyStage::default();
        let handle = stage.begin(id(1), 4, time(10)).expect("begin");
        let view = handle.view();
        assert_eq!(view.expires_at().as_seconds(), 310);
        assert_eq!(stage.pending().expect("pending").expected_revision(), 4);
        assert!(stage.begin(id(2), 4, time(11)).is_err());
        let nsec = handle.take_recovery_nsec().expect("one-use recovery");
        assert_eq!(nsec.with_exposed_secret(str::len), 63);
        assert!(handle.take_recovery_nsec().is_err());
        assert!(stage.cancel());
        assert!(!stage.cancel());
        assert!(format!("{view:?}").contains(view.account().npub().as_str()));
        assert!(!format!("{view:?}").contains("nsec1"));
    }

    #[test]
    fn stage_expires_and_is_destroyed_on_owner_drop() {
        let mut stage = GeneratedKeyStage::default();
        stage.begin(id(1), 0, time(20)).expect("begin");
        let expiry = 20 + i64::try_from(GENERATED_KEY_STAGE_TTL.as_secs()).expect("ttl");
        assert!(stage.expire(time(expiry)));
        assert!(stage.pending().is_none());
        assert!(stage.take(id(1), time(expiry)).is_err());

        let mut shutdown_stage = GeneratedKeyStage::default();
        shutdown_stage.begin(id(2), 0, time(30)).expect("begin");
        drop(shutdown_stage);
    }
}
