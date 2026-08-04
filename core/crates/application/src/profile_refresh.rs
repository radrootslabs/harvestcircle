use radroots_studio_domain::{PublicKey, RelayUrl, SafeError, SafeErrorCode};

use crate::{
    ActiveAccountSnapshot, AppCore, AppSnapshot, CachedProfile, Clock, NostrClient,
    ProfileLoadState, ProfileRefreshStatus, ProfileRepository, RelayConnectionState,
    SnapshotRevision, StateTransition,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileRefreshPlan {
    public_key: PublicKey,
    active_account: ActiveAccountSnapshot,
    relays: Vec<RelayUrl>,
    expected_revision: SnapshotRevision,
}

impl ProfileRefreshPlan {
    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    #[must_use]
    pub const fn active_account(&self) -> &ActiveAccountSnapshot {
        &self.active_account
    }

    #[must_use]
    pub fn relays(&self) -> &[RelayUrl] {
        &self.relays
    }

    #[must_use]
    pub const fn expected_revision(&self) -> SnapshotRevision {
        self.expected_revision
    }
}

impl AppCore {
    /// Manually refreshes the active account's Nostr kind-0 profile.
    ///
    /// Cached public metadata remains visible while the asynchronous request is
    /// running. Calling this command while signed out is an idempotent no-op.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error. Relay and invalid-data
    /// failures are represented as nonfatal snapshot state.
    pub async fn refresh_active_profile(
        &self,
        profiles: &(impl ProfileRepository + ?Sized),
        client: &(impl NostrClient + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.refresh_profile_for_active_account(profiles, client, clock)
            .await
    }

    /// Refreshes the current active account while retaining any cached profile.
    ///
    /// Stale results are discarded when the account is replaced or signed out.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error. Relay and invalid-data
    /// failures are represented as nonfatal snapshot state.
    async fn refresh_profile_for_active_account(
        &self,
        profiles: &(impl ProfileRepository + ?Sized),
        client: &(impl NostrClient + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        let Some(plan) = self.begin_profile_refresh()? else {
            return Ok(self.snapshot());
        };
        let result = client.fetch_profile(plan.public_key(), plan.relays()).await;
        self.complete_profile_refresh(&plan, result, profiles, clock)
    }

    /// Begins a refresh on the actor and returns the immutable network plan.
    ///
    /// # Errors
    ///
    /// Returns a safe state error when the loading transition is invalid.
    pub fn begin_profile_refresh(&self) -> Result<Option<ProfileRefreshPlan>, SafeError> {
        let Some(active) = self.snapshot().active_account().cloned() else {
            return Ok(None);
        };
        let public_key = active.account().public_key();
        let loading = self.apply_transition(StateTransition::UpdateActiveAccount {
            expected: public_key,
            active_account: Box::new(ActiveAccountSnapshot::new(
                active.account().clone(),
                RelayConnectionState::Connecting,
                ProfileLoadState::Loading,
                active.profile().cloned(),
            )),
            problem: None,
        })?;
        Ok(Some(ProfileRefreshPlan {
            public_key,
            active_account: active,
            relays: loading.relay_configuration().relays().to_vec(),
            expected_revision: loading.revision(),
        }))
    }

    /// Applies a correlated refresh result on the actor.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error. Stale results are
    /// discarded without persistence or publication.
    pub fn complete_profile_refresh(
        &self,
        plan: &ProfileRefreshPlan,
        result: Result<Option<radroots_studio_domain::Kind0ProfileCandidate>, SafeError>,
        profiles: &(impl ProfileRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        if !is_current_active(self, plan.public_key()) {
            return Ok(self.snapshot());
        }

        let current_active = self
            .snapshot()
            .active_account()
            .cloned()
            .ok_or_else(invalid_profile_completion)?;

        match result {
            Ok(Some(candidate)) => {
                let cached = CachedProfile::new(
                    candidate.clone(),
                    clock.now(),
                    ProfileRefreshStatus::Success,
                );
                profiles.save_profile(&cached)?;
                let winning_profile = profiles.load_profile(plan.public_key())?.map_or_else(
                    || candidate.metadata().clone(),
                    |profile| profile.candidate().metadata().clone(),
                );
                self.apply_transition(StateTransition::UpdateActiveAccount {
                    expected: plan.public_key(),
                    active_account: Box::new(ActiveAccountSnapshot::new(
                        current_active.account().clone(),
                        RelayConnectionState::Connected,
                        ProfileLoadState::Fresh,
                        Some(winning_profile),
                    )),
                    problem: None,
                })
            }
            Ok(None) => self.apply_transition(StateTransition::UpdateActiveAccount {
                expected: plan.public_key(),
                active_account: Box::new(ActiveAccountSnapshot::new(
                    current_active.account().clone(),
                    RelayConnectionState::Connected,
                    if current_active.profile().is_some() {
                        ProfileLoadState::Cached
                    } else {
                        ProfileLoadState::Empty
                    },
                    current_active.profile().cloned(),
                )),
                problem: None,
            }),
            Err(error) => {
                let status = refresh_status(error);
                profiles.record_refresh_status(plan.public_key(), clock.now(), status)?;
                self.apply_transition(StateTransition::UpdateActiveAccount {
                    expected: plan.public_key(),
                    active_account: Box::new(ActiveAccountSnapshot::new(
                        current_active.account().clone(),
                        RelayConnectionState::Degraded,
                        ProfileLoadState::Error(error),
                        current_active.profile().cloned(),
                    )),
                    problem: Some(error),
                })
            }
        }
    }
}

const fn invalid_profile_completion() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        radroots_studio_domain::SafeMessage::new("The active profile refresh is no longer valid."),
    )
}

fn is_current_active(core: &AppCore, public_key: PublicKey) -> bool {
    core.snapshot()
        .active_account()
        .is_some_and(|active| active.account().public_key() == public_key)
}

const fn refresh_status(error: SafeError) -> ProfileRefreshStatus {
    match error.code() {
        SafeErrorCode::InvalidProfileMetadata | SafeErrorCode::ProfileRefreshFailed => {
            ProfileRefreshStatus::InvalidData
        }
        _ => ProfileRefreshStatus::Offline,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use radroots_studio_domain::{
        EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, RelayUrl, SafeError,
        SafeErrorCode, SafeMessage, SecretKeyInput, UnixTimestamp, select_latest_kind0,
    };

    use crate::{
        ActiveAccountSnapshot, AppCore, BoxFuture, CachedProfile, Clock, InMemoryAccountRepository,
        InMemoryOperationJournal, InMemorySecretStore, NostrClient, ProfileLoadState,
        ProfileRefreshStatus, ProfileRepository, RelayConfiguration, RelayConnectionState,
    };

    #[derive(Default)]
    struct MemoryProfiles(Mutex<Option<CachedProfile>>);

    impl ProfileRepository for MemoryProfiles {
        fn load_profile(&self, _public_key: PublicKey) -> Result<Option<CachedProfile>, SafeError> {
            Ok(self.0.lock().expect("profiles").clone())
        }
        fn save_profile(&self, profile: &CachedProfile) -> Result<(), SafeError> {
            let mut cached = self.0.lock().expect("profiles");
            let selected = cached.as_ref().map_or_else(
                || profile.clone(),
                |current| {
                    let winner = select_latest_kind0([
                        current.candidate().clone(),
                        profile.candidate().clone(),
                    ])
                    .expect("two candidates");
                    if &winner == current.candidate() {
                        current.clone()
                    } else {
                        profile.clone()
                    }
                },
            );
            *cached = Some(selected);
            Ok(())
        }
        fn record_refresh_status(
            &self,
            _public_key: PublicKey,
            refreshed_at: UnixTimestamp,
            status: ProfileRefreshStatus,
        ) -> Result<(), SafeError> {
            if let Some(profile) = self.0.lock().expect("profiles").as_mut() {
                *profile = CachedProfile::new(profile.candidate().clone(), refreshed_at, status);
            }
            Ok(())
        }
        fn remove_profile(&self, _public_key: PublicKey) -> Result<(), SafeError> {
            *self.0.lock().expect("profiles") = None;
            Ok(())
        }
    }

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(50).expect("time")
        }
    }

    struct FixedClient(Result<Option<Kind0ProfileCandidate>, SafeError>);
    impl NostrClient for FixedClient {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
        ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
            let result = self.0.clone();
            Box::pin(async move { result })
        }
    }

    struct BlockingClient {
        started: tokio::sync::Semaphore,
        release: tokio::sync::Semaphore,
        result: Result<Option<Kind0ProfileCandidate>, SafeError>,
    }

    impl BlockingClient {
        fn new(result: Result<Option<Kind0ProfileCandidate>, SafeError>) -> Self {
            Self {
                started: tokio::sync::Semaphore::new(0),
                release: tokio::sync::Semaphore::new(0),
                result,
            }
        }
    }

    impl NostrClient for BlockingClient {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
        ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
            Box::pin(async move {
                self.started.add_permits(1);
                let permit = self.release.acquire().await.expect("release open");
                permit.forget();
                self.result.clone()
            })
        }
    }

    fn profile(public_key: PublicKey, name: &str, timestamp: i64) -> Kind0ProfileCandidate {
        Kind0ProfileCandidate::new(
            EventId::from_bytes([u8::try_from(timestamp).expect("small timestamp"); 32]),
            public_key,
            UnixTimestamp::from_seconds(timestamp).expect("time"),
            ProfileMetadata::new(Some(name.to_owned()), None, None, None, None).expect("profile"),
        )
    }

    fn active_core(profiles: &MemoryProfiles, cached_name: Option<&str>) -> (AppCore, PublicKey) {
        let relays =
            RelayConfiguration::new(vec![RelayUrl::parse("ws://localhost:8080").expect("relay")]);
        let core = AppCore::in_memory(relays);
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let public_key = core
            .import_secret_key(
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("secret"),
                &accounts,
                &accounts,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("import")
            .account()
            .public_key();
        if let Some(name) = cached_name {
            profiles
                .save_profile(&CachedProfile::new(
                    profile(public_key, name, 10),
                    UnixTimestamp::from_seconds(11).expect("time"),
                    ProfileRefreshStatus::Success,
                ))
                .expect("cache");
        }
        core.activate_account(
            public_key,
            &accounts,
            &accounts,
            profiles,
            &secrets,
            &FixedClock,
        )
        .expect("activate");
        (core, public_key)
    }

    #[tokio::test]
    async fn refresh_transitions_from_cache_through_loading_to_fresh_profile() {
        let profiles = MemoryProfiles::default();
        let (core, public_key) = active_core(&profiles, Some("Cached"));
        assert_eq!(
            core.snapshot()
                .active_account()
                .map(crate::ActiveAccountSnapshot::profile_state),
            Some(ProfileLoadState::Cached)
        );
        let plan = core
            .begin_profile_refresh()
            .expect("begin refresh")
            .expect("active refresh");
        let loading = core.snapshot();
        assert_eq!(
            loading
                .active_account()
                .map(crate::ActiveAccountSnapshot::profile_state),
            Some(ProfileLoadState::Loading)
        );
        assert_eq!(
            loading
                .active_account()
                .map(crate::ActiveAccountSnapshot::relay_state),
            Some(RelayConnectionState::Connecting)
        );
        let client = FixedClient(Ok(Some(profile(public_key, "Fresh", 20))));
        let result = client.fetch_profile(plan.public_key(), plan.relays()).await;
        core.complete_profile_refresh(&plan, result, &profiles, &FixedClock)
            .expect("complete refresh");
        assert_eq!(
            core.snapshot()
                .active_account()
                .and_then(|active| active.profile())
                .and_then(ProfileMetadata::name),
            Some("Fresh")
        );
    }

    #[tokio::test]
    async fn refresh_failure_preserves_cached_profile_as_nonfatal_state() {
        let profiles = MemoryProfiles::default();
        let (core, public_key) = active_core(&profiles, Some("Cached"));
        let cached = profile(public_key, "Cached", 10);
        let error = SafeError::new(
            SafeErrorCode::RelayConnectionFailed,
            SafeMessage::new("The relay is offline."),
        );

        let snapshot = core
            .refresh_profile_for_active_account(&profiles, &FixedClient(Err(error)), &FixedClock)
            .await
            .expect("nonfatal refresh");

        assert_eq!(snapshot.recoverable_problem(), Some(error));
        assert_eq!(
            snapshot
                .active_account()
                .map(crate::ActiveAccountSnapshot::relay_state),
            Some(RelayConnectionState::Degraded)
        );
        assert_eq!(
            profiles
                .load_profile(public_key)
                .expect("load")
                .expect("cache")
                .candidate(),
            &cached
        );
    }

    #[tokio::test]
    async fn refresh_discards_stale_completion_after_sign_out() {
        let profiles = MemoryProfiles::default();
        let (core, public_key) = active_core(&profiles, Some("Cached"));
        let client = BlockingClient::new(Ok(Some(profile(public_key, "Stale", 20))));

        let refresh = core.refresh_profile_for_active_account(&profiles, &client, &FixedClock);
        let sign_out = async {
            let permit = client.started.acquire().await.expect("refresh starts");
            permit.forget();
            core.sign_out().expect("sign out");
            client.release.add_permits(1);
        };
        let (result, ()) = tokio::join!(refresh, sign_out);

        assert!(
            result
                .expect("stale result is harmless")
                .active_account()
                .is_none()
        );
        assert_eq!(
            profiles
                .load_profile(public_key)
                .expect("load")
                .expect("cached")
                .candidate()
                .metadata()
                .name(),
            Some("Cached")
        );
    }

    #[tokio::test]
    async fn manual_refresh_is_repeatable_and_signed_out_safe() {
        let profiles = MemoryProfiles::default();
        let (core, public_key) = active_core(&profiles, None);
        let first = core
            .refresh_active_profile(
                &profiles,
                &FixedClient(Ok(Some(profile(public_key, "First", 10)))),
                &FixedClock,
            )
            .await
            .expect("first refresh");
        let second = core
            .refresh_active_profile(
                &profiles,
                &FixedClient(Ok(Some(profile(public_key, "Second", 20)))),
                &FixedClock,
            )
            .await
            .expect("second refresh");

        assert!(second.revision() > first.revision());
        assert_eq!(
            second
                .active_account()
                .and_then(|active| active.profile())
                .and_then(ProfileMetadata::name),
            Some("Second")
        );
        let signed_out = core.sign_out().expect("sign out");
        let no_op = core
            .refresh_active_profile(&profiles, &FixedClient(Ok(None)), &FixedClock)
            .await
            .expect("signed-out no-op");
        assert_eq!(no_op, signed_out);
    }

    #[test]
    fn overlapping_refreshes_keep_the_newest_event_regardless_of_completion_order() {
        let profiles = MemoryProfiles::default();
        let (core, public_key) = active_core(&profiles, Some("Cached"));
        let first = core
            .begin_profile_refresh()
            .expect("first")
            .expect("active");
        let second = core
            .begin_profile_refresh()
            .expect("second")
            .expect("active");

        core.complete_profile_refresh(
            &second,
            Ok(Some(profile(public_key, "Newest", 30))),
            &profiles,
            &FixedClock,
        )
        .expect("newest completes first");
        let final_snapshot = core
            .complete_profile_refresh(
                &first,
                Ok(Some(profile(public_key, "Older", 20))),
                &profiles,
                &FixedClock,
            )
            .expect("older completes last");

        assert_eq!(
            final_snapshot
                .active_account()
                .and_then(ActiveAccountSnapshot::profile)
                .and_then(ProfileMetadata::name),
            Some("Newest")
        );
        assert_eq!(
            profiles
                .load_profile(public_key)
                .expect("cache")
                .expect("profile")
                .candidate()
                .metadata()
                .name(),
            Some("Newest")
        );
    }
}
