use std::collections::BTreeMap;
use std::future::Future;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radroots_studio_application::{
    ActorMailbox, AppSnapshot, ChangeSubscriptionId, Clock, CommandContext, CommandEnvelope,
    CommandReceipt, CommandResult, CommandSubmission, DurableRequestId, ForegroundSessionBinding,
    GenerateAccountReceipt, GeneratedKeyRecoveryHandle, GeneratedKeyStage, ImportAccountReceipt,
    LifecycleGate, NostrClient, OrderedSnapshotChanges, ProfileFetchResult, ProfileRefreshPlan,
    RecoveryStageId, RelayConfiguration, RemovalConfirmationToken, RequestId, RuntimeCommandClass,
    RuntimeLifecycle, SecretStore, SessionGeneration, SnapshotChange, SnapshotChangeReceiver,
    SnapshotRevision, StagedGeneratedKey, TaskCorrelation,
};
use radroots_studio_domain::{
    AccountIdentity, BindingAvailability, LocalSignerBinding, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, SecretKeyInput,
};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, watch};

use crate::blocking::{BlockingExecutionError, BoundedBlockingExecutor};
use crate::{InstallationIdentity, InstallationIdentitySource, PersistentAppCore};

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TASK_CAPACITY: usize = 64;
const DEFAULT_BLOCKING_CAPACITY: usize = 4;

enum RuntimeCommand {
    Snapshot,
    GenerateAccount {
        durable_request: DurableRequestId,
        expected_revision: u64,
    },
    BeginGeneratedKeyStage,
    AcknowledgeGeneratedKeyStage {
        id: RecoveryStageId,
        durable_request: DurableRequestId,
    },
    CancelGeneratedKeyStage,
    ImportSecretKey {
        input: SecretKeyInput,
        durable_request: DurableRequestId,
        expected_revision: u64,
    },
    SelectAccount(PublicKey),
    ActivateAccount(PublicKey),
    SignOut,
    RefreshActiveProfile,
    RequestAccountRemoval(PublicKey),
    ConfirmAccountRemoval {
        token: RemovalConfirmationToken,
        durable_request: DurableRequestId,
    },
    SubscribeChanges(NonZeroUsize),
    UnsubscribeChanges(ChangeSubscriptionId),
    Close,
}

enum RuntimeCommandValue {
    Snapshot(Box<AppSnapshot>),
    Generated(GenerateAccountReceipt),
    GeneratedKeyStage(GeneratedKeyRecoveryHandle),
    GeneratedKeyStageCancelled(bool),
    Imported(ImportAccountReceipt),
    RemovalRequest(RemovalConfirmationToken),
    Subscription(RuntimeChangeSubscription),
    Unsubscribed(bool),
    Closed,
}

impl RuntimeCommand {
    const fn class(&self) -> RuntimeCommandClass {
        match self {
            Self::Snapshot | Self::SubscribeChanges(_) | Self::UnsubscribeChanges(_) => {
                RuntimeCommandClass::Observe
            }
            Self::GenerateAccount { .. }
            | Self::BeginGeneratedKeyStage
            | Self::AcknowledgeGeneratedKeyStage { .. }
            | Self::ImportSecretKey { .. }
            | Self::ActivateAccount(_)
            | Self::ConfirmAccountRemoval { .. } => RuntimeCommandClass::UseCredential,
            Self::SelectAccount(_)
            | Self::SignOut
            | Self::RequestAccountRemoval(_)
            | Self::CancelGeneratedKeyStage => RuntimeCommandClass::MutateLocalState,
            Self::RefreshActiveProfile => RuntimeCommandClass::UseRelay,
            Self::Close => RuntimeCommandClass::Shutdown,
        }
    }

    const fn resolves_revision_through_durable_replay(&self) -> bool {
        matches!(
            self,
            Self::GenerateAccount { .. } | Self::ImportSecretKey { .. }
        )
    }
}

struct RuntimeActor {
    adapter: Arc<PersistentAppCore>,
    secrets: Arc<dyn SecretStore>,
    clock: Arc<dyn Clock>,
    nostr: Arc<dyn NostrClient>,
    lifecycle: Arc<Mutex<LifecycleGate>>,
    runtime: Handle,
    blocking: BoundedBlockingExecutor,
    session_generation: SessionGeneration,
    published_session_generation: Arc<AtomicU64>,
    profile_tasks: BTreeMap<RequestId, PendingProfileTask>,
    changes: OrderedSnapshotChanges,
    published_foreground_session: Arc<Mutex<Option<ForegroundSessionBinding>>>,
    generated_key_stage: GeneratedKeyStage,
}

struct PendingProfileTask {
    correlation: TaskCorrelation,
    plan: ProfileRefreshPlan,
    deadline: Instant,
    reply: oneshot::Sender<CommandReceipt<RuntimeCommandValue>>,
    handle: tokio::task::JoinHandle<()>,
}

struct ProfileCompletion {
    request_id: RequestId,
    result: Result<ProfileFetchResult, SafeError>,
}

#[derive(Clone)]
pub struct RuntimeActorHandle {
    mailbox: ActorMailbox<RuntimeCommand, RuntimeCommandValue>,
    adapter: Arc<PersistentAppCore>,
    lifecycle: Arc<Mutex<LifecycleGate>>,
    next_request: Arc<AtomicU64>,
    session_generation: Arc<AtomicU64>,
    foreground_session: Arc<Mutex<Option<ForegroundSessionBinding>>>,
    installation_identity: InstallationIdentity,
    runtime: Handle,
    actor_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    actor_exit: watch::Receiver<bool>,
}

#[derive(Clone)]
pub struct RuntimeDependencies {
    secrets: Arc<dyn SecretStore>,
    clock: Arc<dyn Clock>,
    nostr: Arc<dyn NostrClient>,
    installation_source: Arc<dyn InstallationIdentitySource>,
}

impl RuntimeDependencies {
    #[must_use]
    pub fn new(
        secrets: Arc<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        nostr: Arc<dyn NostrClient>,
        installation_source: Arc<dyn InstallationIdentitySource>,
    ) -> Self {
        Self {
            secrets,
            clock,
            nostr,
            installation_source,
        }
    }
}

pub struct RuntimeChangeSubscription {
    id: ChangeSubscriptionId,
    receiver: SnapshotChangeReceiver,
}

impl RuntimeChangeSubscription {
    #[must_use]
    pub const fn id(&self) -> ChangeSubscriptionId {
        self.id
    }

    pub async fn receive(&mut self) -> Option<SnapshotChange> {
        self.receiver.receive().await
    }
}

impl RuntimeActorHandle {
    /// Opens, migrates, recovers, and starts one actor-owned file-backed runtime.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or lifecycle error before the actor is
    /// published when opening cannot reach ready state.
    pub async fn open(
        path: &Path,
        relay_configuration: RelayConfiguration,
        dependencies: RuntimeDependencies,
        capacity: NonZeroUsize,
        runtime: &Handle,
    ) -> Result<Self, SafeError> {
        let blocking = BoundedBlockingExecutor::new(DEFAULT_BLOCKING_CAPACITY, runtime);
        let path = path.to_path_buf();
        let adapter = blocking
            .execute(Instant::now() + DEFAULT_COMMAND_TIMEOUT, move || {
                PersistentAppCore::open(&path, relay_configuration)
            })
            .await
            .map_err(blocking_execution_failed)??;
        Self::start(adapter, dependencies, capacity, runtime, blocking).await
    }

