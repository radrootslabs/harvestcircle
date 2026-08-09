use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

use radroots_studio_application::ChangeSubscriptionId;

use crate::commands::RuntimeCore;
use crate::{AppSnapshotDto, StudioAppCore, StudioError};

const OBSERVER_CHANGE_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(63);
const MAX_OBSERVERS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct SnapshotChangeDto {
    pub snapshot: AppSnapshotDto,
    pub previous_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct ShutdownReceiptDto {
    pub final_revision: u64,
    pub closed: bool,
}

#[cfg_attr(not(coverage_nightly), uniffi::export(callback_interface))]
pub trait StudioChangeObserver: Send + Sync {
    fn on_change(&self, change: SnapshotChangeDto);
}

#[cfg_attr(not(coverage_nightly), derive(uniffi::Object))]
pub struct ObserverSubscription {
    core: Weak<RuntimeCore>,
    id: Mutex<Option<ChangeSubscriptionId>>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl ObserverSubscription {
    pub async fn unsubscribe(&self) {
        let id = self
            .id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let (Some(core), Some(id)) = (self.core.upgrade(), id) else {
            return;
        };
        let task = {
            core.observers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id)
        };
        if let Some(Some(task)) = task {
            task.abort();
            let _ = task.await;
        }
        let _ = core.actor.unsubscribe_changes(id).await;
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl StudioAppCore {
    /// Subscribes to ordered revision changes including predecessor metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe observer or lifecycle error.
    pub async fn subscribe_changes_v2(
        &self,
        observer: Box<dyn StudioChangeObserver>,
    ) -> Result<Arc<ObserverSubscription>, StudioError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(closed_error());
        }
        let mut subscription = self
            .inner
            .actor
            .subscribe_changes(OBSERVER_CHANGE_CAPACITY)
            .await
            .map_err(StudioError::from)?;
        let id = subscription.id();
        let observer: Arc<dyn StudioChangeObserver> = Arc::from(observer);
        let runtime_core = Arc::downgrade(&self.inner);
        let admitted = {
            let mut observers = self
                .inner
                .observers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.inner.closed.load(Ordering::Acquire) || observers.len() >= MAX_OBSERVERS {
                false
            } else {
                observers.insert(id, None);
                true
            }
        };
        if !admitted {
            self.inner
                .actor
                .unsubscribe_changes(id)
                .await
                .map_err(StudioError::from)?;
            return Err(observer_registration_error());
        }
        let task = crate::commands::runtime()?.spawn(async move {
            while let Some(change) = subscription.receive().await {
                let Some(runtime_core) = runtime_core.upgrade() else {
                    break;
                };
                let delivery = SnapshotChangeDto {
                    snapshot: AppSnapshotDto::from_runtime(
                        change.snapshot(),
                        runtime_core.effective_lifecycle(),
                    ),
                    previous_revision: change
                        .previous_revision()
                        .map(radroots_studio_application::SnapshotRevision::value),
                };
                if catch_unwind(AssertUnwindSafe(|| observer.on_change(delivery))).is_err() {
                    break;
                }
            }
            if let Some(runtime_core) = runtime_core.upgrade() {
                let _ = runtime_core.actor.unsubscribe_changes(id).await;
                runtime_core
                    .observers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
            }
        });
        let retained = {
            let mut observers = self
                .inner
                .observers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(slot) = observers.get_mut(&id) {
                *slot = Some(task);
                true
            } else {
                task.abort();
                false
            }
        };
        if !retained {
            let _ = self.inner.actor.unsubscribe_changes(id).await;
            return Err(closed_error());
        }
        Ok(Arc::new(ObserverSubscription {
            core: Arc::downgrade(&self.inner),
            id: Mutex::new(Some(id)),
        }))
    }

    /// Stops observer delivery and waits for actor-owned shutdown.
    ///
    /// # Errors
    ///
    /// Returns a safe closed or timeout error when shutdown cannot complete.
    pub async fn shutdown_v2(&self) -> Result<ShutdownReceiptDto, StudioError> {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return Err(closed_error());
        }
        let handles = std::mem::take(
            &mut *self
                .inner
                .observers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for (_, task) in handles {
            if let Some(task) = task {
                task.abort();
                let _ = task.await;
            }
        }
        self.inner.actor.close().await.map_err(StudioError::from)?;
        Ok(ShutdownReceiptDto {
            final_revision: self.inner.actor.snapshot().revision().value(),
            closed: true,
        })
    }
}

fn closed_error() -> StudioError {
    StudioError::Failure {
        code: crate::WireErrorCode::InvalidApplicationState,
        category: crate::WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: crate::WireRecoveryAction::None,
        correlation_id: None,
        safe_message: "The application runtime is closed.".to_owned(),
    }
}

fn observer_registration_error() -> StudioError {
    StudioError::Failure {
        code: crate::WireErrorCode::ObserverRegistrationFailed,
        category: crate::WireErrorCategory::Lifecycle,
        retryable: true,
        recovery_action: crate::WireRecoveryAction::Retry,
        correlation_id: None,
        safe_message: "The change observer could not be registered.".to_owned(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_studio_application::{InMemorySecretStore, RelayConfiguration};
    use radroots_studio_domain::{RelayDestinationPolicy, RelayUrl};
    use radroots_studio_nostr::SdkNostrClient;
    use radroots_studio_runtime::{
        RuntimeActorHandle, RuntimeDependencies, UuidInstallationIdentitySource,
    };

    use crate::commands::{ACTOR_MAILBOX_CAPACITY, RuntimeCore, SystemClock, runtime};
    use crate::{
        AppSnapshotDto, ProfileLoadStateDto, SnapshotChangeDto, StudioAppCore, StudioChangeObserver,
    };

    const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

    #[derive(Default)]
    struct RecordingObserver {
        snapshots: Mutex<Vec<AppSnapshotDto>>,
        core: Mutex<Option<Arc<StudioAppCore>>>,
    }

    struct PanickingObserver;

    impl StudioChangeObserver for PanickingObserver {
        fn on_change(&self, _change: SnapshotChangeDto) {
            panic!("injected host callback failure");
        }
    }

    impl StudioChangeObserver for RecordingObserver {
        fn on_change(&self, change: SnapshotChangeDto) {
            let snapshot = change.snapshot;
            if let Some(core) = self.core.lock().expect("core").as_ref() {
                assert_eq!(core.snapshot().revision, snapshot.revision);
            }
            self.snapshots.lock().expect("snapshots").push(snapshot);
        }
    }

    async fn core() -> Arc<StudioAppCore> {
        core_with_relays(RelayConfiguration::default()).await
    }

    async fn core_with_relays(relays: RelayConfiguration) -> Arc<StudioAppCore> {
        let actor = RuntimeActorHandle::in_memory(
            relays,
            RuntimeDependencies::new(
                Arc::new(InMemorySecretStore::default()),
                Arc::new(SystemClock),
                Arc::new(SdkNostrClient::new(std::time::Duration::from_millis(10))),
                Arc::new(UuidInstallationIdentitySource),
            ),
            NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).expect("capacity"),
            runtime().expect("runtime").handle(),
        )
        .await
        .expect("actor");
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: Mutex::new(std::collections::BTreeMap::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
                startup_relay_problem: None,
            }),
        })
    }

    #[test]
    fn callbacks_allow_reentry_and_stop_after_subscription_close() {
        runtime().expect("runtime").block_on(async {
            let core = core().await;
            let observer = Arc::new(RecordingObserver::default());
            *observer.core.lock().expect("core") = Some(Arc::clone(&core));
            let subscription = core
                .subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
                .await
                .expect("subscribe");
            wait_for_snapshot_count(&observer, 1).await;
            core.inner
                .actor
                .bootstrap()
                .await
                .expect("idempotent bootstrap");
            assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
            subscription.unsubscribe().await;
            subscription.unsubscribe().await;
            core.inner.actor.sign_out().await.expect("sign out");
            assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
        });
    }

    #[test]
    fn core_close_deregisters_all_observers_and_rejects_new_subscriptions() {
        runtime().expect("runtime").block_on(async {
            let core = core().await;
            let observer = Arc::new(RecordingObserver::default());
            let subscription = core
                .subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
                .await
                .expect("subscribe");
            let _active_subscription = core
                .subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
                .await
                .expect("second subscription");
            let id = subscription
                .id
                .lock()
                .expect("subscription id")
                .expect("active subscription id");
            let handle = core
                .inner
                .observers
                .lock()
                .expect("observers")
                .get_mut(&id)
                .expect("registered observer")
                .take()
                .expect("observer task");
            handle.abort();

            core.shutdown_v2().await.expect("shutdown");
            assert!(core.shutdown_v2().await.is_err());

            assert!(
                core.subscribe_changes_v2(Box::new(ArcObserver(observer)))
                    .await
                    .is_err()
            );
            assert!(core.inner.observers.lock().expect("observers").is_empty());
        });
    }

    #[test]
    fn subscription_unsubscribe_tolerates_a_dropped_runtime_core() {
        runtime().expect("runtime").block_on(async {
            let core = core().await;
            let observer = Arc::new(RecordingObserver::default());
            let subscription = core
                .subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
                .await
                .expect("subscribe");
            wait_for_snapshot_count(&observer, 1).await;

            drop(core);
            subscription.unsubscribe().await;
        });
    }

    #[test]
    fn observer_registration_is_bounded_and_callback_panics_are_contained() {
        runtime().expect("runtime").block_on(async {
            let core = core().await;
            let panic_subscription = core
                .subscribe_changes_v2(Box::new(PanickingObserver))
                .await
                .expect("panic observer registration");
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(core.inner.observers.lock().expect("observers").is_empty());
            panic_subscription.unsubscribe().await;

            let observer = Arc::new(RecordingObserver::default());
            let mut subscriptions = Vec::new();
            for _ in 0..super::MAX_OBSERVERS {
                subscriptions.push(
                    core.subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
                        .await
                        .expect("bounded observer registration"),
                );
            }
            assert!(
                core.subscribe_changes_v2(Box::new(ArcObserver(observer)))
                    .await
                    .is_err()
            );
            for subscription in subscriptions {
                subscription.unsubscribe().await;
            }
            assert!(core.inner.observers.lock().expect("observers").is_empty());
        });
    }

    #[tokio::test]
    async fn ffi_callback_receives_async_profile_refresh_and_stops_after_unsubscribe() {
        let local_relay = MockRelay::run().await.expect("local relay");
        let relay_url = local_relay.url().await;
        let publisher = Client::new(Keys::parse(SECRET_HEX).expect("known key"));
        publisher
            .add_relay(relay_url.clone())
            .await
            .expect("publisher relay");
        publisher.connect().await;
        publisher.wait_for_connection(Duration::from_secs(2)).await;
        publisher
            .send_event_builder(EventBuilder::metadata(
                &Metadata::new().display_name("FFI Profile"),
            ))
            .await
            .expect("publish profile");

        let core = core_with_relays(
            RelayConfiguration::new(vec![
                RelayUrl::parse(relay_url.as_str(), RelayDestinationPolicy::Local)
                    .expect("relay URL"),
            ])
            .expect("relay configuration"),
        )
        .await;
        core.bootstrap().await.expect("bootstrap");
        let observer = Arc::new(RecordingObserver::default());
        *observer.core.lock().expect("core") = Some(Arc::clone(&core));
        let subscription = core
            .subscribe_changes_v2(Box::new(ArcObserver(observer.clone())))
            .await
            .expect("subscribe");
        let imported = core
            .import_account_v2(
                crate::RequestContextDto {
                    request_id: "observer-import".to_owned(),
                    expected_revision: core.snapshot().revision,
                    deadline_millis: 5_000,
                },
                SECRET_HEX.as_bytes().to_vec(),
            )
            .await
            .expect("import")
            .snapshot;
        let public_key = imported.selected_public_key_hex.expect("selection");
        core.activate_account(public_key).await.expect("activate");
        core.refresh_active_profile().await.expect("refresh");

        wait_for_fresh_profile(&observer).await;
        let snapshots = observer.snapshots.lock().expect("snapshots").clone();
        assert!(snapshots.iter().any(|snapshot| {
            snapshot.active_account.as_ref().is_some_and(|active| {
                active.profile_state == ProfileLoadStateDto::Fresh
                    && active
                        .profile
                        .as_ref()
                        .and_then(|profile| profile.display_name.as_deref())
                        == Some("FFI Profile")
            })
        }));
        subscription.unsubscribe().await;
        let count = observer.snapshots.lock().expect("snapshots").len();
        core.sign_out().await.expect("sign out");
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), count);

        core.shutdown_v2().await.expect("shutdown");
        publisher.shutdown().await;
        local_relay.shutdown();
    }

    struct ArcObserver(Arc<RecordingObserver>);

    impl StudioChangeObserver for ArcObserver {
        fn on_change(&self, change: SnapshotChangeDto) {
            self.0.on_change(change);
        }
    }

    async fn wait_for_snapshot_count(observer: &RecordingObserver, minimum: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while observer.snapshots.lock().expect("snapshots").len() < minimum {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("snapshot delivery");
    }

    async fn wait_for_fresh_profile(observer: &RecordingObserver) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let fresh = observer
                    .snapshots
                    .lock()
                    .expect("snapshots")
                    .iter()
                    .any(|snapshot| {
                        snapshot.active_account.as_ref().is_some_and(|active| {
                            active.profile_state == ProfileLoadStateDto::Fresh
                        })
                    });
                if fresh {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fresh profile delivery");
    }
}
