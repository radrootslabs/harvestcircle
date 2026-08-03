use std::time::Duration;

use radroots_studio_domain::{
    AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, LocalSignerBinding,
    Nsec, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, UnixTimestamp,
};
use radroots_studio_nostr::generate_local_keypair;

pub const GENERATED_KEY_STAGE_TTL: Duration = Duration::from_mins(5);

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
    account: AccountSummary,
    secret: SecretKeyInput,
    recovery_nsec: Nsec,
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
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }

    pub fn with_recovery_nsec<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        self.recovery_nsec.with_exposed_secret(operation)
    }

    #[must_use]
    pub fn into_commit_parts(self) -> (AccountSummary, SecretKeyInput) {
        (self.account, self.secret)
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
        expected_revision: u64,
        now: UnixTimestamp,
    ) -> Result<GeneratedKeyStageView, SafeError> {
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
            account,
            secret,
            recovery_nsec,
            expected_revision,
            expires_at,
        };
        let view = pending.view();
        self.pending = Some(pending);
        Ok(view)
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
    pub fn take(&mut self, now: UnixTimestamp) -> Result<StagedGeneratedKey, SafeError> {
        self.expire(now);
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
    use radroots_studio_domain::UnixTimestamp;

    use super::{GENERATED_KEY_STAGE_TTL, GeneratedKeyStage};

    fn time(seconds: i64) -> UnixTimestamp {
        UnixTimestamp::from_seconds(seconds).expect("time")
    }

    #[test]
    fn stage_is_exclusive_cancelable_and_never_publishes_secret_debug() {
        let mut stage = GeneratedKeyStage::default();
        let view = stage.begin(4, time(10)).expect("begin");
        assert_eq!(view.expires_at().as_seconds(), 310);
        assert_eq!(stage.pending().expect("pending").expected_revision(), 4);
        assert!(stage.begin(4, time(11)).is_err());
        assert!(stage.cancel());
        assert!(!stage.cancel());
        assert!(format!("{view:?}").contains(view.account().npub().as_str()));
        assert!(!format!("{view:?}").contains("nsec1"));
    }

    #[test]
    fn stage_expires_and_is_destroyed_on_owner_drop() {
        let mut stage = GeneratedKeyStage::default();
        stage.begin(0, time(20)).expect("begin");
        let expiry = 20 + i64::try_from(GENERATED_KEY_STAGE_TTL.as_secs()).expect("ttl");
        assert!(stage.expire(time(expiry)));
        assert!(stage.pending().is_none());
        assert!(stage.take(time(expiry)).is_err());

        let mut shutdown_stage = GeneratedKeyStage::default();
        shutdown_stage.begin(0, time(30)).expect("begin");
        drop(shutdown_stage);
    }
}
