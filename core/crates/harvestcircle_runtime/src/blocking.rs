use std::sync::Arc;
use std::time::Instant;

use tokio::runtime::Handle;
use tokio::sync::Semaphore;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockingExecutionError {
    DeadlineElapsed,
    Saturated,
    TaskFailed,
}

#[derive(Clone)]
pub(crate) struct BoundedBlockingExecutor {
    permits: Arc<Semaphore>,
    runtime: Handle,
}

impl BoundedBlockingExecutor {
    pub(crate) fn new(capacity: usize, runtime: &Handle) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(capacity)),
            runtime: runtime.clone(),
        }
    }

    pub(crate) async fn execute<T, F>(
        &self,
        deadline: Instant,
        operation: F,
    ) -> Result<T, BlockingExecutionError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        if Instant::now() >= deadline {
            return Err(BlockingExecutionError::DeadlineElapsed);
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| BlockingExecutionError::Saturated)?;
        self.runtime
            .spawn_blocking(move || {
                let _permit = permit;
                operation()
            })
            .await
            .map_err(|_| BlockingExecutionError::TaskFailed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use tokio::sync::oneshot;

    use super::{BlockingExecutionError, BoundedBlockingExecutor};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn executor_rejects_saturation_without_starting_excess_work() {
        let executor = BoundedBlockingExecutor::new(1, &tokio::runtime::Handle::current());
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let first_release = Arc::clone(&release);
        let (started, started_rx) = oneshot::channel();
        let first_executor = executor.clone();
        let first = tokio::spawn(async move {
            first_executor
                .execute(Instant::now() + Duration::from_secs(5), move || {
                    let _ = started.send(());
                    let (lock, ready) = &*first_release;
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = ready.wait(released).expect("release wait");
                    }
                    7
                })
                .await
        });
        started_rx.await.expect("first work starts");

        let second = executor
            .execute(Instant::now() + Duration::from_secs(5), || 9)
            .await;
        assert_eq!(second, Err(BlockingExecutionError::Saturated));

        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_all();
        assert_eq!(first.await.expect("first join"), Ok(7));
    }

    #[tokio::test]
    async fn executor_rejects_expired_work_before_spawn() {
        let executor = BoundedBlockingExecutor::new(1, &tokio::runtime::Handle::current());
        let result = executor.execute(Instant::now(), || 1).await;
        assert_eq!(result, Err(BlockingExecutionError::DeadlineElapsed));
    }

    #[tokio::test]
    async fn executor_classifies_panicked_work_without_panicking_the_actor() {
        let executor = BoundedBlockingExecutor::new(1, &tokio::runtime::Handle::current());
        let result = executor
            .execute::<(), _>(Instant::now() + Duration::from_secs(1), || {
                panic!("test-only blocking task failure");
            })
            .await;
        assert_eq!(result, Err(BlockingExecutionError::TaskFailed));
    }
}