    /// Starts one isolated actor-owned in-memory runtime for tests.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or lifecycle error before publication.
    pub async fn in_memory(
        relay_configuration: RelayConfiguration,
        dependencies: RuntimeDependencies,
        capacity: NonZeroUsize,
        runtime: &Handle,
    ) -> Result<Self, SafeError> {
        let blocking = BoundedBlockingExecutor::new(DEFAULT_BLOCKING_CAPACITY, runtime);
        let adapter = blocking
            .execute(Instant::now() + DEFAULT_COMMAND_TIMEOUT, move || {
                PersistentAppCore::in_memory(relay_configuration)
            })
            .await
            .map_err(blocking_execution_failed)??;
        Self::start(adapter, dependencies, capacity, runtime, blocking).await
    }

    async fn start(
        adapter: PersistentAppCore,
        dependencies: RuntimeDependencies,
        capacity: NonZeroUsize,
        runtime: &Handle,
        blocking: BoundedBlockingExecutor,
    ) -> Result<Self, SafeError> {
        let mut gate = LifecycleGate::opening();
        gate.begin_compatibility_check()?;
        gate.compatibility_accepted()?;
        gate.ownership_acquired()?;
        gate.migration_complete()?;
        let RuntimeDependencies {
            secrets,
            clock,
            nostr,
            installation_source,
        } = dependencies;
        let adapter = Arc::new(adapter);
        let bootstrap_adapter = Arc::clone(&adapter);
        let bootstrap_secrets = Arc::clone(&secrets);
        let bootstrap_clock = Arc::clone(&clock);
        let installation_identity = blocking
            .execute(Instant::now() + DEFAULT_COMMAND_TIMEOUT, move || {
                bootstrap_adapter
                    .bootstrap(bootstrap_secrets.as_ref(), bootstrap_clock.as_ref())?;
                bootstrap_adapter.initialize_installation_identity(installation_source.as_ref())
            })
            .await
            .map_err(blocking_execution_failed)??;
        gate.recovery_complete()?;

        let lifecycle = Arc::new(Mutex::new(gate));
        let (mailbox, receiver) = ActorMailbox::bounded(capacity);
        let session_generation = Arc::new(AtomicU64::new(SessionGeneration::initial().value()));
        let foreground_session = Arc::new(Mutex::new(None));
        let changes = OrderedSnapshotChanges::new(adapter.core().snapshot());
        let actor = RuntimeActor {
            adapter: Arc::clone(&adapter),
            secrets,
            clock,
            nostr,
            lifecycle: Arc::clone(&lifecycle),
            runtime: runtime.clone(),
            blocking,
            session_generation: SessionGeneration::initial(),
            published_session_generation: Arc::clone(&session_generation),
            profile_tasks: BTreeMap::new(),
            changes,
            published_foreground_session: Arc::clone(&foreground_session),
            generated_key_stage: GeneratedKeyStage::default(),
        };
        let (actor_exit_sender, actor_exit) = watch::channel(false);
        let actor_task = runtime.spawn(async move {
            actor.run(receiver).await;
            let _ = actor_exit_sender.send(true);
        });
        Ok(Self {
            mailbox,
            adapter,
            lifecycle,
            next_request: Arc::new(AtomicU64::new(1)),
            session_generation,
            foreground_session,
            installation_identity,
            runtime: runtime.clone(),
            actor_task: Arc::new(Mutex::new(Some(actor_task))),
            actor_exit,
        })
    }

    #[must_use]
    pub fn lifecycle(&self) -> RuntimeLifecycle {
        self.lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lifecycle()
    }

