use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use harvestcircle_application::{BoxFuture, SecretStore};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use tokio::sync::{oneshot, watch};

const KEYRING_QUEUE_CAPACITY: usize = 8;

enum Request {
    Put(
        PublicKey,
        SecretKeyInput,
        oneshot::Sender<Result<(), SafeError>>,
    ),
    Load(
        PublicKey,
        oneshot::Sender<Result<SecretKeyInput, SafeError>>,
    ),
    Contains(PublicKey, oneshot::Sender<Result<bool, SafeError>>),
    Delete(PublicKey, oneshot::Sender<Result<(), SafeError>>),
    Close,
}

pub(crate) struct BoundedKeyringWorker {
    sender: Mutex<Option<std::sync::mpsc::SyncSender<Request>>>,
    completion: watch::Receiver<bool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl BoundedKeyringWorker {
    pub(crate) fn new(store: impl SecretStore + 'static) -> Result<Arc<Self>, SafeError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(KEYRING_QUEUE_CAPACITY);
        let (completion_sender, completion_receiver) = watch::channel(false);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| worker_unavailable())?;
        let thread = std::thread::Builder::new()
            .name("harvestcircle-keyring-worker".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Put(public_key, secret, response) => {
                            let _ = response.send(
                                runtime.block_on(async { store.put(public_key, secret).await }),
                            );
                        }
                        Request::Load(public_key, response) => {
                            let _ = response
                                .send(runtime.block_on(async { store.load(public_key).await }));
                        }
                        Request::Contains(public_key, response) => {
                            let _ = response
                                .send(runtime.block_on(async { store.contains(public_key).await }));
                        }
                        Request::Delete(public_key, response) => {
                            let _ = response
                                .send(runtime.block_on(async { store.delete(public_key).await }));
                        }
                        Request::Close => break,
                    }
                }
                let _ = completion_sender.send(true);
            })
            .map_err(|_| worker_unavailable())?;
        Ok(Arc::new(Self {
            sender: Mutex::new(Some(sender)),
            completion: completion_receiver,
            thread: Mutex::new(Some(thread)),
        }))
    }

    async fn submit<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<T>) -> Request,
    ) -> Result<T, SafeError> {
        let (response_sender, response_receiver) = oneshot::channel();
        {
            let sender_guard = self.sender.lock().map_err(|_| worker_unavailable())?;
            let Some(sender) = sender_guard.as_ref() else {
                return Err(worker_unavailable());
            };
            sender
                .try_send(request(response_sender))
                .map_err(|_| worker_unavailable())?;
        }
        response_receiver.await.map_err(|_| worker_unavailable())
    }

    pub(crate) async fn close(&self) -> Result<(), SafeError> {
        let sender = self.sender.lock().map_err(|_| worker_unavailable())?.take();
        if let Some(sender) = sender {
            signal_close(sender);
        }
        let mut completion = self.completion.clone();
        while !*completion.borrow() {
            completion
                .changed()
                .await
                .map_err(|_| worker_unavailable())?;
        }
        let thread = self.thread.lock().map_err(|_| worker_unavailable())?.take();
        if let Some(thread) = thread {
            thread.join().map_err(|_| worker_unavailable())?;
        }
        Ok(())
    }
}

fn signal_close(sender: std::sync::mpsc::SyncSender<Request>) {
    match sender.try_send(Request::Close) {
        Ok(())
        | Err(std::sync::mpsc::TrySendError::Full(Request::Close))
        | Err(std::sync::mpsc::TrySendError::Disconnected(Request::Close)) => {}
        Err(
            std::sync::mpsc::TrySendError::Full(_) | std::sync::mpsc::TrySendError::Disconnected(_),
        ) => {
            unreachable!("close signaling constructs only close requests")
        }
    }
}

impl SecretStore for BoundedKeyringWorker {
    fn put(
        &self,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.submit(|response| Request::Put(public_key, secret, response))
                .await?
        })
    }

    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
        Box::pin(async move {
            self.submit(|response| Request::Load(public_key, response))
                .await?
        })
    }

    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
        Box::pin(async move {
            self.submit(|response| Request::Contains(public_key, response))
                .await?
        })
    }

    fn delete(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.submit(|response| Request::Delete(public_key, response))
                .await?
        })
    }
}

impl Drop for BoundedKeyringWorker {
    fn drop(&mut self) {
        if let Ok(sender) = self.sender.get_mut()
            && let Some(sender) = sender.take()
        {
            let _ = sender.try_send(Request::Close);
        }
    }
}

const fn worker_unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::KeyringUnavailable,
        SafeMessage::new("The operating system credential store is unavailable."),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use harvestcircle_application::{BoxFuture, InMemorySecretStore, SecretStore};
    use harvestcircle_domain::{PublicKey, SafeError, SecretKeyInput};
    use tokio::sync::oneshot;

    use super::{BoundedKeyringWorker, Request, signal_close};

    fn public_key() -> PublicKey {
        PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
            .expect("public key")
    }

    struct SlowContainsStore {
        inner: InMemorySecretStore,
    }

    impl SecretStore for SlowContainsStore {
        fn put(
            &self,
            public_key: PublicKey,
            secret: SecretKeyInput,
        ) -> BoxFuture<'_, Result<(), SafeError>> {
            self.inner.put(public_key, secret)
        }

        fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
            self.inner.load(public_key)
        }

        fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
            Box::pin(async move {
                std::thread::sleep(Duration::from_millis(50));
                self.inner.contains(public_key).await
            })
        }

        fn delete(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
            self.inner.delete(public_key)
        }
    }

    #[tokio::test]
    async fn worker_round_trips_without_exposing_secret_material() {
        let worker = BoundedKeyringWorker::new(InMemorySecretStore::default()).expect("worker");
        let secret = SecretKeyInput::parse(
            "0000000000000000000000000000000000000000000000000000000000000001".to_owned(),
        )
        .expect("secret");
        worker.put(public_key(), secret).await.expect("put");
        assert!(worker.contains(public_key()).await.expect("contains"));
        let loaded = worker.load(public_key()).await.expect("load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        worker.delete(public_key()).await.expect("delete");
        worker.close().await.expect("close");
        assert!(worker.contains(public_key()).await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_waiting_never_blocks_the_tokio_runtime_thread() {
        let worker = BoundedKeyringWorker::new(SlowContainsStore {
            inner: InMemorySecretStore::default(),
        })
        .expect("worker");
        let response = worker.contains(public_key());
        tokio::pin!(response);

        tokio::select! {
            biased;
            result = &mut response => panic!("slow keyring response completed before runtime progress: {result:?}"),
            () = tokio::task::yield_now() => {}
        }

        assert!(!response.await.expect("contains"));
        worker.close().await.expect("close");
    }

    #[test]
    fn close_signal_never_blocks_on_a_full_bounded_queue() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let (response, _response_receiver) = oneshot::channel();
        assert!(
            sender
                .try_send(Request::Contains(public_key(), response))
                .is_ok()
        );

        signal_close(sender);

        assert!(matches!(receiver.recv(), Ok(Request::Contains(_, _))));
        assert!(receiver.recv().is_err());
    }
}
