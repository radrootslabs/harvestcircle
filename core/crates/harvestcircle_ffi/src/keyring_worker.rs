use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use harvestcircle_application::{BoxFuture, DurableRequestId, SecretStore};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use tokio::sync::{oneshot, watch};

const KEYRING_QUEUE_CAPACITY: usize = 8;
const KEYRING_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);
const OPERATION_QUEUED: u8 = 0;
const OPERATION_STARTED: u8 = 1;
const OPERATION_COMPLETED: u8 = 2;
const OPERATION_CANCELLED: u8 = 3;

enum Request {
    Put(
        DurableRequestId,
        PublicKey,
        SecretKeyInput,
        Arc<AtomicU8>,
        oneshot::Sender<Result<(), SafeError>>,
    ),
    Load(
        PublicKey,
        oneshot::Sender<Result<SecretKeyInput, SafeError>>,
    ),
    Contains(
        PublicKey,
        Arc<AtomicU8>,
        oneshot::Sender<Result<bool, SafeError>>,
    ),
    Delete(
        DurableRequestId,
        PublicKey,
        Arc<AtomicU8>,
        oneshot::Sender<Result<(), SafeError>>,
    ),
    Close,
}

struct CancellationGuard {
    phase: Arc<AtomicU8>,
    armed: bool,
}

impl CancellationGuard {
    fn new(phase: Arc<AtomicU8>) -> Self {
        Self { phase, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.phase.compare_exchange(
                OPERATION_QUEUED,
                OPERATION_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
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
                        Request::Put(request_id, public_key, secret, phase, response) => {
                            if start_operation(&phase) {
                                let result = runtime.block_on(async {
                                    store.put(&request_id, public_key, secret).await
                                });
                                finish_operation(&phase);
                                let _ = response.send(result);
                            }
                        }
                        Request::Load(public_key, response) => {
                            let _ = response
                                .send(runtime.block_on(async { store.load(public_key).await }));
                        }
                        Request::Contains(public_key, phase, response) => {
                            if start_operation(&phase) {
                                let result =
                                    runtime.block_on(async { store.contains(public_key).await });
                                finish_operation(&phase);
                                let _ = response.send(result);
                            }
                        }
                        Request::Delete(request_id, public_key, phase, response) => {
                            if start_operation(&phase) {
                                let result = runtime.block_on(async {
                                    store.delete(&request_id, public_key).await
                                });
                                finish_operation(&phase);
                                let _ = response.send(result);
                            }
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
        request: impl FnOnce(Arc<AtomicU8>, oneshot::Sender<T>) -> Request,
    ) -> Result<T, SafeError> {
        let (response_sender, response_receiver) = oneshot::channel();
        let phase = Arc::new(AtomicU8::new(OPERATION_QUEUED));
        let mut cancellation = CancellationGuard::new(Arc::clone(&phase));
        {
            let sender_guard = self.sender.lock().map_err(|_| worker_unavailable())?;
            let Some(sender) = sender_guard.as_ref() else {
                return Err(worker_unavailable());
            };
            sender
                .try_send(request(Arc::clone(&phase), response_sender))
                .map_err(|_| worker_unavailable())?;
        }
        let response = response_receiver.await;
        cancellation.disarm();
        response.map_err(|_| match phase.load(Ordering::Acquire) {
            OPERATION_STARTED | OPERATION_COMPLETED => recovery_required(),
            _ => worker_unavailable(),
        })
    }

    pub(crate) async fn close(&self) -> Result<(), SafeError> {
        self.close_with_deadline(KEYRING_SHUTDOWN_DEADLINE).await
    }

    async fn close_with_deadline(&self, deadline: Duration) -> Result<(), SafeError> {
        let sender = self.sender.lock().map_err(|_| worker_unavailable())?.take();
        if let Some(sender) = sender {
            signal_close(sender);
        }
        let mut completion = self.completion.clone();
        tokio::time::timeout(deadline, async {
            while !*completion.borrow() {
                completion
                    .changed()
                    .await
                    .map_err(|_| worker_unavailable())?;
            }
            loop {
                let finished = self
                    .thread
                    .lock()
                    .map_err(|_| worker_unavailable())?
                    .as_ref()
                    .is_none_or(JoinHandle::is_finished);
                if finished {
                    break;
                }
                tokio::task::yield_now().await;
            }
            Ok::<(), SafeError>(())
        })
        .await
        .map_err(|_| recovery_required())??;
        let thread = self.thread.lock().map_err(|_| worker_unavailable())?.take();
        if let Some(thread) = thread {
            thread.join().map_err(|_| recovery_required())?;
        }
        Ok(())
    }
}

fn start_operation(phase: &AtomicU8) -> bool {
    phase
        .compare_exchange(
            OPERATION_QUEUED,
            OPERATION_STARTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

fn finish_operation(phase: &AtomicU8) {
    phase.store(OPERATION_COMPLETED, Ordering::Release);
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
    fn put<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            self.submit(|phase, response| {
                Request::Put(request_id.clone(), public_key, secret, phase, response)
            })
            .await?
        })
    }

    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
        Box::pin(async move {
            self.submit(|_phase, response| Request::Load(public_key, response))
                .await?
        })
    }

    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
        Box::pin(async move {
            self.submit(|phase, response| Request::Contains(public_key, phase, response))
                .await?
        })
    }

