use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use harvestcircle_application::SecretStore;
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use tokio::sync::watch;

const KEYRING_QUEUE_CAPACITY: usize = 8;

enum Request {
    Put(
        PublicKey,
        SecretKeyInput,
        std::sync::mpsc::SyncSender<Result<(), SafeError>>,
    ),
    Load(
        PublicKey,
        std::sync::mpsc::SyncSender<Result<SecretKeyInput, SafeError>>,
    ),
    Contains(
        PublicKey,
        std::sync::mpsc::SyncSender<Result<bool, SafeError>>,
    ),
    Delete(
        PublicKey,
        std::sync::mpsc::SyncSender<Result<(), SafeError>>,
    ),
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
        let thread = std::thread::Builder::new()
            .name("harvestcircle-keyring-worker".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Put(public_key, secret, response) => {
                            let _ = response.send(store.put(public_key, secret));
                        }
                        Request::Load(public_key, response) => {
                            let _ = response.send(store.load(public_key));
                        }
                        Request::Contains(public_key, response) => {
                            let _ = response.send(store.contains(public_key));
                        }
                        Request::Delete(public_key, response) => {
                            let _ = response.send(store.delete(public_key));
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

    fn submit<T>(
        &self,
        request: impl FnOnce(std::sync::mpsc::SyncSender<T>) -> Request,
    ) -> Result<T, SafeError> {
        let (response_sender, response_receiver) = std::sync::mpsc::sync_channel(1);
        let sender_guard = self.sender.lock().map_err(|_| worker_unavailable())?;
        let Some(sender) = sender_guard.as_ref() else {
            return Err(worker_unavailable());
        };
        sender
            .try_send(request(response_sender))
            .map_err(|_| worker_unavailable())?;
        drop(sender_guard);
        response_receiver.recv().map_err(|_| worker_unavailable())
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
    fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError> {
        self.submit(|response| Request::Put(public_key, secret, response))?
    }

    fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError> {
        self.submit(|response| Request::Load(public_key, response))?
    }

    fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError> {
        self.submit(|response| Request::Contains(public_key, response))?
    }

    fn delete(&self, public_key: PublicKey) -> Result<(), SafeError> {
        self.submit(|response| Request::Delete(public_key, response))?
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
    use harvestcircle_application::{InMemorySecretStore, SecretStore};
    use harvestcircle_domain::{PublicKey, SecretKeyInput};

    use super::{BoundedKeyringWorker, Request, signal_close};

    fn public_key() -> PublicKey {
        PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
            .expect("public key")
    }

    #[tokio::test]
    async fn worker_round_trips_without_exposing_secret_material() {
        let worker = BoundedKeyringWorker::new(InMemorySecretStore::default()).expect("worker");
        let secret = SecretKeyInput::parse(
            "0000000000000000000000000000000000000000000000000000000000000001".to_owned(),
        )
        .expect("secret");
        worker.put(public_key(), secret).expect("put");
        assert!(worker.contains(public_key()).expect("contains"));
        let loaded = worker.load(public_key()).expect("load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        worker.delete(public_key()).expect("delete");
        worker.close().await.expect("close");
        assert!(worker.contains(public_key()).is_err());
    }

    #[test]
    fn close_signal_never_blocks_on_a_full_bounded_queue() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let (response, _response_receiver) = std::sync::mpsc::sync_channel(1);
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
