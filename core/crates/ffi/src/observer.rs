use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

use radroots_studio_application::{AppObserver, AppSnapshot, ObserverHandle};

use crate::commands::RuntimeCore;
use crate::{AppSnapshotDto, StudioAppCore, StudioError};

#[uniffi::export(callback_interface)]
pub trait StudioObserver: Send + Sync {
    fn on_snapshot_changed(&self, snapshot: AppSnapshotDto);
}

struct ObserverBridge {
    observer: Arc<dyn StudioObserver>,
}

impl AppObserver for ObserverBridge {
    fn on_snapshot_changed(&self, snapshot: AppSnapshot) {
        self.observer.on_snapshot_changed((&snapshot).into());
    }
}

#[derive(uniffi::Object)]
pub struct ObserverSubscription {
    core: Weak<RuntimeCore>,
    handle: Mutex<Option<ObserverHandle>>,
}

#[uniffi::export]
impl ObserverSubscription {
    pub fn close(&self) {
        let handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let (Some(core), Some(handle)) = (self.core.upgrade(), handle) else {
            return;
        };
        let _ = core.adapter.core().unsubscribe(handle);
        core.observers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&handle);
    }
}

impl Drop for ObserverSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

#[uniffi::export]
impl StudioAppCore {
    /// Subscribes to revisioned snapshots and immediately delivers the current value.
    ///
    /// # Errors
    ///
    /// Returns a safe observer or lifecycle error.
    pub fn subscribe(
        &self,
        observer: Box<dyn StudioObserver>,
    ) -> Result<Arc<ObserverSubscription>, StudioError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(closed_error());
        }
        let bridge = Arc::new(ObserverBridge {
            observer: Arc::from(observer),
        });
        let handle = self
            .inner
            .adapter
            .core()
            .subscribe(bridge)
            .map_err(StudioError::from)?;
        self.inner
            .observers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(handle);
        Ok(Arc::new(ObserverSubscription {
            core: Arc::downgrade(&self.inner),
            handle: Mutex::new(Some(handle)),
        }))
    }

    pub fn close(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let handles = std::mem::take(
            &mut *self
                .inner
                .observers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for handle in handles {
            let _ = self.inner.adapter.core().unsubscribe(handle);
        }
        let _ = self.inner.adapter.sign_out();
    }
}

fn closed_error() -> StudioError {
    StudioError::Failure {
        code: "InvalidApplicationState".to_owned(),
        message: "The application runtime is closed.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use radroots_studio_application::RelayConfiguration;
    use radroots_studio_storage::PersistentAppCore;

    use crate::commands::{RuntimeCore, SystemClock};
    use crate::{AppSnapshotDto, StudioAppCore, StudioObserver};

    #[derive(Default)]
    struct RecordingObserver {
        snapshots: Mutex<Vec<AppSnapshotDto>>,
        core: Mutex<Option<Arc<StudioAppCore>>>,
    }

    impl StudioObserver for RecordingObserver {
        fn on_snapshot_changed(&self, snapshot: AppSnapshotDto) {
            if let Some(core) = self.core.lock().expect("core").as_ref() {
                assert_eq!(core.snapshot().revision, snapshot.revision);
            }
            self.snapshots.lock().expect("snapshots").push(snapshot);
        }
    }

    fn core() -> Arc<StudioAppCore> {
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                adapter: PersistentAppCore::in_memory(RelayConfiguration::default()).expect("core"),
                secrets: radroots_studio_storage::OsKeyringSecretStore,
                clock: SystemClock,
                nostr: radroots_studio_application::SdkNostrClient::new(
                    std::time::Duration::from_millis(10),
                ),
                observers: Mutex::new(std::collections::BTreeSet::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }

    #[test]
    fn callbacks_allow_reentry_and_stop_after_subscription_close() {
        let core = core();
        core.inner.adapter.core().bootstrap().expect("bootstrap");
        let observer = Arc::new(RecordingObserver::default());
        *observer.core.lock().expect("core") = Some(Arc::clone(&core));
        let subscription = core
            .subscribe(Box::new(ArcObserver(observer.clone())))
            .expect("subscribe");

        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
        core.inner
            .adapter
            .core()
            .bootstrap()
            .expect("idempotent bootstrap");
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
        subscription.close();
        core.inner.adapter.core().sign_out().expect("sign out");
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
    }

    #[test]
    fn core_close_deregisters_all_observers_and_rejects_new_subscriptions() {
        let core = core();
        core.inner.adapter.core().bootstrap().expect("bootstrap");
        let observer = Arc::new(RecordingObserver::default());
        let _subscription = core
            .subscribe(Box::new(ArcObserver(observer.clone())))
            .expect("subscribe");

        core.close();
        core.close();

        assert!(core.subscribe(Box::new(ArcObserver(observer))).is_err());
        assert!(core.inner.observers.lock().expect("observers").is_empty());
    }

    struct ArcObserver(Arc<RecordingObserver>);

    impl StudioObserver for ArcObserver {
        fn on_snapshot_changed(&self, snapshot: AppSnapshotDto) {
            self.0.on_snapshot_changed(snapshot);
        }
    }
}
