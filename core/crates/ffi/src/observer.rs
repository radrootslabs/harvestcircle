use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

use radroots_studio_application::ChangeSubscriptionId;

use crate::commands::RuntimeCore;
use crate::{AppSnapshotDto, StudioAppCore, StudioError};

const OBSERVER_CHANGE_CAPACITY: NonZeroUsize = match NonZeroUsize::new(64) {
    Some(capacity) => capacity,
    None => unreachable!(),
};

#[uniffi::export(callback_interface)]
pub trait StudioObserver: Send + Sync {
    fn on_snapshot_changed(&self, snapshot: AppSnapshotDto);
}

#[derive(uniffi::Object)]
pub struct ObserverSubscription {
    core: Weak<RuntimeCore>,
    id: Mutex<Option<ChangeSubscriptionId>>,
}

#[uniffi::export]
impl ObserverSubscription {
    pub fn unsubscribe(&self) {
        let id = self
            .id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let (Some(core), Some(id)) = (self.core.upgrade(), id) else {
            return;
        };
        if let Some(task) = core
            .observers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id)
        {
            task.abort();
        }
        let actor = core.actor.clone();
        crate::commands::runtime().spawn(async move {
            let _ = actor.unsubscribe_changes(id).await;
        });
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
    pub async fn subscribe(
        &self,
        observer: Box<dyn StudioObserver>,
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
        let observer: Arc<dyn StudioObserver> = Arc::from(observer);
        let task = crate::commands::runtime().spawn(async move {
            while let Some(change) = subscription.receive().await {
                observer.on_snapshot_changed(change.snapshot().into());
            }
        });
        self.inner
            .observers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, task);
        Ok(Arc::new(ObserverSubscription {
            core: Arc::downgrade(&self.inner),
            id: Mutex::new(Some(id)),
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
        for (_, task) in handles {
            task.abort();
        }
        let actor = self.inner.actor.clone();
        crate::commands::runtime().spawn(async move {
            let _ = actor.close().await;
        });
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
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nostr::{EventBuilder, Keys, Metadata};
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::Client;
    use radroots_studio_application::{InMemorySecretStore, RelayConfiguration, SdkNostrClient};
    use radroots_studio_domain::RelayUrl;
    use radroots_studio_storage::RuntimeActorHandle;

    use crate::commands::{ACTOR_MAILBOX_CAPACITY, RuntimeCore, SystemClock, runtime};
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
        let actor = RuntimeActorHandle::in_memory(
            relays,
            Arc::new(InMemorySecretStore::default()),
            Arc::new(SystemClock),
            Arc::new(SdkNostrClient::new(std::time::Duration::from_millis(10))),
            NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).expect("capacity"),
            runtime().handle(),
        )
        .expect("actor");
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: Mutex::new(std::collections::BTreeMap::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }

    #[test]
    fn callbacks_allow_reentry_and_stop_after_subscription_close() {
        runtime().block_on(async {
            let core = core();
            let observer = Arc::new(RecordingObserver::default());
            *observer.core.lock().expect("core") = Some(Arc::clone(&core));
            let subscription = core
                .subscribe(Box::new(ArcObserver(observer.clone())))
                .await
                .expect("subscribe");

            wait_for_snapshot_count(&observer, 1).await;
            core.inner
                .actor
                .bootstrap()
                .await
                .expect("idempotent bootstrap");
            assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
            subscription.unsubscribe();
            core.inner.actor.sign_out().await.expect("sign out");
            assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
        });
    }

    #[test]
    fn core_close_deregisters_all_observers_and_rejects_new_subscriptions() {
        let core = core();
        let observer = Arc::new(RecordingObserver::default());
        let _subscription = runtime()
            .block_on(core.subscribe(Box::new(ArcObserver(observer.clone()))))
            .expect("subscribe");

        core.shutdown();
        core.shutdown();

        assert!(
            runtime()
                .block_on(core.subscribe(Box::new(ArcObserver(observer))))
                .is_err()
        );
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
            .await
            .expect("subscribe");
        let imported = core
            .import_secret_key(SECRET_HEX.to_owned())
            .await
            .expect("import");
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
