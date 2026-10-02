use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

use harvestcircle_application::ChangeSubscriptionId;

use crate::commands::RuntimeCore;
use crate::{AppSnapshotDto, HarvestCircleAppCore, HarvestCircleError};

const OBSERVER_CHANGE_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(63);
pub(crate) const MAX_OBSERVERS: usize = 32;

pub(crate) struct ObserverTask {
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    _admission: Arc<tokio::sync::OwnedSemaphorePermit>,
}

struct ObserverResources {
    // Field drop order keeps callback destruction inside its admission reservation.
    observer: Box<dyn HarvestCircleChangeObserver>,
    admission: Arc<tokio::sync::OwnedSemaphorePermit>,
}

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

impl Drop for ObserverSubscription {
    fn drop(&mut self) {
        let Ok(retained_id) = self.id.get_mut() else {
            return;
        };
        let (Some(core), Some(id)) = (self.core.upgrade(), *retained_id) else {
            return;
        };
        let task = {
            let Ok(observers) = core.observers.lock() else {
                return;
            };
            let Ok(retired) = core.retired_observers.lock() else {
                return;
            };
            observers.get(&id).or_else(|| retired.get(&id)).cloned()
        };
        if let Some(task) = task
            && let Ok(mut stop) = task.stop.lock()
        {
            drop(stop.take());
        }
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl ObserverSubscription {
    pub async fn unsubscribe(&self) {
        let id = {
            let Ok(retained_id) = self.id.lock() else {
                return;
            };
            *retained_id
        };
        let (Some(core), Some(id)) = (self.core.upgrade(), id) else {
            return;
        };
        let _close = core.close_gate.lock().await;
        if finish_observers(&core, Some(id)).await.is_err() {
            return;
        }
        let _ = core.actor.unsubscribe_changes(id).await;
        if let Ok(mut retained_id) = self.id.lock() {
            *retained_id = None;
        }
    }
}

async fn finish_observers(
    core: &RuntimeCore,
    selected: Option<ChangeSubscriptionId>,
) -> Result<(), HarvestCircleError> {
    let tasks = {
        let observers = core
            .observers
            .lock()
            .map_err(|_| crate::commands::internal_state_unavailable())?;
        let retired = core
            .retired_observers
            .lock()
            .map_err(|_| crate::commands::internal_state_unavailable())?;
        observers
            .iter()
            .chain(retired.iter())
            .filter(|(id, _)| selected.is_none_or(|selected| selected == **id))
            .map(|(id, task)| (*id, Arc::clone(task)))
            .collect::<Vec<_>>()
    };
    for (_, task) in &tasks {
        if let Some(task) = task.handle.lock().await.as_ref() {
            task.abort();
        }
    }
    for (id, task) in tasks {
        {
            let mut retained = task.handle.lock().await;
            if let Some(task) = retained.as_mut() {
                // The registry retains the exact handle if this awaiting future is cancelled.
                let _ = task.await;
            }
            drop(retained.take());
        }
        // Admission stays reserved until the join, including callback destruction, finishes.
        let mut observers = core
            .observers
            .lock()
            .map_err(|_| crate::commands::internal_state_unavailable())?;
        let mut retired = core
            .retired_observers
            .lock()
            .map_err(|_| crate::commands::internal_state_unavailable())?;
        observers.remove(&id);
        retired.remove(&id);
    }
    Ok(())
}

fn retire_observer(core: &RuntimeCore, id: ChangeSubscriptionId) {
    let Ok(mut observers) = core.observers.lock() else {
        return;
    };
    let Ok(mut retired) = core.retired_observers.lock() else {
        return;
    };
    if let Some(task) = observers.remove(&id) {
        retired.insert(id, task);
    }
}

async fn forward_observer(
    resources: ObserverResources,
    runtime_core: Weak<RuntimeCore>,
    mut subscription: harvestcircle_runtime::RuntimeChangeSubscription,
    mut stopped: tokio::sync::oneshot::Receiver<()>,
) {
    let id = subscription.id();
    loop {
        let change = tokio::select! {
            biased;
            _ = &mut stopped => break,
            change = subscription.receive() => change,
        };
        let Some(change) = change else {
            break;
        };
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
        if catch_unwind(AssertUnwindSafe(|| resources.observer.on_change(delivery))).is_err() {
            break;
        }
    }
    if let Some(runtime_core) = runtime_core.upgrade() {
        let _ = runtime_core.actor.unsubscribe_changes(id).await;
        retire_observer(&runtime_core, id);
    }
    // Consume the whole bundle so async capture cannot separate the callback and reservation.
    drop(resources);
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
        let resources = {
            let _observers = self
                .inner
                .observers
                .lock()
                .map_err(|_| observer_registration_error())?;
            let mut retired = self
                .inner
                .retired_observers
                .lock()
                .map_err(|_| observer_registration_error())?;
            retired.retain(|_, task| match task.handle.try_lock() {
                Ok(retained) => retained.as_ref().is_some_and(|task| !task.is_finished()),
                Err(_) => true,
            });
            if !self.inner.is_open() {
                return Err(closed_error());
            }
            let admission = Arc::clone(&self.inner.observer_admission)
                .try_acquire_owned()
                .map_err(|_| observer_registration_error())?;
            ObserverResources {
                observer,
                admission: Arc::new(admission),
            }
        };
        let subscription = self
            .inner
            .actor
            .subscribe_changes(OBSERVER_CHANGE_CAPACITY)
            .await
            .map_err(HarvestCircleError::from)?;
        let id = subscription.id();
        let runtime_core = Arc::downgrade(&self.inner);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let admitted = {
            let mut observers = self
                .inner
                .observers
                .lock()
                .map_err(|_| observer_registration_error())?;
            let _retired = self
                .inner
                .retired_observers
                .lock()
                .map_err(|_| observer_registration_error())?;
            if !self.inner.is_open() {
                false
            } else {
                let admission = Arc::clone(&resources.admission);
                let task = self.inner.runtime.spawn(forward_observer(
                    resources,
                    runtime_core,
                    subscription,
                    stopped,
                ));
                observers.insert(
                    id,
                    Arc::new(ObserverTask {
                        handle: tokio::sync::Mutex::new(Some(task)),
                        stop: Mutex::new(Some(stop)),
                        _admission: admission,
                    }),
                );
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
        {
            let _observers = self
                .inner
                .observers
                .lock()
                .map_err(|_| crate::commands::internal_state_unavailable())?;
            let _retired = self
                .inner
                .retired_observers
                .lock()
                .map_err(|_| crate::commands::internal_state_unavailable())?;
            let _ =
                self.inner
                    .close_state
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        }
        let _close = self.inner.close_gate.lock().await;
        if self.inner.close_state.load(Ordering::Acquire) == 2 {
            return Ok(ShutdownReceiptDto {
                final_revision: self.inner.actor.snapshot().revision().value(),
                closed: true,
            });
        }
        finish_observers(&self.inner, None).await?;
        self.inner
            .actor
            .close()
            .await
            .map_err(HarvestCircleError::from)?;
        match (
            self.inner.keyring.as_ref(),
            self.inner.host_runtime.as_ref(),
        ) {
            (Some(keyring), Some(runtime)) => {
                let keyring = Arc::clone(keyring);
                runtime
                    .run(async move { keyring.close().await })
                    .await
                    .map_err(|()| closed_error())?
                    .map_err(HarvestCircleError::from)?;
            }
            (Some(_), None) => return Err(closed_error()),
            (None, _) => {}
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
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::Poll;
    use std::time::Duration;

    use harvestcircle_application::{
        DurableRequestId, RelayAccess, RelayConfiguration, RelayEndpoint, RelayUrlPolicy,
    };
    use harvestcircle_domain::SecretKeyInput;
    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;

    use crate::commands::{RuntimeCore, test_actor, test_actor_with_nostr_timeout};
    use crate::{
        AppSnapshotDto, HarvestCircleAppCore, HarvestCircleChangeObserver, ProfileLoadStateDto,
        SnapshotChangeDto,
    };

    const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
    const TEST_RELAY_TIMEOUT: Duration = Duration::from_secs(2);
    #[derive(Default)]
    struct RecordingObserver {
        snapshots: Mutex<Vec<AppSnapshotDto>>,
        core: Mutex<Option<Arc<HarvestCircleAppCore>>>,
    }

    struct PanickingObserver;

    #[derive(Default)]
    struct ObserverDropState {
        finished: AtomicBool,
        released: AtomicBool,
    }

    struct GatedDropObserver {
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        state: Arc<ObserverDropState>,
    }

    impl HarvestCircleChangeObserver for GatedDropObserver {
        fn on_change(&self, _change: SnapshotChangeDto) {}
    }

    impl Drop for GatedDropObserver {
        fn drop(&mut self) {
            if let Some(entered) = self.entered.take() {
                let _ = entered.send(());
            }
            let released = self
                .release
                .get_mut()
                .is_ok_and(|release| release.recv_timeout(OBSERVER_DELIVERY_TIMEOUT).is_ok());
            self.state.released.store(released, Ordering::Release);
            self.state.finished.store(true, Ordering::Release);
        }
    }

    struct GatedObserver {
        changes: Mutex<Vec<SnapshotChangeDto>>,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        delivered: tokio::sync::Notify,
    }

    impl HarvestCircleChangeObserver for Arc<GatedObserver> {
        fn on_change(&self, change: SnapshotChangeDto) {
            self.changes.lock().expect("changes").push(change);
            if let Some(entered) = self.entered.lock().expect("initial callback gate").take() {
                entered.send(()).expect("initial callback entered");
                self.release
                    .lock()
                    .expect("callback release gate")
                    .recv_timeout(OBSERVER_DELIVERY_TIMEOUT)
                    .expect("release initial callback");
            }
            self.delivered.notify_one();
        }
    }

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
        core_with_actor(actor, directory)
    }

    async fn core_with_live_relays(relays: RelayConfiguration) -> Arc<HarvestCircleAppCore> {
        let (actor, directory) = test_actor_with_nostr_timeout(relays, TEST_RELAY_TIMEOUT).await;
        core_with_actor(actor, directory)
    }

    fn core_with_actor(
        actor: harvestcircle_runtime::RuntimeActorHandle,
        directory: Arc<tempfile::TempDir>,
    ) -> Arc<HarvestCircleAppCore> {
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: tokio::runtime::Handle::current(),
                host_runtime: None,
                keyring: None,
                observers: Mutex::new(std::collections::BTreeMap::new()),
                retired_observers: Mutex::new(std::collections::BTreeMap::new()),
                observer_admission: Arc::new(tokio::sync::Semaphore::new(super::MAX_OBSERVERS)),
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
                retired_observers: Mutex::new(std::collections::BTreeMap::new()),
                observer_admission: Arc::new(tokio::sync::Semaphore::new(super::MAX_OBSERVERS)),
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_unsubscribe_retry_waits_for_the_retained_callback_task() {
        let core = core().await;
        let (observer, entered, release) = gated_observer();
        let subscription = core
            .subscribe_changes_v2(Box::new(Arc::clone(&observer)))
            .await
            .expect("subscribe gated callback");
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, entered)
            .await
            .expect("callback entry deadline")
            .expect("callback entered");
        let task = observer_abort_handle(&core, &subscription);
        let first_was_pending = {
            let mut first = Box::pin(subscription.unsubscribe());
            std::future::poll_fn(|context| Poll::Ready(first.as_mut().poll(context)))
                .await
                .is_pending()
        };
        let mut retry = Box::pin(subscription.unsubscribe());
        let retry_was_pending =
            std::future::poll_fn(|context| Poll::Ready(retry.as_mut().poll(context)))
                .await
                .is_pending();
        let finished_before_release = task.is_finished();

        release
            .send(())
            .expect("release callback before assertions");
        if retry_was_pending {
            tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, retry)
                .await
                .expect("unsubscribe retry completion");
        }
        wait_for_aborted_task(&task).await;
        core.shutdown_v2().await.expect("cleanup runtime");

        assert!(
            first_was_pending,
            "first unsubscribe must wait for callback exit"
        );
        assert!(!finished_before_release, "callback was explicitly gated");
        assert!(
            retry_was_pending,
            "retry must retain ownership of callback cleanup"
        );
        assert!(task.is_finished());
        assert_eq!(observer.changes.lock().expect("changes").len(), 1);
        subscription.unsubscribe().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_shutdown_retry_finishes_observers_before_terminal_close() {
        let core = core().await;
        let (observer, entered, release) = gated_observer();
        let subscription = core
            .subscribe_changes_v2(Box::new(Arc::clone(&observer)))
            .await
            .expect("subscribe gated callback");
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, entered)
            .await
            .expect("callback entry deadline")
            .expect("callback entered");
        let task = observer_abort_handle(&core, &subscription);
        let first_was_pending = {
            let mut first = Box::pin(core.shutdown_v2());
            std::future::poll_fn(|context| Poll::Ready(first.as_mut().poll(context)))
                .await
                .is_pending()
        };
        let mut retry = Box::pin(core.shutdown_v2());
        let initial_retry =
            std::future::poll_fn(|context| Poll::Ready(retry.as_mut().poll(context))).await;
        // This command orders actor work after any close submitted by the retry.
        let _ = core.inner.actor.bootstrap().await;
        let retried = if initial_retry.is_pending() {
            std::future::poll_fn(|context| Poll::Ready(retry.as_mut().poll(context))).await
        } else {
            initial_retry
        };
        let closed_before_callback_exit = retried.is_ready();
        let finished_before_release = task.is_finished();

        release
            .send(())
            .expect("release callback before assertions");
        let receipt = match retried {
            Poll::Ready(receipt) => receipt.expect("retry close receipt"),
            Poll::Pending => tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, retry)
                .await
                .expect("resumed shutdown completion")
                .expect("resumed shutdown receipt"),
        };
        wait_for_aborted_task(&task).await;
        subscription.unsubscribe().await;
        assert_eq!(core.shutdown_v2().await.expect("repeated close"), receipt);

        assert!(
            first_was_pending,
            "first shutdown must wait for callback exit"
        );
        assert!(!finished_before_release, "callback was explicitly gated");
        assert!(
            !closed_before_callback_exit,
            "terminal close must retain observer cleanup ownership"
        );
        assert!(receipt.closed);
        assert!(task.is_finished());
        assert!(core.inner.observers.lock().expect("observers").is_empty());
        assert_eq!(observer.changes.lock().expect("changes").len(), 1);
    }

    #[tokio::test]
    async fn abandoned_observer_handles_release_bounded_registration_admission() {
        let core = core().await;
        let observer = Arc::new(RecordingObserver::default());
        for _ in 0..super::MAX_OBSERVERS {
            let subscription = core
                .subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer))))
                .await
                .expect("admit observer before abandonment");
            drop(subscription);
        }
        core.inner
            .actor
            .bootstrap()
            .await
            .expect("actor ordering barrier");
        let cleanup_completed = tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
            while !core.inner.observers.lock().expect("observers").is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();
        let registered_after_abandonment = core.inner.observers.lock().expect("observers").len();
        let replacement = core
            .subscribe_changes_v2(Box::new(ArcObserver(observer)))
            .await;
        let replacement_was_admitted = replacement.is_ok();
        if let Ok(replacement) = replacement {
            replacement.unsubscribe().await;
        }
        core.shutdown_v2()
            .await
            .expect("cleanup abandoned observers");

        assert!(
            cleanup_completed,
            "bounded observer task cleanup must finish after abandonment"
        );
        assert_eq!(
            registered_after_abandonment, 0,
            "abandoned handles must release admission"
        );
        assert!(
            replacement_was_admitted,
            "abandonment must not exhaust the fixed observer limit"
        );
    }

    #[tokio::test]
    async fn cancelled_native_registration_before_and_after_actor_reply_never_delivers() {
        let core = core().await;
        for cancel_after_actor_reply in [false, true] {
            let observer = Arc::new(RecordingObserver::default());
            let mut registration =
                Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
            let first_poll =
                std::future::poll_fn(|context| Poll::Ready(registration.as_mut().poll(context)))
                    .await;
            assert!(
                first_poll.is_pending(),
                "registration queued on the current-thread actor"
            );
            if cancel_after_actor_reply {
                core.inner
                    .actor
                    .bootstrap()
                    .await
                    .expect("actor installed registration before cancellation");
            }
            drop(registration);
            core.inner
                .actor
                .bootstrap()
                .await
                .expect("actor cancellation ordering barrier");
            assert!(core.inner.observers.lock().expect("observers").is_empty());
            assert!(observer.snapshots.lock().expect("snapshots").is_empty());
        }
        core.shutdown_v2().await.expect("cleanup runtime");
    }

    #[tokio::test]
    async fn pending_native_registrations_share_the_fixed_observer_admission_limit() {
        let core = core().await;
        let observer = Arc::new(RecordingObserver::default());
        let mut pending = Vec::with_capacity(super::MAX_OBSERVERS);
        for _ in 0..super::MAX_OBSERVERS {
            let mut registration =
                Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
            let first_poll =
                std::future::poll_fn(|context| Poll::Ready(registration.as_mut().poll(context)))
                    .await;
            assert!(
                first_poll.is_pending(),
                "admitted registration awaits the actor"
            );
            core.inner
                .actor
                .bootstrap()
                .await
                .expect("actor installed the unpolled reply");
            pending.push(registration);
        }
        assert!(core.inner.observers.lock().expect("observers").is_empty());
        let mut excess =
            Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
        let refused =
            std::future::poll_fn(|context| Poll::Ready(excess.as_mut().poll(context))).await;
        let refused_before_actor_allocation = matches!(
            refused,
            Poll::Ready(Err(crate::HarvestCircleError::Failure {
                code: crate::WireErrorCode::ObserverRegistrationFailed,
                ..
            }))
        );
        drop(excess);
        drop(pending);
        core.inner
            .actor
            .bootstrap()
            .await
            .expect("cancelled reply cleanup barrier");
        let replacement = core
            .subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer))))
            .await
            .expect("replacement after pending cancellation");
        replacement.unsubscribe().await;
        core.shutdown_v2().await.expect("cleanup runtime");

        assert!(
            refused_before_actor_allocation,
            "pending replies must consume the same fixed observer slots"
        );
        assert_eq!(
            core.inner.observer_admission.available_permits(),
            super::MAX_OBSERVERS
        );
    }

    #[tokio::test]
    async fn cancelled_pending_registration_retains_admission_until_callback_destruction_finishes()
    {
        let core = core().await;
        let state = Arc::new(ObserverDropState::default());
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let pending_core = Arc::clone(&core);
        let drop_state = Arc::clone(&state);
        let mut gated_registration = Box::pin(async move {
            pending_core
                .subscribe_changes_v2(Box::new(GatedDropObserver {
                    entered: Some(entered_sender),
                    release: Mutex::new(release_receiver),
                    state: drop_state,
                }))
                .await
        });
        let first_poll =
            std::future::poll_fn(|context| Poll::Ready(gated_registration.as_mut().poll(context)))
                .await;
        assert!(
            first_poll.is_pending(),
            "gated registration awaits the actor"
        );
        core.inner
            .actor
            .bootstrap()
            .await
            .expect("gated actor reply ready but unpolled");

        let observer = Arc::new(RecordingObserver::default());
        let mut pending = Vec::with_capacity(super::MAX_OBSERVERS - 1);
        for _ in 1..super::MAX_OBSERVERS {
            let mut registration =
                Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
            let first_poll =
                std::future::poll_fn(|context| Poll::Ready(registration.as_mut().poll(context)))
                    .await;
            assert!(
                first_poll.is_pending(),
                "peer registration awaits the actor"
            );
            core.inner
                .actor
                .bootstrap()
                .await
                .expect("peer actor reply ready but unpolled");
            pending.push(registration);
        }
        assert_eq!(core.inner.observer_admission.available_permits(), 0);

        let dropping = std::thread::spawn(move || drop(gated_registration));
        let entered = tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, entered_receiver)
            .await
            .is_ok_and(|entered| entered.is_ok());
        let destructor_was_gated = !state.finished.load(Ordering::Acquire);
        let mut replacement =
            Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
        let replacement_poll =
            std::future::poll_fn(|context| Poll::Ready(replacement.as_mut().poll(context))).await;
        let refused_during_destruction = matches!(
            replacement_poll,
            Poll::Ready(Err(crate::HarvestCircleError::Failure {
                code: crate::WireErrorCode::ObserverRegistrationFailed,
                ..
            }))
        );

        let released = release_sender.send(()).is_ok();
        let joined = dropping.join().is_ok();
        drop(replacement);
        drop(pending);
        core.inner
            .actor
            .bootstrap()
            .await
            .expect("cancelled registration cleanup barrier");
        let resumed = core
            .subscribe_changes_v2(Box::new(ArcObserver(observer)))
            .await
            .expect("registration after callback destruction");
        resumed.unsubscribe().await;
        core.shutdown_v2()
            .await
            .expect("cleanup runtime before assertions");

        assert!(entered, "callback destructor entry was observed");
        assert!(
            destructor_was_gated,
            "replacement was probed during callback destruction"
        );
        assert!(
            released && joined,
            "the destructor gate was released and its owned thread joined"
        );
        assert!(state.released.load(Ordering::Acquire));
        assert!(state.finished.load(Ordering::Acquire));
        assert!(
            refused_during_destruction,
            "callback destruction must retain its pending admission slot"
        );
        assert_eq!(
            core.inner.observer_admission.available_permits(),
            super::MAX_OBSERVERS
        );
    }

    #[tokio::test]
    async fn ready_native_registration_reply_cannot_install_after_terminal_close() {
        let core = core().await;
        let observer = Arc::new(RecordingObserver::default());
        let mut registration =
            Box::pin(core.subscribe_changes_v2(Box::new(ArcObserver(Arc::clone(&observer)))));
        let first_poll =
            std::future::poll_fn(|context| Poll::Ready(registration.as_mut().poll(context))).await;
        assert!(
            first_poll.is_pending(),
            "registration queued on the current-thread actor"
        );
        core.inner
            .actor
            .bootstrap()
            .await
            .expect("actor reply ready before close");

        let closed = core
            .shutdown_v2()
            .await
            .expect("close before host registration resumes");
        assert!(
            registration.await.is_err(),
            "closed registration must not install a callback task"
        );

        assert!(closed.closed);
        assert!(observer.snapshots.lock().expect("snapshots").is_empty());
        assert!(core.inner.observers.lock().expect("observers").is_empty());
        assert!(
            core.inner
                .retired_observers
                .lock()
                .expect("retired observers")
                .is_empty()
        );
        assert_eq!(
            core.inner.observer_admission.available_permits(),
            super::MAX_OBSERVERS
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_native_callback_queue_terminates_before_shutdown_returns() {
        let core = core().await;
        let mut public_keys = Vec::new();
        for request_id in [
            "01890f3e-7b1c-7000-8000-000000000052",
            "01890f3e-7b1c-7000-8000-000000000053",
        ] {
            let imported = core
                .inner
                .actor
                .import_secret_key(
                    DurableRequestId::parse(request_id).expect("test import request"),
                    core.inner.actor.snapshot().revision(),
                    SecretKeyInput::parse(Keys::generate().secret_key().to_secret_hex())
                        .expect("ephemeral identity input"),
                    OBSERVER_DELIVERY_TIMEOUT,
                )
                .await
                .expect("isolated memory-store import");
            public_keys.push(imported.identity().public_key());
        }
        core.inner
            .actor
            .select_identity(public_keys[1])
            .await
            .expect("initial selection");
        let initial_revision = core.snapshot().revision;
        let (observer, entered, release) = gated_observer();
        let subscription = core
            .subscribe_changes_v2(Box::new(Arc::clone(&observer)))
            .await
            .expect("gated observer registration");
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, entered)
            .await
            .expect("callback entry deadline")
            .expect("callback entered");
        let task = observer_abort_handle(&core, &subscription);
        let publications = super::OBSERVER_CHANGE_CAPACITY.get() + 2;
        for offset in 0..publications {
            core.inner
                .actor
                .select_identity(public_keys[offset % 2])
                .await
                .expect("revision-changing selection");
        }
        let final_revision = core.snapshot().revision;
        let mut closing = Box::pin(core.shutdown_v2());
        let first_close_poll =
            std::future::poll_fn(|context| Poll::Ready(closing.as_mut().poll(context))).await;
        let closing_was_pending = first_close_poll.is_pending();
        release
            .send(())
            .expect("release saturated callback before assertions");
        let receipt = match first_close_poll {
            Poll::Ready(receipt) => receipt.expect("shutdown receipt"),
            Poll::Pending => tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, closing)
                .await
                .expect("bounded full-queue close")
                .expect("shutdown receipt"),
        };
        let callback_count_at_close = observer.changes.lock().expect("changes").len();
        subscription.unsubscribe().await;
        subscription.unsubscribe().await;
        let repeated = core.shutdown_v2().await.expect("repeated close");
        let refused = core
            .subscribe_changes_v2(Box::new(PanickingObserver))
            .await
            .is_err();

        assert!(
            closing_was_pending,
            "close must wait for the running callback"
        );
        assert_eq!(
            final_revision,
            initial_revision + u64::try_from(publications).expect("publication count")
        );
        assert_eq!(receipt.final_revision, final_revision);
        assert!(receipt.closed);
        assert_eq!(repeated, receipt);
        assert!(task.is_finished());
        assert!(refused);
        assert!(core.inner.observers.lock().expect("observers").is_empty());
        assert_eq!(
            observer.changes.lock().expect("changes").len(),
            callback_count_at_close
        );
    }

    fn gated_observer() -> (
        Arc<GatedObserver>,
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        (
            Arc::new(GatedObserver {
                changes: Mutex::new(Vec::new()),
                entered: Mutex::new(Some(entered_sender)),
                release: Mutex::new(release_receiver),
                delivered: tokio::sync::Notify::new(),
            }),
            entered_receiver,
            release_sender,
        )
    }

    fn observer_abort_handle(
        core: &HarvestCircleAppCore,
        subscription: &crate::ObserverSubscription,
    ) -> tokio::task::AbortHandle {
        let id = subscription.id.lock().expect("id").expect("registered id");
        let task = core
            .inner
            .observers
            .lock()
            .expect("observers")
            .get(&id)
            .expect("registered observer")
            .clone();
        let retained = task.handle.try_lock().expect("observer join not started");
        retained.as_ref().expect("observer task").abort_handle()
    }

    async fn wait_for_aborted_task(task: &tokio::task::AbortHandle) {
        tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
            while !task.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("released callback task finishes");
    }

    #[test]
    fn slow_callback_recovers_final_tail_after_actor_queue_saturation() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("two-worker callback test runtime");
        runtime.block_on(async {
            let core = core().await;
            let mut public_keys = Vec::with_capacity(2);
            for request_id in [
                "01890f3e-7b1c-7000-8000-000000000050",
                "01890f3e-7b1c-7000-8000-000000000051",
            ] {
                let imported = core
                    .inner
                    .actor
                    .import_secret_key(
                        DurableRequestId::parse(request_id).expect("test import request"),
                        core.inner.actor.snapshot().revision(),
                        SecretKeyInput::parse(Keys::generate().secret_key().to_secret_hex())
                            .expect("ephemeral identity input"),
                        OBSERVER_DELIVERY_TIMEOUT,
                    )
                    .await
                    .expect("import into isolated memory secret store");
                public_keys.push(imported.identity().public_key());
            }
            assert_ne!(public_keys[0], public_keys[1]);
            core.inner
                .actor
                .select_identity(public_keys[1])
                .await
                .expect("select second identity before observation");
            let initial_revision = core.snapshot().revision;
            let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
            let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
            let observer = Arc::new(GatedObserver {
                changes: Mutex::new(Vec::new()),
                entered: Mutex::new(Some(entered_sender)),
                release: Mutex::new(release_receiver),
                delivered: tokio::sync::Notify::new(),
            });
            let subscription = core
                .subscribe_changes_v2(Box::new(Arc::clone(&observer)))
                .await
                .expect("subscribe slow callback");
            tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, entered_receiver)
                .await
                .expect("initial callback deadline")
                .expect("initial callback entered");

            let queued_changes = super::OBSERVER_CHANGE_CAPACITY.get();
            let publications = queued_changes + 2;
            let final_revision = tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
                let mut final_revision = initial_revision;
                for offset in 0..publications {
                    final_revision = core
                        .inner
                        .actor
                        .select_identity(public_keys[offset % public_keys.len()])
                        .await
                        .expect("alternate selected identity")
                        .revision()
                        .value();
                }
                final_revision
            })
            .await
            .expect("bounded actor publications");
            assert_eq!(
                final_revision,
                initial_revision + u64::try_from(publications).expect("publication count")
            );
            assert_eq!(observer.changes.lock().expect("changes").len(), 1);
            release_sender.send(()).expect("release slow callback");

            tokio::time::timeout(OBSERVER_DELIVERY_TIMEOUT, async {
                loop {
                    let delivered = observer.delivered.notified();
                    if observer
                        .changes
                        .lock()
                        .expect("changes")
                        .last()
                        .is_some_and(|change| change.snapshot.revision == final_revision)
                    {
                        break;
                    }
                    delivered.await;
                }
            })
            .await
            .expect("final callback without a later publication");

            let changes = observer.changes.lock().expect("changes").clone();
            let mut expected_revisions = vec![initial_revision];
            expected_revisions.extend(
                (1..=queued_changes)
                    .map(|offset| initial_revision + u64::try_from(offset).expect("queue offset")),
            );
            expected_revisions.push(final_revision);
            assert_eq!(
                changes
                    .iter()
                    .map(|change| change.snapshot.revision)
                    .collect::<Vec<_>>(),
                expected_revisions
            );
            assert_eq!(changes[0].previous_revision, None);
            for change in &changes[1..] {
                assert_eq!(change.previous_revision, Some(change.snapshot.revision - 1));
            }
            subscription.unsubscribe().await;
            core.shutdown_v2().await.expect("shutdown");
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
            let task = core
                .inner
                .observers
                .lock()
                .expect("observers")
                .get(&id)
                .expect("registered observer")
                .clone();
            task.handle
                .lock()
                .await
                .as_ref()
                .expect("observer task")
                .abort();

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

        let core = core_with_live_relays(
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
