use std::future::Future;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::runtime::{Builder, Handle};
use tokio::sync::{oneshot, watch};

pub(crate) struct HostRuntime {
    handle: Handle,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    completion: watch::Receiver<bool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(test)]
pub(crate) struct CompletionGatedHostRuntime {
    pub(crate) runtime: Arc<HostRuntime>,
    pub(crate) entered: std::sync::mpsc::Receiver<()>,
    pub(crate) release: std::sync::mpsc::SyncSender<()>,
}

impl HostRuntime {
    pub(crate) fn new() -> Result<Arc<Self>, ()> {
        Self::new_inner(None)
    }

    fn new_inner(
        completion_gate: Option<(
            std::sync::mpsc::SyncSender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
    ) -> Result<Arc<Self>, ()> {
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let (completion_sender, completion_receiver) = watch::channel(false);
        let thread = std::thread::Builder::new()
            .name("harvestcircle-host-runtime".to_owned())
            .spawn(move || {
                let Ok(runtime) = Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("harvestcircle-runtime-worker")
                    .build()
                else {
                    let _ = startup_sender.send(Err(()));
                    let _ = completion_sender.send(true);
                    return;
                };
                if startup_sender.send(Ok(runtime.handle().clone())).is_err() {
                    let _ = completion_sender.send(true);
                    return;
                }
                runtime.block_on(async {
                    let _ = shutdown_receiver.await;
                });
                runtime.shutdown_timeout(Duration::from_secs(5));
                if let Some((entered, release)) = completion_gate {
                    let _ = entered.send(());
                    let _ = release.recv();
                }
                let _ = completion_sender.send(true);
            })
            .map_err(|_| ())?;
        let handle = startup_receiver.recv().map_err(|_| ())??;
        Ok(Arc::new(Self {
            handle,
            shutdown: Mutex::new(Some(shutdown_sender)),
            completion: completion_receiver,
            thread: Mutex::new(Some(thread)),
        }))
    }

    #[cfg(test)]
    pub(crate) fn new_completion_gated_for_test() -> Result<CompletionGatedHostRuntime, ()> {
        let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let runtime = Self::new_inner(Some((entered_sender, release_receiver)))?;
        Ok(CompletionGatedHostRuntime {
            runtime,
            entered: entered_receiver,
            release: release_sender,
        })
    }

    pub(crate) fn handle(&self) -> &Handle {
        &self.handle
    }

    pub(crate) fn block_on<F>(&self, future: F) -> Result<F::Output, ()>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        self.handle.spawn(async move {
            let _ = sender.send(future.await);
        });
        receiver.recv().map_err(|_| ())
    }

    pub(crate) async fn shutdown(&self) -> Result<(), ()> {
        let sender = self.shutdown.lock().map_err(|_| ())?.take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }

        let mut completion = self.completion.clone();
        while !*completion.borrow() {
            completion.changed().await.map_err(|_| ())?;
        }

        let thread = self.thread.lock().map_err(|_| ())?.take();
        if let Some(thread) = thread {
            thread.join().map_err(|_| ())?;
        }
        Ok(())
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        if let Ok(sender) = self.shutdown.get_mut()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::HostRuntime;

    #[test]
    fn host_runtime_executes_work_and_shuts_down_explicitly() {
        let host = HostRuntime::new().expect("host runtime");
        assert_eq!(host.block_on(async { 7 }).expect("runtime result"), 7);
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        test_runtime.block_on(host.shutdown()).expect("shutdown");
        test_runtime
            .block_on(host.shutdown())
            .expect("idempotent shutdown");
    }

    #[tokio::test]
    async fn cancelled_shutdown_is_resumable() {
        let gated = HostRuntime::new_completion_gated_for_test().expect("host runtime");
        let host = gated.runtime;
        let entered_receiver = gated.entered;
        let release_sender = gated.release;
        let first_host = Arc::clone(&host);
        let first = tokio::spawn(async move { first_host.shutdown().await });
        tokio::task::spawn_blocking(move || entered_receiver.recv())
            .await
            .expect("entered join")
            .expect("shutdown entered completion gate");
        first.abort();
        assert!(first.await.is_err());
        release_sender.send(()).expect("release shutdown");
        host.shutdown().await.expect("resumed shutdown");
    }
}
