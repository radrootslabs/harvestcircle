use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

use harvestcircle_application::ChangeSubscriptionId;

use crate::commands::RuntimeCore;
use crate::{AppSnapshotDto, HarvestCircleAppCore, HarvestCircleError};

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
pub trait HarvestCircleChangeObserver: Send + Sync {
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
        let id = {
            let Ok(mut retained_id) = self.id.lock() else {
                return;
            };
            retained_id.take()
        };
        let (Some(core), Some(id)) = (self.core.upgrade(), id) else {
            return;
        };
        let task = {
            let Ok(mut observers) = core.observers.lock() else {
                return;
            };
            observers.remove(&id)
        };
        if let Some(Some(task)) = task {
            task.abort();
            let _ = task.await;
        }
        let _ = core.actor.unsubscribe_changes(id).await;
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl HarvestCircleAppCore {
    /// Subscribes to ordered revision changes including predecessor metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe observer or lifecycle error.
    pub async fn subscribe_changes_v2(
        &self,
        observer: Box<dyn HarvestCircleChangeObserver>,
    ) -> Result<Arc<ObserverSubscription>, HarvestCircleError> {
        if !self.inner.is_open() {
            return Err(closed_error());
        }
        let mut subscription = self
            .inner
            .actor
            .subscribe_changes(OBSERVER_CHANGE_CAPACITY)
            .await
            .map_err(HarvestCircleError::from)?;
        let id = subscription.id();
        let observer: Arc<dyn HarvestCircleChangeObserver> = Arc::from(observer);
        let runtime_core = Arc::downgrade(&self.inner);
        let admitted = {
            let mut observers = self
                .inner
                .observers
                .lock()
                .map_err(|_| observer_registration_error())?;
            if !self.inner.is_open() || observers.len() >= MAX_OBSERVERS {
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
                .map_err(HarvestCircleError::from)?;
            return Err(observer_registration_error());
        }
        let task = self.inner.runtime.spawn(async move {
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
                        .map(harvestcircle_application::SnapshotRevision::value),
                };
                if catch_unwind(AssertUnwindSafe(|| observer.on_change(delivery))).is_err() {
                    break;
                }
            }
            if let Some(runtime_core) = runtime_core.upgrade() {
                let _ = runtime_core.actor.unsubscribe_changes(id).await;
                if let Ok(mut observers) = runtime_core.observers.lock() {
                    observers.remove(&id);
                }
            }
        });
        let retained = {
            let mut observers = self.inner.observers.lock().map_err(|_| {
                task.abort();
                observer_registration_error()
            })?;
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

    /// Stops admission and waits for observer, actor, keyring, and runtime shutdown.
    ///
    /// Once shutdown begins, dropping or cancelling the calling future does
    /// not reopen admission. A later call resumes the same close sequence and
    /// successful calls are idempotent.
    ///
    /// # Errors
    ///
    /// Returns a safe closed or timeout error when shutdown cannot complete.
    pub async fn shutdown_v2(&self) -> Result<ShutdownReceiptDto, HarvestCircleError> {
        let _ = self
            .inner
            .close_state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        let _close = self.inner.close_gate.lock().await;
        if self.inner.close_state.load(Ordering::Acquire) == 2 {
            return Ok(ShutdownReceiptDto {
                final_revision: self.inner.actor.snapshot().revision().value(),
                closed: true,
            });
        }
        let handles = std::mem::take(
            &mut *self
                .inner
                .observers
                .lock()
                .map_err(|_| crate::commands::internal_state_unavailable())?,
        );
        let tasks = handles
            .into_values()
            .flatten()
            .collect::<Vec<tokio::task::JoinHandle<()>>>();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
        self.inner
            .actor
            .close()
            .await
            .map_err(HarvestCircleError::from)?;
        if let Some(keyring) = self.inner.keyring.as_ref() {
            keyring.close().await.map_err(HarvestCircleError::from)?;
        }
        if let Some(runtime) = self.inner.host_runtime.as_ref() {
            runtime.shutdown().await.map_err(|()| closed_error())?;
        }
        self.inner.close_state.store(2, Ordering::Release);
        Ok(ShutdownReceiptDto {
            final_revision: self.inner.actor.snapshot().revision().value(),
            closed: true,
        })
    }
}

fn closed_error() -> HarvestCircleError {
    crate::commands::runtime_closed_error()
}

fn observer_registration_error() -> HarvestCircleError {
    HarvestCircleError::Failure {
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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use harvestcircle_application::{
        RelayAccess, RelayConfiguration, RelayEndpoint, RelayUrlPolicy,
    };
    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;

    use crate::commands::{RuntimeCore, test_actor};
    use crate::{
        AppSnapshotDto, HarvestCircleAppCore, HarvestCircleChangeObserver, ProfileLoadStateDto,
        SnapshotChangeDto,
    };

    const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
    #[derive(Default)]
    struct RecordingObserver {
        snapshots: Mutex<Vec<AppSnapshotDto>>,
        core: Mutex<Option<Arc<HarvestCircleAppCore>>>,
    }

    struct PanickingObserver;

    impl HarvestCircleChangeObserver for PanickingObserver {
        fn on_change(&self, _change: SnapshotChangeDto) {
            panic!("injected host callback failure");
        }
    }

    impl HarvestCircleChangeObserver for RecordingObserver {
        fn on_change(&self, change: SnapshotChangeDto) {
            let snapshot = change.snapshot;
            if let Some(core) = self.core.lock().expect("core").as_ref() {
                assert_eq!(core.snapshot().revision, snapshot.revision);
            }
            self.snapshots.lock().expect("snapshots").push(snapshot);
        }
    }

    async fn core() -> Arc<HarvestCircleAppCore> {
        core_with_relays(RelayConfiguration::default()).await
    }

    async fn core_with_relays(relays: RelayConfiguration) -> Arc<HarvestCircleAppCore> {
        let (actor, directory) = test_actor(relays).await;
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: tokio::runtime::Handle::current(),
                host_runtime: None,
                keyring: None,
                observers: Mutex::new(std::collections::BTreeMap::new()),
                close_state: std::sync::atomic::AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                _test_directory: Some(directory),
            }),
        })
    }

    async fn core_with_host_runtime(
        host_runtime: Arc<crate::host_runtime::HostRuntime>,
    ) -> Arc<HarvestCircleAppCore> {
        let (actor, directory) = test_actor(RelayConfiguration::default()).await;
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: tokio::runtime::Handle::current(),
                host_runtime: Some(host_runtime),
                keyring: None,
                observers: Mutex::new(std::collections::BTreeMap::new()),
                close_state: std::sync::atomic::AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                _test_directory: Some(directory),
            }),
        })
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().expect("test runtime")
    }

    #[test]
    fn callbacks_allow_reentry_and_stop_after_subscription_close() {
        test_runtime().block_on(async {
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
        test_runtime().block_on(async {
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

            let first = core.shutdown_v2().await.expect("shutdown");
            let repeated = core.shutdown_v2().await.expect("repeated shutdown");
            assert_eq!(repeated, first);

            assert!(
                core.subscribe_changes_v2(Box::new(ArcObserver(observer)))
                    .await
                    .is_err()
            );
            assert!(core.inner.observers.lock().expect("observers").is_empty());
        });
    }

    #[tokio::test]
    async fn cancelled_host_close_remains_non_admitting_and_resumes() {
        let gated = crate::host_runtime::HostRuntime::new_completion_gated_for_test()
            .expect("host runtime");
        let core = core_with_host_runtime(gated.runtime).await;
        let closing_core = Arc::clone(&core);
        let closing = tokio::spawn(async move { closing_core.shutdown_v2().await });
        tokio::task::spawn_blocking(move || gated.entered.recv())
            .await
            .expect("entered join")
            .expect("close reached host completion gate");
        closing.abort();
        assert!(closing.await.is_err());
        assert!(
            core.subscribe_changes_v2(Box::new(PanickingObserver))
                .await
                .is_err()
        );
        gated.release.send(()).expect("release host close");
        assert!(core.shutdown_v2().await.expect("resumed close").closed);
    }

    #[test]
    fn subscription_unsubscribe_tolerates_a_dropped_runtime_core() {
        test_runtime().block_on(async {
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
        test_runtime().block_on(async {
            let core = core().await;
            let panic_subscription = core
                .subscribe_changes_v2(Box::new(PanickingObserver))
                .await
                .expect("panic observer registration");
            wait_for_observer_count(&core, 0).await;
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
                RelayEndpoint::new(
                    relay_url.as_str(),
                    RelayUrlPolicy::Local,
                    RelayAccess::ReadWrite,
                )
                .expect("relay endpoint"),
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
            .import_identity(
                crate::RequestContextDto {
                    request_id: "01890f3e-7b1c-7000-8000-000000000049".to_owned(),
                    expected_revision: core.snapshot().revision,
                    deadline_millis: 5_000,
                },
                SECRET_HEX.as_bytes().to_vec(),
            )
            .await
            .expect("import")
            .snapshot;
        let public_key = imported.selected_public_key_hex.expect("selection");
        core.activate_identity(public_key).await.expect("activate");
        core.refresh_active_profile().await.expect("refresh");

        wait_for_fresh_profile(&observer).await;
        let snapshots = observer.snapshots.lock().expect("snapshots").clone();
        assert!(snapshots.iter().any(|snapshot| {
            snapshot.active_identity.as_ref().is_some_and(|active| {
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

    impl HarvestCircleChangeObserver for ArcObserver {
        fn on_change(&self, change: SnapshotChangeDto) {
            self.0.on_change(change);
        }
    }

    const OBSERVER_DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

    async fn wait_for_snapshot_count(observer: &RecordingObserver, minimum: usize) {
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
            while observer.snapshots.lock().expect("snapshots").len() < minimum {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("snapshot delivery");
    }

    async fn wait_for_observer_count(core: &HarvestCircleAppCore, expected: usize) {
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
            while core.inner.observers.lock().expect("observers").len() != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("observer deregistration");
    }

    async fn wait_for_fresh_profile(observer: &RecordingObserver) {
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
            loop {
                let fresh = observer
                    .snapshots
                    .lock()
                    .expect("snapshots")
                    .iter()
                    .any(|snapshot| {
                        snapshot.active_identity.as_ref().is_some_and(|active| {
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
