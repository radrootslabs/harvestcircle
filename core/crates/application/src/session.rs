use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage};
use radroots_studio_nostr::import_secret;

use crate::{
    AccountRepository, ActiveAccountSnapshot, AppCore, AppSnapshot, AppStateRepository, Clock,
    ProfileLoadState, ProfileRepository, RelayConnectionState, SecretStore, StateTransition,
};

impl AppCore {
    /// Validates and prepares a saved local account before replacing the active session.
    ///
    /// # Errors
    ///
    /// Returns a safe account, credential, profile-cache, persistence, or state
    /// error while preserving any previously active session.
    pub fn activate_account(
        &self,
        public_key: PublicKey,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        profiles: &(impl ProfileRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        let account = accounts
            .find_account(public_key)?
            .ok_or_else(account_not_found)?;
        self.apply_transition(StateTransition::BeginActivation(public_key))?;
        let prepared = (|| {
            let credential = secrets.load(public_key)?;
            let imported = import_secret(credential)?;
            let (derived_public_key, _npub, canonical_secret) = imported.into_parts();
            drop(canonical_secret);
            if derived_public_key != public_key {
                return Err(invalid_credential());
            }
            let cached = profiles.load_profile(public_key)?;
            let active = ActiveAccountSnapshot::new(
                account.with_last_used_at(clock.now()),
                RelayConnectionState::Disconnected,
                if cached.is_some() {
                    ProfileLoadState::Cached
                } else {
                    ProfileLoadState::Empty
                },
                cached.map(|profile| profile.candidate().metadata().clone()),
            );
            accounts.update_account(active.account())?;
            app_state.save_selected_account(Some(public_key))?;
            Ok(active)
        })();
        match prepared {
            Ok(active) => {
                self.apply_transition(StateTransition::ActivationSucceeded(Box::new(active)))
            }
            Err(error) => {
                self.apply_transition(StateTransition::ActivationFailed(error))?;
                Err(error)
            }
        }
    }
}

const fn account_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountNotFound,
        SafeMessage::new("The account was not found."),
    )
}

const fn invalid_credential() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidSecretKey,
        SafeMessage::new("The Nostr account credential is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};

    use crate::{
        AppCore, CachedProfile, Clock, InMemoryAccountRepository, InMemoryOperationJournal,
        InMemorySecretStore, ProfileRefreshStatus, ProfileRepository, RelayConfiguration,
        SecretStore, SessionState,
    };

    #[derive(Default)]
    struct EmptyProfiles;

    impl ProfileRepository for EmptyProfiles {
        fn load_profile(&self, _public_key: PublicKey) -> Result<Option<CachedProfile>, SafeError> {
            Ok(None)
        }

        fn save_profile(&self, _profile: &CachedProfile) -> Result<(), SafeError> {
            Ok(())
        }

        fn record_refresh_status(
            &self,
            _public_key: PublicKey,
            _refreshed_at: UnixTimestamp,
            _status: ProfileRefreshStatus,
        ) -> Result<(), SafeError> {
            Ok(())
        }

        fn remove_profile(&self, _public_key: PublicKey) -> Result<(), SafeError> {
            Ok(())
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(30).expect("time")
        }
    }

    fn input(value: &str) -> SecretKeyInput {
        SecretKeyInput::parse(value.to_owned()).expect("input")
    }

    #[test]
    fn activate_account_switches_only_after_candidate_is_ready() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        let profiles = EmptyProfiles;
        core.bootstrap().expect("bootstrap");
        let first = core
            .import_secret_key(
                input("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"),
                &accounts,
                &accounts,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("first")
            .account()
            .public_key();
        let second = core
            .import_secret_key(
                input("1111111111111111111111111111111111111111111111111111111111111111"),
                &accounts,
                &accounts,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("second")
            .account()
            .public_key();
        core.activate_account(
            first,
            &accounts,
            &accounts,
            &profiles,
            &secrets,
            &FixedClock,
        )
        .expect("activate first");
        assert_eq!(core.snapshot().session(), SessionState::Active);
        assert_eq!(
            core.snapshot()
                .active_account()
                .map(|active| active.account().public_key()),
            Some(first)
        );

        secrets.delete(second).expect("remove second credential");
        let error = core
            .activate_account(
                second,
                &accounts,
                &accounts,
                &profiles,
                &secrets,
                &FixedClock,
            )
            .expect_err("missing credential");
        assert_eq!(
            error.code(),
            radroots_studio_domain::SafeErrorCode::CredentialMissing
        );
        assert_eq!(core.snapshot().session(), SessionState::Active);
        assert_eq!(
            core.snapshot()
                .active_account()
                .map(|active| active.account().public_key()),
            Some(first)
        );
    }
}