    #[must_use]
    pub fn session_generation(&self) -> SessionGeneration {
        SessionGeneration::from_value(self.session_generation.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn foreground_session(&self) -> Option<ForegroundSessionBinding> {
        self.foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn installation_identity(&self) -> &InstallationIdentity {
        &self.installation_identity
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshot {
        self.adapter.core().snapshot()
    }

    /// Returns the ready snapshot through the actor command boundary.
    ///
    /// # Errors
    ///
    /// Returns a typed safe actor error.
    pub async fn bootstrap(&self) -> Result<AppSnapshot, SafeError> {
        Self::expect_snapshot(self.dispatch(RuntimeCommand::Snapshot, None).await?)
    }

    /// Generates one account through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, storage, keyring, timeout, or actor error.
    pub async fn generate_account(
        &self,
        request: DurableRequestId,
        expected_revision: SnapshotRevision,
        timeout: Duration,
    ) -> Result<GenerateAccountReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::GenerateAccount {
                    durable_request: request,
                    expected_revision: expected_revision.value(),
                },
                expected_revision,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::Generated(receipt) => Ok(receipt),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Begins the only actor-owned generated-key recovery stage.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, timeout, key-generation, or actor error.
    pub async fn begin_generated_key_stage(&self) -> Result<GeneratedKeyRecoveryHandle, SafeError> {
        match self
            .dispatch(RuntimeCommand::BeginGeneratedKeyStage, None)
            .await?
        {
            RuntimeCommandValue::GeneratedKeyStage(view) => Ok(view),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Acknowledges recovery and commits the staged account and credential once.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable, conflict, keyring, storage, timeout, or actor error.
    pub async fn acknowledge_generated_key_stage(
        &self,
        id: RecoveryStageId,
        request: DurableRequestId,
        expected_revision: SnapshotRevision,
        timeout: Duration,
    ) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch_durable(
                RuntimeCommand::AcknowledgeGeneratedKeyStage {
                    id,
                    durable_request: request,
                },
                expected_revision,
                timeout,
            )
            .await?;
        Self::expect_snapshot(value)
    }

    /// Cancels and zeroizes the active generated-key stage, if present.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or actor error.
    pub async fn cancel_generated_key_stage(&self) -> Result<bool, SafeError> {
        match self
            .dispatch(RuntimeCommand::CancelGeneratedKeyStage, None)
            .await?
        {
            RuntimeCommandValue::GeneratedKeyStageCancelled(cancelled) => Ok(cancelled),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Imports one account through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, storage, keyring, timeout, or actor error.
    pub async fn import_secret_key(
        &self,
        request: DurableRequestId,
        expected_revision: SnapshotRevision,
        input: SecretKeyInput,
        timeout: Duration,
    ) -> Result<ImportAccountReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::ImportSecretKey {
                    input,
                    durable_request: request,
                    expected_revision: expected_revision.value(),
                },
                expected_revision,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::Imported(receipt) => Ok(receipt),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Selects one account through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, storage, timeout, or actor error.
    pub async fn select_account(&self, public_key: PublicKey) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch(RuntimeCommand::SelectAccount(public_key), None)
            .await?;
        Self::expect_snapshot(value)
    }

    /// Activates one account through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, credential, storage, timeout, or actor error.
    pub async fn activate_account(&self, public_key: PublicKey) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch(RuntimeCommand::ActivateAccount(public_key), None)
            .await?;
        Self::expect_snapshot(value)
    }

    /// Signs out through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or actor error.
    pub async fn sign_out(&self) -> Result<AppSnapshot, SafeError> {
        let value = self.dispatch(RuntimeCommand::SignOut, None).await?;
        Self::expect_snapshot(value)
    }

    /// Refreshes the active profile through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe relay, storage, timeout, or actor error.
    pub async fn refresh_active_profile(&self) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch(RuntimeCommand::RefreshActiveProfile, None)
            .await?;
        Self::expect_snapshot(value)
    }

    /// Creates one removal request through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, timeout, or actor error.
    pub async fn request_account_removal(
        &self,
        public_key: PublicKey,
    ) -> Result<RemovalConfirmationToken, SafeError> {
        match self
            .dispatch(RuntimeCommand::RequestAccountRemoval(public_key), None)
            .await?
        {
            RuntimeCommandValue::RemovalRequest(token) => Ok(token),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Confirms one removal through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe account, credential, storage, timeout, or actor error.
    pub async fn confirm_account_removal(
        &self,
        token: RemovalConfirmationToken,
        request: DurableRequestId,
        expected_revision: SnapshotRevision,
        timeout: Duration,
    ) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch_durable(
                RuntimeCommand::ConfirmAccountRemoval {
                    token,
                    durable_request: request,
                },
                expected_revision,
                timeout,
            )
            .await?;
        Self::expect_snapshot(value)
    }

    /// Closes command admission and cancels supervised work.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or actor error. Repeated calls return closed.
    pub async fn close(&self) -> Result<(), SafeError> {
        self.close_with_timeout(DEFAULT_COMMAND_TIMEOUT).await
    }

    /// Closes the runtime within the supplied command deadline.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or actor error. An expired queued close cannot
    /// later change runtime state.
    pub async fn close_with_timeout(&self, timeout: Duration) -> Result<(), SafeError> {
        let deadline = Instant::now() + timeout;
        if matches!(self.lifecycle(), RuntimeLifecycle::Closed) {
            return self.await_actor_exit(deadline).await;
        }
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        let command_result = match self
            .dispatch_with_deadline(RuntimeCommand::Close, None, request_id, deadline)
            .await
        {
            Ok(RuntimeCommandValue::Closed) => Ok(()),
            Ok(_) => Err(invalid_actor_response()),
            Err(error) => Err(error),
        };
        let exit_result = self.await_actor_exit(deadline).await;
        if matches!(self.lifecycle(), RuntimeLifecycle::Closed) {
            exit_result
        } else {
            command_result.and(exit_result)
        }
    }

    async fn await_actor_exit(&self, deadline: Instant) -> Result<(), SafeError> {
        let mut actor_exit = self.actor_exit.clone();
        if !*actor_exit.borrow() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            self.timeout(remaining, actor_exit.wait_for(|exited| *exited))
                .await
                .map_err(|_| command_timed_out())?
                .map_err(|_| runtime_closed())?;
        }
        let actor_task = self
            .actor_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(actor_task) = actor_task {
            let remaining = deadline.saturating_duration_since(Instant::now());
            self.timeout(remaining, actor_task)
                .await
                .map_err(|_| command_timed_out())?
                .map_err(|_| runtime_closed())?;
        }
        Ok(())
    }

    /// Atomically registers a bounded ordered change consumer with its initial snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe actor or subscription error.
    pub async fn subscribe_changes(
        &self,
        capacity: NonZeroUsize,
    ) -> Result<RuntimeChangeSubscription, SafeError> {
        match self
            .dispatch(RuntimeCommand::SubscribeChanges(capacity), None)
            .await?
        {
            RuntimeCommandValue::Subscription(subscription) => Ok(subscription),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Removes a change consumer through the serialized actor boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe actor error.
    pub async fn unsubscribe_changes(&self, id: ChangeSubscriptionId) -> Result<bool, SafeError> {
        match self
            .dispatch(RuntimeCommand::UnsubscribeChanges(id), None)
            .await?
        {
            RuntimeCommandValue::Unsubscribed(removed) => Ok(removed),
            _ => Err(invalid_actor_response()),
        }
    }

    async fn dispatch(
        &self,
        command: RuntimeCommand,
        expected_revision: Option<SnapshotRevision>,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        self.dispatch_with_deadline(
            command,
            expected_revision,
            request_id,
            Instant::now() + DEFAULT_COMMAND_TIMEOUT,
        )
        .await
    }

    async fn dispatch_durable(
        &self,
        command: RuntimeCommand,
        expected_revision: SnapshotRevision,
        timeout: Duration,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        self.dispatch_with_deadline(
            command,
            Some(expected_revision),
            request_id,
            Instant::now() + timeout,
        )
        .await
    }

    async fn dispatch_with_deadline(
        &self,
        command: RuntimeCommand,
        expected_revision: Option<SnapshotRevision>,
        request_id: RequestId,
        deadline: Instant,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let context = CommandContext::new(request_id, expected_revision, deadline);
        let receipt = match self.mailbox.submit(context, command) {
            CommandSubmission::Accepted(ticket) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match self.timeout(remaining, ticket.receipt()).await {
                    Ok(receipt) => receipt,
                    Err(_) => CommandReceipt::new(request_id, CommandResult::TimedOut),
                }
            }
            CommandSubmission::Rejected(receipt) => receipt,
        };
        match receipt.into_result() {
            CommandResult::Completed(value) => Ok(value),
            CommandResult::Conflicted { .. } => Err(command_conflicted()),
            CommandResult::Rejected(_) => Err(command_rejected()),
            CommandResult::TimedOut => Err(command_timed_out()),
            CommandResult::Closed => Err(runtime_closed()),
            CommandResult::Failed(error) => Err(error),
        }
    }

    fn timeout<F>(&self, duration: Duration, future: F) -> tokio::time::Timeout<F>
    where
        F: Future,
    {
        let _guard = self.runtime.enter();
        tokio::time::timeout(duration, future)
    }

    #[cfg(test)]
    async fn import_secret_key_test(
        &self,
        input: SecretKeyInput,
    ) -> Result<ImportAccountReceipt, SafeError> {
        let request_number = self.next_request.fetch_add(1, Ordering::Relaxed);
        self.import_secret_key(
            DurableRequestId::parse(format!("test:import:{request_number}"))?,
            self.snapshot().revision(),
            input,
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
    }

    #[cfg(test)]
    async fn acknowledge_generated_key_stage_test(
        &self,
        id: RecoveryStageId,
    ) -> Result<AppSnapshot, SafeError> {
        let request_number = self.next_request.fetch_add(1, Ordering::Relaxed);
        self.acknowledge_generated_key_stage(
            id,
            DurableRequestId::parse(format!("test:generate:{request_number}"))?,
            self.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
    }

    #[cfg(test)]
    async fn confirm_account_removal_test(
        &self,
        token: RemovalConfirmationToken,
    ) -> Result<AppSnapshot, SafeError> {
        let request_number = self.next_request.fetch_add(1, Ordering::Relaxed);
        self.confirm_account_removal(
            token,
            DurableRequestId::parse(format!("test:remove:{request_number}"))?,
            self.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
    }

    #[cfg(test)]
    async fn import_secret_key_with_timeout(
        &self,
        input: SecretKeyInput,
        timeout: Duration,
    ) -> Result<ImportAccountReceipt, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        let expected_revision = self.adapter.core().snapshot().revision();
        let durable_request = DurableRequestId::parse(format!("test:timeout:{raw_request}"))?;
        match self
            .dispatch_with_deadline(
                RuntimeCommand::ImportSecretKey {
                    input,
                    durable_request,
                    expected_revision: expected_revision.value(),
                },
                Some(expected_revision),
                request_id,
                Instant::now() + timeout,
            )
            .await?
        {
            RuntimeCommandValue::Imported(receipt) => Ok(receipt),
            _ => Err(invalid_actor_response()),
        }
    }

    fn expect_snapshot(value: RuntimeCommandValue) -> Result<AppSnapshot, SafeError> {
        match value {
            RuntimeCommandValue::Snapshot(snapshot) => Ok(*snapshot),
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActor {
    async fn run(
        mut self,
        mut receiver: mpsc::Receiver<CommandEnvelope<RuntimeCommand, RuntimeCommandValue>>,
    ) {
        let (completion_sender, mut completions) = mpsc::channel(DEFAULT_TASK_CAPACITY);
        loop {
            tokio::select! {
                envelope = receiver.recv() => {
                    let Some(envelope) = envelope else {
                        break;
                    };
                    if !self.handle_command(envelope, &completion_sender).await {
                        break;
                    }
                }
                completion = completions.recv(), if !self.profile_tasks.is_empty() => {
                    if let Some(completion) = completion {
                        self.complete_profile_task(completion).await;
                    }
                }
            }
        }
        self.cancel_profile_tasks(None).await;
    }

    async fn handle_command(
        &mut self,
        envelope: CommandEnvelope<RuntimeCommand, RuntimeCommandValue>,
        completion_sender: &mpsc::Sender<ProfileCompletion>,
    ) -> bool {
        let (context, command, reply) = envelope.into_parts();
        if let Some(result) = self.preflight(context, &command) {
            let _ = reply.send(CommandReceipt::new(context.request_id(), result));
            return true;
        }
        if matches!(command, RuntimeCommand::RefreshActiveProfile) {
            self.start_profile_task(context, reply, completion_sender.clone())
                .await;
            return true;
        }
        if matches!(command, RuntimeCommand::Close) {
            let result = self.close_actor().await;
            let closed = matches!(result, CommandResult::Completed(_));
            let _ = reply.send(CommandReceipt::new(context.request_id(), result));
            return !closed;
        }
        let changes_session = matches!(
            command,
            RuntimeCommand::ActivateAccount(_)
                | RuntimeCommand::SignOut
                | RuntimeCommand::ConfirmAccountRemoval { .. }
        );
        let begins_generated_recovery = matches!(&command, RuntimeCommand::BeginGeneratedKeyStage);
        let result = self.execute_command(context, command).await;
        if begins_generated_recovery && matches!(&result, CommandResult::Completed(_)) {
            let snapshot = self.adapter.core().snapshot();
            self.cancel_profile_tasks(Some(&snapshot)).await;
        }
        if changes_session && matches!(result, CommandResult::Completed(_)) {
            self.advance_session_generation().await;
            self.synchronize_foreground_session();
        }
        if matches!(result, CommandResult::Completed(_)) {
            self.changes.publish(self.adapter.core().snapshot());
        }
        let _ = reply.send(CommandReceipt::new(context.request_id(), result));
        true
    }

    fn preflight(
        &self,
        context: CommandContext,
        command: &RuntimeCommand,
    ) -> Option<CommandResult<RuntimeCommandValue>> {
        if context.is_expired(Instant::now()) {
            return Some(CommandResult::TimedOut);
        }
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .to_owned();
        if matches!(lifecycle.lifecycle(), RuntimeLifecycle::Closed) {
            return Some(CommandResult::Closed);
        }
        if !lifecycle.allows(command.class()) {
            return Some(CommandResult::Failed(command_unavailable()));
        }
        if self.generated_key_stage.pending().is_some()
            && !matches!(
                command,
                RuntimeCommand::Snapshot
                    | RuntimeCommand::AcknowledgeGeneratedKeyStage { .. }
                    | RuntimeCommand::CancelGeneratedKeyStage
                    | RuntimeCommand::SubscribeChanges(_)
                    | RuntimeCommand::UnsubscribeChanges(_)
                    | RuntimeCommand::Close
            )
        {
            return Some(CommandResult::Failed(generated_recovery_route_active()));
        }
        let current_revision = self.adapter.core().snapshot().revision();
        if !command.resolves_revision_through_durable_replay()
            && context
                .expected_revision()
                .is_some_and(|expected| expected != current_revision)
        {
            return Some(CommandResult::Conflicted { current_revision });
        }
        None
    }

    async fn execute_command(
        &mut self,
        context: CommandContext,
        command: RuntimeCommand,
    ) -> CommandResult<RuntimeCommandValue> {
        let result = match command {
            RuntimeCommand::Snapshot => Ok(RuntimeCommandValue::Snapshot(Box::new(
                self.adapter.core().snapshot(),
            ))),
            RuntimeCommand::GenerateAccount {
                durable_request,
                expected_revision,
            } => {
                self.run_blocking(context.deadline(), move |adapter, secrets, clock| {
                    adapter
                        .generate_account_durable(
                            &durable_request,
                            expected_revision,
                            secrets.as_ref(),
                            clock.as_ref(),
                        )
                        .map(RuntimeCommandValue::Generated)
                })
                .await
            }
            RuntimeCommand::BeginGeneratedKeyStage => {
                match NonZeroU64::new(context.request_id().get()).map(RecoveryStageId::new) {
                    Some(stage_id) => self
                        .generated_key_stage
                        .begin(
                            self.adapter.key_material(),
                            stage_id,
                            self.adapter.core().snapshot().revision().value(),
                            self.clock.now(),
                        )
                        .map(RuntimeCommandValue::GeneratedKeyStage),
                    None => Err(request_space_exhausted()),
                }
            }
            RuntimeCommand::AcknowledgeGeneratedKeyStage {
                id,
                durable_request,
            } => match self.generated_key_stage.take(id, self.clock.now()) {
                Ok(staged) => {
                    self.run_blocking(context.deadline(), move |adapter, secrets, clock| {
                        commit_generated_key_stage(
                            adapter.as_ref(),
                            secrets.as_ref(),
                            clock.as_ref(),
                            &durable_request,
                            staged,
                        )
                    })
                    .await
                }
                Err(error) => Err(error),
            },
            RuntimeCommand::CancelGeneratedKeyStage => Ok(
                RuntimeCommandValue::GeneratedKeyStageCancelled(self.generated_key_stage.cancel()),
            ),
            RuntimeCommand::ImportSecretKey {
                input,
                durable_request,
                expected_revision,
            } => {
                self.run_blocking(context.deadline(), move |adapter, secrets, clock| {
                    adapter
                        .import_secret_key_durable(
                            &durable_request,
                            expected_revision,
                            input,
                            secrets.as_ref(),
                            clock.as_ref(),
                        )
                        .map(RuntimeCommandValue::Imported)
                })
                .await
            }
            RuntimeCommand::SelectAccount(public_key) => {
                self.run_blocking(context.deadline(), move |adapter, _, _| {
                    adapter
                        .select_account(public_key)
                        .map(Box::new)
                        .map(RuntimeCommandValue::Snapshot)
                })
                .await
            }
            RuntimeCommand::ActivateAccount(public_key) => {
                self.run_blocking(context.deadline(), move |adapter, secrets, clock| {
                    adapter
                        .activate_account(public_key, secrets.as_ref(), clock.as_ref())
                        .map(Box::new)
                        .map(RuntimeCommandValue::Snapshot)
                })
                .await
            }
            RuntimeCommand::SignOut => self
                .adapter
                .sign_out()
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot),
            RuntimeCommand::RequestAccountRemoval(public_key) => self
                .adapter
                .request_account_removal(public_key, self.clock.as_ref())
                .map(RuntimeCommandValue::RemovalRequest),
            RuntimeCommand::ConfirmAccountRemoval {
                token,
                durable_request,
            } => {
                self.run_blocking(context.deadline(), move |adapter, secrets, clock| {
                    adapter
                        .confirm_account_removal_durable(
                            &durable_request,
                            token,
                            secrets.as_ref(),
                            clock.as_ref(),
                        )
                        .map(Box::new)
                        .map(RuntimeCommandValue::Snapshot)
                })
                .await
            }
            RuntimeCommand::SubscribeChanges(capacity) => self
                .changes
                .subscribe(capacity)
                .map(|(id, receiver)| {
                    RuntimeCommandValue::Subscription(RuntimeChangeSubscription { id, receiver })
                })
                .ok_or_else(observer_registration_failed),
            RuntimeCommand::UnsubscribeChanges(id) => Ok(RuntimeCommandValue::Unsubscribed(
                self.changes.unsubscribe(id),
            )),
            RuntimeCommand::Close | RuntimeCommand::RefreshActiveProfile => {
                Err(invalid_actor_response())
            }
        };
        result.map_or_else(CommandResult::Failed, CommandResult::Completed)
    }

    async fn run_blocking<F>(
        &self,
        deadline: Instant,
        operation: F,
    ) -> Result<RuntimeCommandValue, SafeError>
    where
        F: FnOnce(
                Arc<PersistentAppCore>,
                Arc<dyn SecretStore>,
                Arc<dyn Clock>,
            ) -> Result<RuntimeCommandValue, SafeError>
            + Send
            + 'static,
    {
        let adapter = Arc::clone(&self.adapter);
        let secrets = Arc::clone(&self.secrets);
        let clock = Arc::clone(&self.clock);
        self.blocking
            .execute(deadline, move || operation(adapter, secrets, clock))
            .await
            .map_err(blocking_execution_failed)?
    }

    async fn start_profile_task(
        &mut self,
        context: CommandContext,
        reply: oneshot::Sender<CommandReceipt<RuntimeCommandValue>>,
        completion_sender: mpsc::Sender<ProfileCompletion>,
    ) {
        let foreground = self
            .published_foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let plan = match self.adapter.core().begin_profile_refresh() {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                let _ = reply.send(CommandReceipt::new(
                    context.request_id(),
                    CommandResult::Completed(RuntimeCommandValue::Snapshot(Box::new(
                        self.adapter.core().snapshot(),
                    ))),
                ));
                return;
            }
            Err(error) => {
                let _ = reply.send(CommandReceipt::new(
                    context.request_id(),
                    CommandResult::Failed(error),
                ));
                return;
            }
        };
        let Some(foreground) = foreground.filter(|binding| {
            binding.identity().public_key() == plan.public_key()
                && binding.generation() == self.session_generation
        }) else {
            let _ = reply.send(CommandReceipt::new(
                context.request_id(),
                CommandResult::Failed(stale_profile_binding()),
            ));
            return;
        };
        let correlation = TaskCorrelation::new(
            context.request_id(),
            plan.public_key(),
            foreground.signer(),
            plan.expected_revision(),
            self.session_generation,
        );
        let client = Arc::clone(&self.nostr);
        let relays = plan.relays().to_vec();
        let request_id = context.request_id();
        let handle = self.runtime.spawn(async move {
            let result = client
                .fetch_profile(correlation.account(), &relays, context.deadline())
                .await;
            let _ = completion_sender
                .send(ProfileCompletion { request_id, result })
                .await;
        });
        let previous = self.profile_tasks.insert(
            request_id,
            PendingProfileTask {
                correlation,
                plan,
                deadline: context.deadline(),
                reply,
                handle,
            },
        );
        if let Some(previous) = previous {
            self.lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fail(request_space_exhausted());
            previous.handle.abort();
            let _ = previous.handle.await;
            let _ = previous.reply.send(CommandReceipt::new(
                previous.correlation.request_id(),
                CommandResult::Failed(request_space_exhausted()),
            ));
            self.cancel_profile_tasks(None).await;
        }
    }

    async fn close_actor(&mut self) -> CommandResult<RuntimeCommandValue> {
        let transition = (|| {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            lifecycle.begin_shutdown()?;
            lifecycle.finish_shutdown()
        })();
        match transition {
            Ok(()) => {
                self.generated_key_stage.cancel();
                self.cancel_profile_tasks(None).await;
                self.changes.close();
                *self
                    .published_foreground_session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                CommandResult::Completed(RuntimeCommandValue::Closed)
            }
            Err(error) => CommandResult::Failed(error),
        }
    }

    async fn complete_profile_task(&mut self, completion: ProfileCompletion) {
        let Some(task) = self.profile_tasks.remove(&completion.request_id) else {
            return;
        };
        let _ = task.handle.await;
        let current = self.adapter.core().snapshot();
        let foreground = self
            .published_foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let correlated = task.correlation.session_generation() == self.session_generation
            && foreground.is_some_and(|binding| {
                binding.generation() == task.correlation.session_generation()
                    && binding.identity().public_key() == task.correlation.account()
                    && binding.signer() == task.correlation.binding()
            })
            && current
                .active_account()
                .is_some_and(|active| active.account().public_key() == task.correlation.account());
        let result = if correlated {
            let plan = task.plan.clone();
            let completed = self
                .run_blocking(task.deadline, move |adapter, _, clock| {
                    adapter
                        .core()
                        .complete_profile_refresh(
                            &plan,
                            completion.result,
                            adapter.database(),
                            clock.as_ref(),
                        )
                        .map(Box::new)
                        .map(RuntimeCommandValue::Snapshot)
                })
                .await;
            completed.map_or_else(CommandResult::Failed, CommandResult::Completed)
        } else {
            CommandResult::Completed(RuntimeCommandValue::Snapshot(Box::new(current)))
        };
        if matches!(result, CommandResult::Completed(_)) {
            self.changes.publish(self.adapter.core().snapshot());
        }
        let _ = task
            .reply
            .send(CommandReceipt::new(task.correlation.request_id(), result));
    }

    async fn advance_session_generation(&mut self) {
        let Some(next) = self.session_generation.next() else {
            self.lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fail(request_space_exhausted());
            self.cancel_profile_tasks(None).await;
            return;
        };
        self.session_generation = next;
        self.published_session_generation
            .store(next.value(), Ordering::Release);
        let snapshot = self.adapter.core().snapshot();
        self.cancel_profile_tasks(Some(&snapshot)).await;
    }

    fn synchronize_foreground_session(&mut self) {
        let session = self
            .adapter
            .core()
            .snapshot()
            .active_account()
            .map(|active| {
                let public_key = active.account().public_key();
                ForegroundSessionBinding::new(
                    AccountIdentity::derive(public_key)?,
                    LocalSignerBinding::new(public_key, BindingAvailability::Available),
                    self.session_generation,
                )
            });
        let session = match session.transpose() {
            Ok(session) => session,
            Err(error) => {
                self.lifecycle
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .fail(error);
                None
            }
        };
        *self
            .published_foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = session;
    }

    async fn cancel_profile_tasks(&mut self, snapshot: Option<&AppSnapshot>) {
        let tasks = std::mem::take(&mut self.profile_tasks);
        for (_, task) in tasks {
            task.handle.abort();
            let _ = task.handle.await;
            let receipt_result = snapshot.map_or(CommandResult::Closed, |snapshot| {
                CommandResult::Completed(RuntimeCommandValue::Snapshot(Box::new(snapshot.clone())))
            });
            let _ = task.reply.send(CommandReceipt::new(
                task.correlation.request_id(),
                receipt_result,
            ));
        }
    }
}

fn commit_generated_key_stage(
    adapter: &PersistentAppCore,
    secrets: &dyn SecretStore,
    clock: &dyn Clock,
    request: &DurableRequestId,
    staged: StagedGeneratedKey,
) -> Result<RuntimeCommandValue, SafeError> {
    adapter.commit_staged_generated_key(request, staged, secrets, clock)?;
    Ok(RuntimeCommandValue::Snapshot(Box::new(
        adapter.core().snapshot(),
    )))
}

const fn blocking_execution_failed(error: BlockingExecutionError) -> SafeError {
    match error {
        BlockingExecutionError::DeadlineElapsed => command_timed_out(),
        BlockingExecutionError::Saturated => command_rejected(),
        BlockingExecutionError::TaskFailed => SafeError::new(
            SafeErrorCode::InvalidApplicationState,
            SafeMessage::new("The runtime blocking worker failed."),
        ),
    }
}

const fn request_space_exhausted() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The runtime request identifier space is exhausted."),
    )
}

const fn stale_profile_binding() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The active account binding changed before profile refresh."),
    )
}

const fn command_conflicted() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The command conflicts with newer application state."),
    )
}

const fn command_rejected() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The runtime is busy. Try again."),
    )
}

