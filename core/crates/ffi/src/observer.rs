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
    pub fn unsubscribe(&self) {
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
        self.unsubscribe();
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

    pub fn shutdown(&self) {
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
        safe_message: "The application runtime is closed.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_studio_application::RelayConfiguration;
    use radroots_studio_domain::RelayUrl;
    use radroots_studio_storage::PersistentAppCore;

    use crate::commands::{RuntimeCore, SystemClock};
    use crate::{AppSnapshotDto, ProfileLoadStateDto, StudioAppCore, StudioObserver};

    const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

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
        core_with_relays(RelayConfiguration::default())
    }

    fn core_with_relays(relays: RelayConfiguration) -> Arc<StudioAppCore> {
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                adapter: PersistentAppCore::in_memory(relays).expect("core"),
                secrets: Arc::new(radroots_studio_application::InMemorySecretStore::default()),
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
        subscription.unsubscribe();
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

        core.shutdown();
        core.shutdown();

        assert!(core.subscribe(Box::new(ArcObserver(observer))).is_err());
        assert!(core.inner.observers.lock().expect("observers").is_empty());
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

        let core = core_with_relays(RelayConfiguration::new(vec![
            RelayUrl::parse(relay_url.as_str()).expect("relay URL"),
        ]));
        core.bootstrap().await.expect("bootstrap");
        let observer = Arc::new(RecordingObserver::default());
        *observer.core.lock().expect("core") = Some(Arc::clone(&core));
        let subscription = core
            .subscribe(Box::new(ArcObserver(observer.clone())))
            .expect("subscribe");
        let imported = core
            .import_secret_key(SECRET_HEX.to_owned())
            .await
            .expect("import");
        let public_key = imported.selected_public_key_hex.expect("selection");
        core.activate_account(public_key).await.expect("activate");
        core.refresh_active_profile().await.expect("refresh");

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
        subscription.unsubscribe();
        let count = observer.snapshots.lock().expect("snapshots").len();
        core.sign_out().await.expect("sign out");
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), count);

        core.shutdown();
        publisher.shutdown().await;
        local_relay.shutdown();
    }

    struct ArcObserver(Arc<RecordingObserver>);

    impl StudioObserver for ArcObserver {
        fn on_snapshot_changed(&self, snapshot: AppSnapshotDto) {
            self.0.on_snapshot_changed(snapshot);
        }
    }
}
