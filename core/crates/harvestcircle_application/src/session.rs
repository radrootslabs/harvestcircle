use crate::{
    ActiveIdentitySnapshot, AppCore, AppSnapshot, AppStateRepository, Clock, IdentityRepository,
    ProfileLoadState, ProfileRepository, RelayConnectionState, SecretStore, StateTransition,
};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage};

impl AppCore {
    /// Drops the active session while retaining identities, selection, and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error if the transition cannot be applied.
    pub fn sign_out(&self) -> Result<AppSnapshot, SafeError> {
        if matches!(self.snapshot().session(), crate::SessionState::SignedOut) {
            return Ok(self.snapshot());
        }
        self.apply_transition(StateTransition::SignOut)
    }

    /// Validates and prepares a saved local identity before replacing the active session.
    ///
    /// # Errors
    ///
    /// Returns a safe identity, credential, profile-cache, persistence, or state
    /// error while preserving any previously active session.
    pub fn activate_identity(
        &self,
        public_key: PublicKey,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        profiles: &(impl ProfileRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        let identity = identities
            .find_identity(public_key)?
            .ok_or_else(identity_not_found)?;
        self.apply_transition(StateTransition::BeginActivation(public_key))?;
        let prepared = (|| {
            let credential = secrets.load(public_key)?;
            let imported = self.key_material().import(credential)?;
            let (derived_public_key, _npub, canonical_secret) = imported.into_parts();
            drop(canonical_secret);
            if derived_public_key != public_key {
                return Err(invalid_credential());
            }
            let cached = profiles.load_profile(public_key)?;
            let active = ActiveIdentitySnapshot::new(
                identity.with_last_used_at(clock.now()),
                RelayConnectionState::Disconnected,
                if cached.is_some() {
                    ProfileLoadState::Cached
                } else {
                    ProfileLoadState::Empty
                },
                cached.map(|profile| profile.candidate().metadata().clone()),
            );
            identities.update_identity(active.identity())?;
            app_state.save_selected_identity(Some(public_key))?;
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

const fn identity_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityNotFound,
        SafeMessage::new("The identity was not found."),
    )
}

const fn invalid_credential() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidSecretKey,
        SafeMessage::new("The Nostr identity credential is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};

    use crate::{
        AppCore, CachedProfile, Clock, InMemoryIdentityRepository, InMemoryOperationJournal,
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
    fn activate_identity_switches_only_after_candidate_is_ready() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        let profiles = EmptyProfiles;
        core.bootstrap().expect("bootstrap");
        let first = core
            .import_secret_key(
                input("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("first")
            .identity()
            .public_key();
        let second = core
            .import_secret_key(
                input("1111111111111111111111111111111111111111111111111111111111111111"),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("second")
            .identity()
            .public_key();
        let activated = core
            .activate_identity(
                first,
                &identities,
                &identities,
                &profiles,
                &secrets,
                &FixedClock,
            )
            .expect("activate first");
        assert_eq!(core.snapshot().session(), SessionState::Active);
        assert_eq!(
            core.snapshot()
                .active_identity()
                .map(|active| active.identity().public_key()),
            Some(first)
        );
        let registered = activated
            .identities()
            .iter()
            .find(|identity| identity.public_key() == first)
            .expect("activated identity remains registered");
        let active = activated.active_identity().expect("active identity");
        assert_eq!(registered, active.identity());
        assert_eq!(registered.last_used_at(), Some(FixedClock.now()));

        secrets.delete(second).expect("remove second credential");
        let error = core
            .activate_identity(
                second,
                &identities,
                &identities,
                &profiles,
                &secrets,
                &FixedClock,
            )
            .expect_err("missing credential");
        assert_eq!(
            error.code(),
            harvestcircle_domain::SafeErrorCode::CredentialMissing
        );
        secrets
            .put(
                second,
                input("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"),
            )
            .expect("mismatched credential");
        let invalid = core
            .activate_identity(
                second,
                &identities,
                &identities,
                &profiles,
                &secrets,
                &FixedClock,
            )
            .expect_err("mismatched credential");
        assert_eq!(
            invalid.code(),
            harvestcircle_domain::SafeErrorCode::InvalidSecretKey
        );
        assert_eq!(core.snapshot().session(), SessionState::Active);
        assert_eq!(
            core.snapshot()
                .active_identity()
                .map(|active| active.identity().public_key()),
            Some(first)
        );
    }

    #[test]
    fn sign_out_retains_saved_identity_selection_and_credential() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        let profiles = EmptyProfiles;
        core.bootstrap().expect("bootstrap");
        let public_key = core
            .import_secret_key(
                input("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("import")
            .identity()
            .public_key();
        core.activate_identity(
            public_key,
            &identities,
            &identities,
            &profiles,
            &secrets,
            &FixedClock,
        )
        .expect("activate");

        let signed_out = core.sign_out().expect("sign out");
        let repeated = core.sign_out().expect("idempotent sign out");
        assert_eq!(signed_out, repeated);
        assert_eq!(signed_out.session(), SessionState::SignedOut);
        assert!(signed_out.active_identity().is_none());
        assert_eq!(signed_out.identities().len(), 1);
        assert_eq!(signed_out.selected_identity(), Some(public_key));
        assert!(secrets.contains(public_key).expect("credential retained"));
    }
}