const fn command_timed_out() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The runtime command timed out."),
    )
}

const fn runtime_closed() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The application runtime is closed."),
    )
}

const fn command_unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The command is unavailable in the current runtime state."),
    )
}

const fn generated_recovery_route_active() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("Complete or cancel generated-key recovery before another action."),
    )
}

const fn invalid_actor_response() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The runtime returned an invalid command response."),
    )
}

const fn observer_registration_failed() -> SafeError {
    SafeError::new(
        SafeErrorCode::ObserverRegistrationFailed,
        SafeMessage::new("The application change subscription could not be registered."),
    )
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread::{self, Thread};
    use std::time::{Duration, Instant};

    use radroots_studio_application::{
        BoxFuture, Clock, DurableRequestId, FailureSecretStore, ForegroundSessionBinding,
        InMemorySecretStore, NostrClient, ProfileFetchResult, RelayConfiguration, RuntimeLifecycle,
        SecretStore, SecretStoreOperation, SessionGeneration, SessionState, SnapshotRevision,
    };
    use radroots_studio_domain::{
        AccountIdentity, BindingAvailability, LocalSignerBinding, PublicKey,
        RelayDestinationPolicy, RelayUrl, SafeError, SafeErrorCode, SecretKeyInput, UnixTimestamp,
    };

    use super::{
        DEFAULT_COMMAND_TIMEOUT, RuntimeActorHandle, RuntimeDependencies, command_unavailable,
    };
    use crate::{InstallationIdentity, InstallationIdentitySource, UuidInstallationIdentitySource};

    struct FixedInstallationIdentity(&'static str);

    impl InstallationIdentitySource for FixedInstallationIdentity {
        fn generate(&self) -> Result<InstallationIdentity, SafeError> {
            InstallationIdentity::parse(self.0)
        }
    }

    fn dependencies(
        secrets: Arc<dyn SecretStore>,
        nostr: Arc<dyn NostrClient>,
    ) -> RuntimeDependencies {
        RuntimeDependencies::new(
            secrets,
            Arc::new(FixedClock),
            nostr,
            Arc::new(UuidInstallationIdentitySource),
        )
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(50).expect("time")
        }
    }

    struct OfflineNostr;

    impl NostrClient for OfflineNostr {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
            _deadline: Instant,
        ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>> {
            Box::pin(async { Ok(ProfileFetchResult::complete(None)) })
        }
    }

    struct BlockingNostr {
        started: tokio::sync::Semaphore,
        release: tokio::sync::Semaphore,
    }

    impl BlockingNostr {
        fn new() -> Self {
            Self {
                started: tokio::sync::Semaphore::new(0),
                release: tokio::sync::Semaphore::new(0),
            }
        }
    }

    impl NostrClient for BlockingNostr {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
            _deadline: Instant,
        ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>> {
            Box::pin(async move {
                self.started.add_permits(1);
                let permit = self.release.acquire().await.expect("release");
                permit.forget();
                Ok(ProfileFetchResult::complete(None))
            })
        }
    }

    struct BlockingSecretStore {
        inner: InMemorySecretStore,
        block_next_put: AtomicBool,
        put_started: AtomicBool,
        released: Mutex<bool>,
        release_signal: Condvar,
    }

    impl BlockingSecretStore {
        fn new() -> Self {
            Self {
                inner: InMemorySecretStore::default(),
                block_next_put: AtomicBool::new(true),
                put_started: AtomicBool::new(false),
                released: Mutex::new(false),
                release_signal: Condvar::new(),
            }
        }

        async fn wait_until_put_started(&self) {
            while !self.put_started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        }

        fn release(&self) {
            *self
                .released
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
            self.release_signal.notify_all();
        }
    }

    impl SecretStore for BlockingSecretStore {
        fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError> {
            if self.block_next_put.swap(false, Ordering::AcqRel) {
                self.put_started.store(true, Ordering::Release);
                let released = self
                    .released
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                drop(
                    self.release_signal
                        .wait_while(released, |released| !*released)
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
            }
            self.inner.put(public_key, secret)
        }

        fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError> {
            self.inner.load(public_key)
        }

        fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError> {
            self.inner.contains(public_key)
        }

        fn delete(&self, public_key: PublicKey) -> Result<(), SafeError> {
            self.inner.delete(public_key)
        }
    }

    async fn actor() -> (RuntimeActorHandle, Arc<InMemorySecretStore>) {
        let secrets = Arc::new(InMemorySecretStore::default());
        let secret_port: Arc<dyn SecretStore> = secrets.clone();
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            dependencies(secret_port, Arc::new(OfflineNostr)),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        (actor, secrets)
    }

    struct ThreadWake(Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on_without_runtime<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park(),
            }
        }
    }

    #[test]
    fn actor_operations_support_foreign_executor_polling() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (actor, _) = runtime.block_on(actor());

        let snapshot = block_on_without_runtime(actor.bootstrap()).expect("bootstrap");
        assert_eq!(snapshot, actor.snapshot());
        block_on_without_runtime(actor.close()).expect("close");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn installation_identity_survives_file_backed_runtime_restart() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let first = RuntimeActorHandle::open(
            &path,
            RelayConfiguration::default(),
            RuntimeDependencies::new(
                Arc::new(InMemorySecretStore::default()),
                Arc::new(FixedClock),
                Arc::new(OfflineNostr),
                Arc::new(FixedInstallationIdentity(
                    "11aabbccddeeff001122334455667788",
                )),
            ),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("first runtime");
        assert_eq!(
            first.installation_identity().as_str(),
            "11aabbccddeeff001122334455667788"
        );
        first.close().await.expect("first close");
        drop(first);

        let second = RuntimeActorHandle::open(
            &path,
            RelayConfiguration::default(),
            RuntimeDependencies::new(
                Arc::new(InMemorySecretStore::default()),
                Arc::new(FixedClock),
                Arc::new(OfflineNostr),
                Arc::new(FixedInstallationIdentity(
                    "22aabbccddeeff001122334455667788",
                )),
            ),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("second runtime");
        assert_eq!(
            second.installation_identity().as_str(),
            "11aabbccddeeff001122334455667788"
        );
        second.close().await.expect("second close");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn account_mutations_run_serially_through_one_ready_actor() {
        let (actor, secrets) = actor().await;
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Ready);

        let imported = actor
            .import_secret_key_test(
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("input"),
            )
            .await
            .expect("import");
        let public_key = imported.account().public_key();
        let activated = actor.activate_account(public_key).await.expect("activate");
        assert_eq!(activated.session(), SessionState::Active);
        let foreground = actor.foreground_session().expect("foreground session");
        assert_eq!(foreground.identity().public_key(), public_key);
        assert_eq!(foreground.signer().account(), public_key);
        assert_eq!(foreground.generation(), actor.session_generation());
        assert!(secrets.contains(public_key).expect("credential"));

        let signed_out = actor.sign_out().await.expect("sign out");
        assert_eq!(signed_out.session(), SessionState::SignedOut);
        assert!(actor.foreground_session().is_none());
        let removal = actor
            .request_account_removal(public_key)
            .await
            .expect("removal request");
        let removed = actor
            .confirm_account_removal_test(removal)
            .await
            .expect("remove");
        assert!(removed.accounts().is_empty());
        assert!(!secrets.contains(public_key).expect("credential removed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn public_actor_commands_cover_generation_selection_and_empty_profile_refresh() {
        let (actor, _) = actor().await;
        let unchanged = actor
            .refresh_active_profile()
            .await
            .expect("refresh without an active account");
        assert!(unchanged.active_account().is_none());

        let generated = actor
            .generate_account(
                DurableRequestId::parse("test:generate:public-surface").expect("request"),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .expect("generate account");
        let selected = actor
            .select_account(generated.account().public_key())
            .await
            .expect("select generated account");
        assert_eq!(
            selected.selected_account(),
            Some(generated.account().public_key())
        );

        let missing =
            PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
                .expect("public key");
        let error = match actor.request_account_removal(missing).await {
            Ok(_) => panic!("unknown account removal must fail"),
            Err(error) => error,
        };
        assert_eq!(error.code(), SafeErrorCode::AccountNotFound);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn staged_recovery_rejects_a_stale_expected_revision_before_commit() {
        let (actor, _) = actor().await;
        let handle = actor
            .begin_generated_key_stage()
            .await
            .expect("generated key stage");
        let stale = SnapshotRevision::from_value(actor.snapshot().revision().value() + 1);
        let error = actor
            .acknowledge_generated_key_stage(
                handle.id(),
                DurableRequestId::parse("test:generate:stale-revision").expect("request"),
                stale,
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .expect_err("stale revision must conflict");
        assert_eq!(
            error.message().as_str(),
            "The command conflicts with newer application state."
        );
        assert!(actor.cancel_generated_key_stage().await.expect("cancel"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fatal_lifecycle_rejects_commands_before_execution() {
        let (actor, _) = actor().await;
        actor
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail(command_unavailable());

        let error = actor
            .bootstrap()
            .await
            .expect_err("fatal lifecycle must reject command admission");
        assert_eq!(
            error.message().as_str(),
            "The command is unavailable in the current runtime state."
        );
        assert!(matches!(actor.lifecycle(), RuntimeLifecycle::Fatal(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generated_key_stage_is_exclusive_cancelable_and_snapshot_free() {
        let (actor, secrets) = actor().await;
        let initial = actor.snapshot();
        let stage = actor
            .begin_generated_key_stage()
            .await
            .expect("generated key stage");

        assert!(actor.begin_generated_key_stage().await.is_err());
        assert_eq!(actor.snapshot(), initial);
        assert!(
            !secrets
                .contains(stage.view().account().public_key())
                .expect("keyring")
        );
        assert!(actor.sign_out().await.is_err());
        assert_eq!(actor.snapshot(), initial);
        assert!(actor.cancel_generated_key_stage().await.expect("cancel"));
        assert!(
            !actor
                .cancel_generated_key_stage()
                .await
                .expect("cancel empty")
        );
        assert_eq!(actor.snapshot(), initial);

        actor
            .begin_generated_key_stage()
            .await
            .expect("replacement stage");
        actor.close().await.expect("close clears stage");
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Closed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn recovery_handle_is_one_use_and_acknowledgement_commits_once() {
        let (actor, secrets) = actor().await;
        let initial = actor.snapshot();
        let handle = actor
            .begin_generated_key_stage()
            .await
            .expect("generated key stage");
        let public_key = handle.view().account().public_key();
        let recovery = handle.take_recovery_nsec().expect("recovery material");
        assert_eq!(recovery.with_exposed_secret(str::len), 63);
        assert!(handle.take_recovery_nsec().is_err());
        assert_eq!(actor.snapshot(), initial);
        assert!(!secrets.contains(public_key).expect("not committed"));

        let committed = actor
            .acknowledge_generated_key_stage_test(handle.id())
            .await
            .expect("acknowledge");
        assert_eq!(committed.accounts().len(), 1);
        assert_eq!(committed.selected_account(), Some(public_key));
        assert!(secrets.contains(public_key).expect("credential committed"));
        assert!(
            actor
                .acknowledge_generated_key_stage_test(handle.id())
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_generated_commit_consumes_the_stage_without_poisoning_the_actor() {
        let secrets = Arc::new(FailureSecretStore::default());
        secrets.fail_next(SecretStoreOperation::Put);
        let secret_port: Arc<dyn SecretStore> = secrets.clone();
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            dependencies(secret_port, Arc::new(OfflineNostr)),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        let handle = actor
            .begin_generated_key_stage()
            .await
            .expect("generated key stage");

        let error = actor
            .acknowledge_generated_key_stage_test(handle.id())
            .await
            .expect_err("injected keyring failure");

        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(actor.snapshot().accounts().is_empty());
        actor
            .begin_generated_key_stage()
            .await
            .expect("fresh recovery after terminal failure");
        assert!(actor.cancel_generated_key_stage().await.expect("cancel"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn session_generation_cancels_correlated_profile_work_on_sign_out() {
        let client = Arc::new(BlockingNostr::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::new(vec![
                RelayUrl::parse("ws://localhost:8080", RelayDestinationPolicy::Local)
                    .expect("relay"),
            ])
            .expect("relay configuration"),
            dependencies(Arc::new(InMemorySecretStore::default()), client.clone()),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        let imported = actor
            .import_secret_key_test(
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("input"),
            )
            .await
            .expect("import");
        actor
            .activate_account(imported.account().public_key())
            .await
            .expect("activate");
        assert_eq!(actor.session_generation().value(), 1);

        let refresh_actor = actor.clone();
        let refresh = tokio::spawn(async move { refresh_actor.refresh_active_profile().await });
        let started = client.started.acquire().await.expect("refresh started");
        started.forget();
        let signed_out = actor.sign_out().await.expect("sign out");
        let cancelled = refresh
            .await
            .expect("refresh task")
            .expect("safe cancellation");

        assert_eq!(actor.session_generation().value(), 2);
        assert_eq!(signed_out.session(), SessionState::SignedOut);
        assert_eq!(cancelled.session(), SessionState::SignedOut);
        assert!(cancelled.active_account().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn profile_refresh_rejects_stale_bindings_and_discards_stale_completions() {
        let client = Arc::new(BlockingNostr::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::new(vec![
                RelayUrl::parse("ws://localhost:8080", RelayDestinationPolicy::Local)
                    .expect("relay"),
            ])
            .expect("relay configuration"),
            dependencies(Arc::new(InMemorySecretStore::default()), client.clone()),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        let imported = actor
            .import_secret_key_test(secret(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
            ))
            .await
            .expect("import");
        let public_key = imported.account().public_key();
        actor.activate_account(public_key).await.expect("activate");
        let binding = actor.foreground_session().expect("foreground binding");
        let stale_binding = ForegroundSessionBinding::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            SessionGeneration::from_value(binding.generation().value() + 1),
        )
        .expect("stale binding fixture");
        let other_public_key =
            PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
                .expect("other public key");
        let other_binding = ForegroundSessionBinding::new(
            AccountIdentity::derive(other_public_key).expect("other identity"),
            LocalSignerBinding::new(other_public_key, BindingAvailability::Available),
            binding.generation(),
        )
        .expect("other binding fixture");

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stale_binding.clone());
        let error = actor
            .refresh_active_profile()
            .await
            .expect_err("stale generation must reject before relay work");
        assert_eq!(
            error.message().as_str(),
            "The active account binding changed before profile refresh."
        );

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(other_binding.clone());
        let error = actor
            .refresh_active_profile()
            .await
            .expect_err("different account binding must reject before relay work");
        assert_eq!(
            error.message().as_str(),
            "The active account binding changed before profile refresh."
        );

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(binding.clone());
        let refresh_actor = actor.clone();
        let refresh = tokio::spawn(async move { refresh_actor.refresh_active_profile().await });
        let started = client.started.acquire().await.expect("refresh started");
        started.forget();
        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stale_binding);
        client.release.add_permits(1);
        let unchanged = refresh
            .await
            .expect("refresh task")
            .expect("stale completion returns current snapshot");
        assert_eq!(unchanged.revision(), actor.snapshot().revision());

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(binding.clone());
        let refresh_actor = actor.clone();
        let refresh = tokio::spawn(async move { refresh_actor.refresh_active_profile().await });
        let started = client
            .started
            .acquire()
            .await
            .expect("second refresh started");
        started.forget();
        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        client.release.add_permits(1);
        let unchanged = refresh
            .await
            .expect("refresh task")
            .expect("missing binding returns current snapshot");
        assert_eq!(unchanged.revision(), actor.snapshot().revision());

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(binding.clone());
        let refresh_actor = actor.clone();
        let refresh = tokio::spawn(async move { refresh_actor.refresh_active_profile().await });
        let started = client
            .started
            .acquire()
            .await
            .expect("third refresh started");
        started.forget();
        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(other_binding);
        client.release.add_permits(1);
        let unchanged = refresh
            .await
            .expect("refresh task")
            .expect("different account binding returns current snapshot");
        assert_eq!(unchanged.revision(), actor.snapshot().revision());

        *actor
            .foreground_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(binding);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn bounded_runtime_rejects_saturation_without_dropping_accepted_commands() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            dependencies(secrets.clone(), Arc::new(OfflineNostr)),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");

        let first_actor = actor.clone();
        let first = tokio::spawn(async move {
            first_actor
                .import_secret_key_test(secret(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
                ))
                .await
        });
        secrets.wait_until_put_started().await;

        let second_actor = actor.clone();
        let second = tokio::spawn(async move {
            second_actor
                .import_secret_key_test(secret(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                ))
                .await
        });
        while actor.mailbox.available_capacity() != 0 {
            assert!(
                !second.is_finished(),
                "second command must enter the mailbox"
            );
            tokio::task::yield_now().await;
        }
        let rejected = actor
            .import_secret_key_test(secret(
                "0000000000000000000000000000000000000000000000000000000000000002",
            ))
            .await
            .expect_err("full mailbox must reject");
        assert_eq!(
            rejected.message().as_str(),
            "The runtime is busy. Try again."
        );

        secrets.release();
        first.await.expect("first task").expect("first command");
        let second = second
            .await
            .expect("second task")
            .expect_err("accepted stale revision conflicts explicitly");
        assert_eq!(
            second.message().as_str(),
            "The account operation conflicts with the current application state."
        );
        assert_eq!(
            actor.bootstrap().await.expect("snapshot").accounts().len(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn queued_command_expiry_returns_timeout_and_prevents_late_mutation() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            dependencies(secrets.clone(), Arc::new(OfflineNostr)),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");

        let first_actor = actor.clone();
        let first = tokio::spawn(async move {
            first_actor
                .import_secret_key_test(secret(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
                ))
                .await
        });
        secrets.wait_until_put_started().await;

        let expired = actor
            .import_secret_key_with_timeout(
                secret("0000000000000000000000000000000000000000000000000000000000000001"),
                Duration::from_millis(10),
            )
            .await
            .expect_err("queued command must time out");
        assert_eq!(expired.message().as_str(), "The runtime command timed out.");

        secrets.release();
        first.await.expect("first task").expect("first command");
        assert_eq!(
            actor.bootstrap().await.expect("snapshot").accounts().len(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_is_terminal_and_every_later_command_is_rejected_as_closed() {
        let (actor, _) = actor().await;
        actor.close().await.expect("close");
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Closed);

        let error = actor.bootstrap().await.expect_err("bootstrap after close");
        assert_eq!(
            error.message().as_str(),
            "The application runtime is closed."
        );
        actor.close().await.expect("repeated close is idempotent");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn actor_subscription_atomically_delivers_initial_then_ordered_changes() {
        let (actor, _) = actor().await;
        let mut subscription = actor
            .subscribe_changes(NonZeroUsize::new(4).expect("capacity"))
            .await
            .expect("subscribe");
        let initial = subscription.receive().await.expect("initial snapshot");
        assert_eq!(initial.revision(), actor.snapshot().revision());
        assert!(initial.previous_revision().is_none());

        actor
            .import_secret_key_test(secret(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
            ))
            .await
            .expect("import");
        let changed = subscription.receive().await.expect("change");
        assert!(changed.revision() > initial.revision());
        assert_eq!(changed.previous_revision(), Some(initial.revision()));
        assert!(
            actor
                .unsubscribe_changes(subscription.id())
                .await
                .expect("unsubscribe")
        );
        assert!(
            !actor
                .unsubscribe_changes(subscription.id())
                .await
                .expect("second unsubscribe")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn expired_queued_shutdown_does_not_close_runtime_later() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            dependencies(secrets.clone(), Arc::new(OfflineNostr)),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        let import_actor = actor.clone();
        let import = tokio::spawn(async move {
            import_actor
                .import_secret_key_test(secret(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
                ))
                .await
        });
        secrets.wait_until_put_started().await;

        let timeout = actor
            .close_with_timeout(Duration::from_millis(10))
            .await
            .expect_err("queued shutdown must expire");
        assert_eq!(timeout.message().as_str(), "The runtime command timed out.");
        secrets.release();
        import.await.expect("import task").expect("import");
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Ready);
        assert_eq!(
            actor
                .bootstrap()
                .await
                .expect("still open")
                .accounts()
                .len(),
            1
        );
        actor.close().await.expect("later close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn shutdown_cancels_in_flight_work_and_terminates_publication() {
        let client = Arc::new(BlockingNostr::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::new(vec![
                RelayUrl::parse("ws://localhost:8080", RelayDestinationPolicy::Local)
                    .expect("relay"),
            ])
            .expect("relay configuration"),
            dependencies(Arc::new(InMemorySecretStore::default()), client.clone()),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .await
        .expect("actor");
        let imported = actor
            .import_secret_key_test(secret(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
            ))
            .await
            .expect("import");
        actor
            .activate_account(imported.account().public_key())
            .await
            .expect("activate");
        let mut changes = actor
            .subscribe_changes(NonZeroUsize::new(4).expect("capacity"))
            .await
            .expect("subscribe");
        changes.receive().await.expect("initial");

        let refresh_actor = actor.clone();
        let refresh = tokio::spawn(async move { refresh_actor.refresh_active_profile().await });
        let started = client.started.acquire().await.expect("refresh started");
        started.forget();
        actor.close().await.expect("close");

        let cancelled = refresh
            .await
            .expect("refresh task")
            .expect_err("refresh closes");
        assert_eq!(
            cancelled.message().as_str(),
            "The application runtime is closed."
        );
        assert!(changes.receive().await.is_none());
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Closed);
    }

    fn secret(value: &str) -> SecretKeyInput {
        SecretKeyInput::parse(value.to_owned()).expect("valid test secret")
    }
}