    fn delete<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            self.submit(|phase, response| {
                Request::Delete(request_id.clone(), public_key, phase, response)
            })
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

const fn recovery_required() -> SafeError {
    SafeError::new(
        SafeErrorCode::PendingOperationRecoveryRequired,
        SafeMessage::new("Credential operation recovery is required."),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
    use std::time::Duration;

    use harvestcircle_application::{
        BoxFuture, DurableRequestId, InMemorySecretStore, SecretStore,
    };
    use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SecretKeyInput};
    use tokio::sync::oneshot;

    use super::{
        BoundedKeyringWorker, CancellationGuard, KEYRING_QUEUE_CAPACITY, OPERATION_CANCELLED,
        OPERATION_COMPLETED, OPERATION_QUEUED, OPERATION_STARTED, Request, finish_operation,
        signal_close, start_operation,
    };

    fn public_key() -> PublicKey {
        PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
            .expect("public key")
    }

    fn request_id() -> DurableRequestId {
        DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000249").expect("request")
    }

    fn alternate_request_id() -> DurableRequestId {
        DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000250").expect("request")
    }

    fn secret() -> SecretKeyInput {
        SecretKeyInput::parse(
            "0000000000000000000000000000000000000000000000000000000000000001".to_owned(),
        )
        .expect("secret")
    }

    struct BlockingPutState {
        inner: InMemorySecretStore,
        block_next_put: AtomicBool,
        put_started: AtomicBool,
        release_put: AtomicBool,
        put_calls: AtomicUsize,
    }

    #[derive(Clone)]
    struct BlockingPutStore {
        state: Arc<BlockingPutState>,
    }

    impl BlockingPutStore {
        fn new() -> Self {
            Self {
                state: Arc::new(BlockingPutState {
                    inner: InMemorySecretStore::default(),
                    block_next_put: AtomicBool::new(true),
                    put_started: AtomicBool::new(false),
                    release_put: AtomicBool::new(false),
                    put_calls: AtomicUsize::new(0),
                }),
            }
        }

        async fn wait_until_started(&self) {
            while !self.state.put_started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        }

        fn release(&self) {
            self.state.release_put.store(true, Ordering::Release);
        }

        fn put_calls(&self) -> usize {
            self.state.put_calls.load(Ordering::Acquire)
        }

        async fn contains_direct(&self, public_key: PublicKey) -> bool {
            self.state
                .inner
                .contains(public_key)
                .await
                .expect("contains")
        }
    }

    impl SecretStore for BlockingPutStore {
        fn put<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
            secret: SecretKeyInput,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            Box::pin(async move {
                self.state.put_calls.fetch_add(1, Ordering::AcqRel);
                if self.state.block_next_put.swap(false, Ordering::AcqRel) {
                    self.state.put_started.store(true, Ordering::Release);
                    while !self.state.release_put.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                }
                self.state.inner.put(request_id, public_key, secret).await
            })
        }

        fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
            self.state.inner.load(public_key)
        }

        fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
            self.state.inner.contains(public_key)
        }

        fn delete<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.state.inner.delete(request_id, public_key)
        }
    }

    struct SlowContainsStore {
        inner: InMemorySecretStore,
    }

    impl SecretStore for SlowContainsStore {
        fn put<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
            secret: SecretKeyInput,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.inner.put(request_id, public_key, secret)
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

        fn delete<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.inner.delete(request_id, public_key)
        }
    }

    #[tokio::test]
    async fn worker_round_trips_without_exposing_secret_material() {
        let worker = BoundedKeyringWorker::new(InMemorySecretStore::default()).expect("worker");
        worker
            .put(&request_id(), public_key(), secret())
            .await
            .expect("put");
        assert!(worker.contains(public_key()).await.expect("contains"));
        let loaded = worker.load(public_key()).await.expect("load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        worker
            .delete(&request_id(), public_key())
            .await
            .expect("delete");
        worker.close().await.expect("close");
        assert!(worker.contains(public_key()).await.is_err());
    }

    #[test]
    fn operation_phases_are_closed_and_cancel_only_queued_work() {
        let cancelled = Arc::new(AtomicU8::new(OPERATION_QUEUED));
        drop(CancellationGuard::new(Arc::clone(&cancelled)));
        assert_eq!(cancelled.load(Ordering::Acquire), OPERATION_CANCELLED);
        assert!(!start_operation(&cancelled));

        let completed = AtomicU8::new(OPERATION_QUEUED);
        assert!(start_operation(&completed));
        assert_eq!(completed.load(Ordering::Acquire), OPERATION_STARTED);
        finish_operation(&completed);
        assert_eq!(completed.load(Ordering::Acquire), OPERATION_COMPLETED);

        let started = Arc::new(AtomicU8::new(OPERATION_STARTED));
        drop(CancellationGuard::new(Arc::clone(&started)));
        assert_eq!(started.load(Ordering::Acquire), OPERATION_STARTED);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_before_start_has_no_credential_effect() {
        let store = BlockingPutStore::new();
        let worker = BoundedKeyringWorker::new(store.clone()).expect("worker");
        let first_worker = Arc::clone(&worker);
        let first = tokio::spawn(async move {
            first_worker
                .put(&request_id(), public_key(), secret())
                .await
        });
        store.wait_until_started().await;

        let queued_worker = Arc::clone(&worker);
        let queued = tokio::spawn(async move {
            queued_worker
                .put(&alternate_request_id(), public_key(), secret())
                .await
        });
        tokio::task::yield_now().await;
        queued.abort();
        assert!(queued.await.expect_err("cancelled task").is_cancelled());

        store.release();
        first.await.expect("first task").expect("first put");
        worker.close().await.expect("close");
        assert_eq!(store.put_calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn caller_loss_after_start_preserves_unknown_outcome_for_recovery() {
        let store = BlockingPutStore::new();
        let worker = BoundedKeyringWorker::new(store.clone()).expect("worker");
        let operation_worker = Arc::clone(&worker);
        let operation = tokio::spawn(async move {
            operation_worker
                .put(&request_id(), public_key(), secret())
                .await
        });
        store.wait_until_started().await;
        operation.abort();
        assert!(operation.await.expect_err("cancelled task").is_cancelled());

        store.release();
        while !store.contains_direct(public_key()).await {
            tokio::task::yield_now().await;
        }
        worker.close().await.expect("close");
        assert_eq!(store.put_calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_timeout_is_recovery_required_and_retry_joins_thread() {
        let store = BlockingPutStore::new();
        let worker = BoundedKeyringWorker::new(store.clone()).expect("worker");
        let operation_worker = Arc::clone(&worker);
        let operation = tokio::spawn(async move {
            operation_worker
                .put(&request_id(), public_key(), secret())
                .await
        });
        store.wait_until_started().await;

        let timeout = worker
            .close_with_deadline(Duration::from_millis(1))
            .await
            .expect_err("blocked worker must time out");
        assert_eq!(
            timeout.code(),
            SafeErrorCode::PendingOperationRecoveryRequired
        );
        assert!(worker.thread.lock().expect("thread").is_some());

        store.release();
        operation.await.expect("operation task").expect("put");
        worker
            .close_with_deadline(Duration::from_secs(1))
            .await
            .expect("retry close");
        assert!(worker.thread.lock().expect("thread").is_none());
        assert!(*worker.completion.borrow());
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
        let (sender, receiver) = std::sync::mpsc::sync_channel(KEYRING_QUEUE_CAPACITY);
        for _ in 0..KEYRING_QUEUE_CAPACITY {
            let (response, _response_receiver) = oneshot::channel();
            let phase = Arc::new(AtomicU8::new(OPERATION_QUEUED));
            assert!(
                sender
                    .try_send(Request::Contains(public_key(), phase, response))
                    .is_ok()
            );
        }
        let (overflow_response, _overflow_receiver) = oneshot::channel();
        assert!(matches!(
            sender.try_send(Request::Contains(
                public_key(),
                Arc::new(AtomicU8::new(OPERATION_QUEUED)),
                overflow_response,
            )),
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));

        signal_close(sender);

        for _ in 0..KEYRING_QUEUE_CAPACITY {
            assert!(matches!(receiver.recv(), Ok(Request::Contains(_, _, _))));
        }
        assert!(receiver.recv().is_err());
    }
}
