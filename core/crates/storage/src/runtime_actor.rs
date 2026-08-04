use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radroots_studio_application::{
    ActorMailbox, AppSnapshot, ChangeSubscriptionId, Clock, CommandContext, CommandEnvelope,
    CommandReceipt, CommandResult, CommandSubmission, ForegroundSessionBinding,
    GenerateAccountReceipt, GeneratedKeyRecoveryHandle, GeneratedKeyStage, ImportAccountReceipt,
    LifecycleGate, NostrClient, OrderedSnapshotChanges, ProfileRefreshPlan, RecoveryStageId,
    RelayConfiguration, RemovalConfirmationToken, RequestId, RuntimeCommandClass, RuntimeLifecycle,
    SecretStore, SessionGeneration, SnapshotChange, SnapshotChangeReceiver, SnapshotRevision,
    TaskCorrelation,
};
use radroots_studio_domain::{
    AccountIdentity, BindingAvailability, Kind0ProfileCandidate, LocalSignerBinding, PublicKey,
    SafeError, SafeErrorCode, SafeMessage, SecretKeyInput,
};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use crate::PersistentAppCore;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TASK_CAPACITY: usize = 64;

enum RuntimeCommand {
    Snapshot,
    GenerateAccount,
    BeginGeneratedKeyStage,
    AcknowledgeGeneratedKeyStage(RecoveryStageId),
    CancelGeneratedKeyStage,
    ImportSecretKey {
        input: SecretKeyInput,
        durable_request: Option<radroots_studio_application::DurableRequestId>,
        durable_expected_revision: Option<u64>,
    },
    SelectAccount(PublicKey),
    ActivateAccount(PublicKey),
    SignOut,
    RefreshActiveProfile,
    RequestAccountRemoval(PublicKey),
    ConfirmAccountRemoval(RemovalConfirmationToken),
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
            Self::GenerateAccount
            | Self::BeginGeneratedKeyStage
            | Self::AcknowledgeGeneratedKeyStage(_)
            | Self::ImportSecretKey { .. }
            | Self::ActivateAccount(_)
            | Self::ConfirmAccountRemoval(_) => RuntimeCommandClass::UseCredential,
            Self::SelectAccount(_)
            | Self::SignOut
            | Self::RequestAccountRemoval(_)
            | Self::CancelGeneratedKeyStage => RuntimeCommandClass::MutateLocalState,
            Self::RefreshActiveProfile => RuntimeCommandClass::UseRelay,
            Self::Close => RuntimeCommandClass::Shutdown,
        }
    }
}

struct RuntimeActor {
    adapter: Arc<PersistentAppCore>,
    secrets: Arc<dyn SecretStore>,
    clock: Arc<dyn Clock>,
    nostr: Arc<dyn NostrClient>,
    lifecycle: Arc<Mutex<LifecycleGate>>,
    runtime: Handle,
    session_generation: SessionGeneration,
    published_session_generation: Arc<AtomicU64>,
    profile_tasks: BTreeMap<RequestId, PendingProfileTask>,
    changes: OrderedSnapshotChanges,
    published_foreground_session: Arc<Mutex<Option<ForegroundSessionBinding>>>,
    durable_request_namespace: String,
    generated_key_stage: GeneratedKeyStage,
}

struct PendingProfileTask {
    correlation: TaskCorrelation,
    plan: ProfileRefreshPlan,
    reply: oneshot::Sender<CommandReceipt<RuntimeCommandValue>>,
    handle: tokio::task::JoinHandle<()>,
}

struct ProfileCompletion {
    request_id: RequestId,
    result: Result<Option<Kind0ProfileCandidate>, SafeError>,
}

#[derive(Clone)]
pub struct RuntimeActorHandle {
    mailbox: ActorMailbox<RuntimeCommand, RuntimeCommandValue>,
    adapter: Arc<PersistentAppCore>,
    lifecycle: Arc<Mutex<LifecycleGate>>,
    runtime: Handle,
    next_request: Arc<AtomicU64>,
    session_generation: Arc<AtomicU64>,
    foreground_session: Arc<Mutex<Option<ForegroundSessionBinding>>>,
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
    pub fn open(
        path: &Path,
        relay_configuration: RelayConfiguration,
        secrets: Arc<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        nostr: Arc<dyn NostrClient>,
        capacity: NonZeroUsize,
        runtime: &Handle,
    ) -> Result<Self, SafeError> {
        Self::start(
            PersistentAppCore::open(path, relay_configuration)?,
            secrets,
            clock,
            nostr,
            capacity,
            runtime,
        )
    }

    /// Starts one isolated actor-owned in-memory runtime for tests.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or lifecycle error before publication.
    pub fn in_memory(
        relay_configuration: RelayConfiguration,
        secrets: Arc<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        nostr: Arc<dyn NostrClient>,
        capacity: NonZeroUsize,
        runtime: &Handle,
    ) -> Result<Self, SafeError> {
        Self::start(
            PersistentAppCore::in_memory(relay_configuration)?,
            secrets,
            clock,
            nostr,
            capacity,
            runtime,
        )
    }

    fn start(
        adapter: PersistentAppCore,
        secrets: Arc<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        nostr: Arc<dyn NostrClient>,
        capacity: NonZeroUsize,
        runtime: &Handle,
    ) -> Result<Self, SafeError> {
        let mut gate = LifecycleGate::opening();
        gate.begin_compatibility_check()?;
        gate.compatibility_accepted()?;
        gate.ownership_acquired()?;
        gate.migration_complete()?;
        adapter.bootstrap(secrets.as_ref(), clock.as_ref())?;
        gate.recovery_complete()?;

        let adapter = Arc::new(adapter);
        let lifecycle = Arc::new(Mutex::new(gate));
        let (mailbox, receiver) = ActorMailbox::bounded(capacity);
        let session_generation = Arc::new(AtomicU64::new(SessionGeneration::initial().value()));
        let foreground_session = Arc::new(Mutex::new(None));
        let changes = OrderedSnapshotChanges::new(adapter.core().snapshot());
        let durable_request_namespace = format!(
            "runtime:{}:{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        );
        let actor = RuntimeActor {
            adapter: Arc::clone(&adapter),
            secrets,
            clock,
            nostr,
            lifecycle: Arc::clone(&lifecycle),
            runtime: runtime.clone(),
            session_generation: SessionGeneration::initial(),
            published_session_generation: Arc::clone(&session_generation),
            profile_tasks: BTreeMap::new(),
            changes,
            published_foreground_session: Arc::clone(&foreground_session),
            durable_request_namespace,
            generated_key_stage: GeneratedKeyStage::default(),
        };
        drop(runtime.spawn(actor.run(receiver)));
        Ok(Self {
            mailbox,
            adapter,
            lifecycle,
            runtime: runtime.clone(),
            next_request: Arc::new(AtomicU64::new(1)),
            session_generation,
            foreground_session,
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
    pub async fn generate_account(&self) -> Result<GenerateAccountReceipt, SafeError> {
        match self.dispatch(RuntimeCommand::GenerateAccount, None).await? {
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
    ) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch(RuntimeCommand::AcknowledgeGeneratedKeyStage(id), None)
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
        input: SecretKeyInput,
    ) -> Result<ImportAccountReceipt, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::ImportSecretKey {
                    input,
                    durable_request: None,
                    durable_expected_revision: None,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::Imported(receipt) => Ok(receipt),
            _ => Err(invalid_actor_response()),
        }
    }

    /// Imports or repairs with a caller-owned durable request and deadline.
    ///
    /// # Errors
    ///
    /// Returns a safe validation, conflict, timeout, persistence, or actor error.
    pub async fn import_secret_key_request(
        &self,
        request: radroots_studio_application::DurableRequestId,
        expected_revision: SnapshotRevision,
        input: SecretKeyInput,
        timeout: Duration,
    ) -> Result<ImportAccountReceipt, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        match self
            .dispatch_with_deadline(
                RuntimeCommand::ImportSecretKey {
                    input,
                    durable_request: Some(request),
                    durable_expected_revision: Some(expected_revision.value()),
                },
                None,
                request_id,
                Instant::now() + timeout,
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
    ) -> Result<AppSnapshot, SafeError> {
        let value = self
            .dispatch(RuntimeCommand::ConfirmAccountRemoval(token), None)
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
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        match self
            .dispatch_with_deadline(
                RuntimeCommand::Close,
                None,
                request_id,
                Instant::now() + timeout,
            )
            .await?
        {
            RuntimeCommandValue::Closed => Ok(()),
            _ => Err(invalid_actor_response()),
        }
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
                let waiting = self
                    .runtime
                    .spawn(async move { tokio::time::timeout(remaining, ticket.receipt()).await });
                match waiting.await {
                    Ok(Ok(receipt)) => receipt,
                    Ok(Err(_)) => CommandReceipt::new(request_id, CommandResult::TimedOut),
                    Err(_) => CommandReceipt::new(request_id, CommandResult::Closed),
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

    #[cfg(test)]
    async fn import_secret_key_with_timeout(
        &self,
        input: SecretKeyInput,
        timeout: Duration,
    ) -> Result<ImportAccountReceipt, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        match self
            .dispatch_with_deadline(
                RuntimeCommand::ImportSecretKey {
                    input,
                    durable_request: None,
                    durable_expected_revision: None,
                },
                None,
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
                    if !self.handle_command(envelope, &completion_sender) {
                        break;
                    }
                }
                completion = completions.recv(), if !self.profile_tasks.is_empty() => {
                    if let Some(completion) = completion {
                        self.complete_profile_task(completion);
                    }
                }
            }
        }
        self.cancel_profile_tasks(None);
    }

    fn handle_command(
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
            self.start_profile_task(context, reply, completion_sender.clone());
            return true;
        }
        if matches!(command, RuntimeCommand::Close) {
            let result = self.close_actor();
            let closed = matches!(result, CommandResult::Completed(_));
            let _ = reply.send(CommandReceipt::new(context.request_id(), result));
            return !closed;
        }
        let changes_session = matches!(
            command,
            RuntimeCommand::ActivateAccount(_)
                | RuntimeCommand::SignOut
                | RuntimeCommand::ConfirmAccountRemoval(_)
        );
        let result = self.execute_sync(context, command);
        if changes_session && matches!(result, CommandResult::Completed(_)) {
            self.advance_session_generation();
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
        let current_revision = self.adapter.core().snapshot().revision();
        if context
            .expected_revision()
            .is_some_and(|expected| expected != current_revision)
        {
            return Some(CommandResult::Conflicted { current_revision });
        }
        None
    }

    fn execute_sync(
        &mut self,
        context: CommandContext,
        command: RuntimeCommand,
    ) -> CommandResult<RuntimeCommandValue> {
        let durable_request = radroots_studio_application::DurableRequestId::parse(format!(
            "{}:{}",
            self.durable_request_namespace,
            context.request_id().get()
        ));
        let expected_revision = context
            .expected_revision()
            .unwrap_or_else(|| self.adapter.core().snapshot().revision())
            .value();
        let result = match command {
            RuntimeCommand::Snapshot => Ok(RuntimeCommandValue::Snapshot(Box::new(
                self.adapter.core().snapshot(),
            ))),
            RuntimeCommand::GenerateAccount => durable_request.and_then(|request| {
                self.adapter
                    .generate_account_durable(
                        &request,
                        expected_revision,
                        self.secrets.as_ref(),
                        self.clock.as_ref(),
                    )
                    .map(RuntimeCommandValue::Generated)
            }),
            RuntimeCommand::BeginGeneratedKeyStage => self
                .generated_key_stage
                .begin(
                    RecoveryStageId::new(
                        NonZeroU64::new(context.request_id().get())
                            .expect("request IDs are always non-zero"),
                    ),
                    expected_revision,
                    self.clock.now(),
                )
                .map(RuntimeCommandValue::GeneratedKeyStage),
            RuntimeCommand::AcknowledgeGeneratedKeyStage(id) => {
                durable_request.and_then(|request| self.commit_generated_key_stage(&request, id))
            }
            RuntimeCommand::CancelGeneratedKeyStage => Ok(
                RuntimeCommandValue::GeneratedKeyStageCancelled(self.generated_key_stage.cancel()),
            ),
            RuntimeCommand::ImportSecretKey {
                input,
                durable_request: caller_request,
                durable_expected_revision,
            } => self.import_secret_key_command(
                input,
                caller_request,
                durable_request,
                durable_expected_revision.unwrap_or(expected_revision),
            ),
            RuntimeCommand::SelectAccount(public_key) => self
                .adapter
                .select_account(public_key)
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot),
            RuntimeCommand::ActivateAccount(public_key) => self
                .adapter
                .activate_account(public_key, self.secrets.as_ref(), self.clock.as_ref())
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot),
            RuntimeCommand::SignOut => self
                .adapter
                .sign_out()
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot),
            RuntimeCommand::RequestAccountRemoval(public_key) => self
                .adapter
                .request_account_removal(public_key, self.clock.as_ref())
                .map(RuntimeCommandValue::RemovalRequest),
            RuntimeCommand::ConfirmAccountRemoval(token) => durable_request.and_then(|request| {
                self.adapter
                    .confirm_account_removal_durable(
                        &request,
                        token,
                        self.secrets.as_ref(),
                        self.clock.as_ref(),
                    )
                    .map(Box::new)
                    .map(RuntimeCommandValue::Snapshot)
            }),
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

    fn commit_generated_key_stage(
        &mut self,
        request: &radroots_studio_application::DurableRequestId,
        id: RecoveryStageId,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let staged = self.generated_key_stage.take(id, self.clock.now())?;
        self.adapter.commit_staged_generated_key(
            request,
            staged,
            self.secrets.as_ref(),
            self.clock.as_ref(),
        )?;
        Ok(RuntimeCommandValue::Snapshot(Box::new(
            self.adapter.core().snapshot(),
        )))
    }

    fn import_secret_key_command(
        &self,
        input: SecretKeyInput,
        caller_request: Option<radroots_studio_application::DurableRequestId>,
        fallback_request: Result<radroots_studio_application::DurableRequestId, SafeError>,
        expected_revision: u64,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let request = caller_request.map_or(fallback_request, Ok)?;
        self.adapter
            .import_secret_key_durable(
                &request,
                expected_revision,
                input,
                self.secrets.as_ref(),
                self.clock.as_ref(),
            )
            .map(RuntimeCommandValue::Imported)
    }

    fn start_profile_task(
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
            let result = client.fetch_profile(correlation.account(), &relays).await;
            let _ = completion_sender
                .send(ProfileCompletion { request_id, result })
                .await;
        });
        let previous = self.profile_tasks.insert(
            request_id,
            PendingProfileTask {
                correlation,
                plan,
                reply,
                handle,
            },
        );
        debug_assert!(previous.is_none(), "request identifiers are unique");
    }

    fn close_actor(&mut self) -> CommandResult<RuntimeCommandValue> {
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
                self.cancel_profile_tasks(None);
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

    fn complete_profile_task(&mut self, completion: ProfileCompletion) {
        let Some(task) = self.profile_tasks.remove(&completion.request_id) else {
            return;
        };
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
            self.adapter
                .core()
                .complete_profile_refresh(
                    &task.plan,
                    completion.result,
                    self.adapter.database(),
                    self.clock.as_ref(),
                )
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot)
                .map_or_else(CommandResult::Failed, CommandResult::Completed)
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

    fn advance_session_generation(&mut self) {
        let Some(next) = self.session_generation.next() else {
            self.lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fail(request_space_exhausted());
            self.cancel_profile_tasks(None);
            return;
        };
        self.session_generation = next;
        self.published_session_generation
            .store(next.value(), Ordering::Release);
        let snapshot = self.adapter.core().snapshot();
        self.cancel_profile_tasks(Some(&snapshot));
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

    fn cancel_profile_tasks(&mut self, snapshot: Option<&AppSnapshot>) {
        let tasks = std::mem::take(&mut self.profile_tasks);
        for (_, task) in tasks {
            task.handle.abort();
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
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use radroots_studio_application::{
        BoxFuture, Clock, InMemorySecretStore, NostrClient, RelayConfiguration, RuntimeLifecycle,
        SecretStore, SessionState,
    };
    use radroots_studio_domain::{
        Kind0ProfileCandidate, PublicKey, RelayUrl, SafeError, SecretKeyInput, UnixTimestamp,
    };

    use super::RuntimeActorHandle;

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
        ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
            Box::pin(async { Ok(None) })
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
        ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
            Box::pin(async move {
                self.started.add_permits(1);
                let permit = self.release.acquire().await.expect("release");
                permit.forget();
                Ok(None)
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

    fn actor() -> (RuntimeActorHandle, Arc<InMemorySecretStore>) {
        let secrets = Arc::new(InMemorySecretStore::default());
        let secret_port: Arc<dyn SecretStore> = secrets.clone();
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            secret_port,
            Arc::new(FixedClock),
            Arc::new(OfflineNostr),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");
        (actor, secrets)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn account_mutations_run_serially_through_one_ready_actor() {
        let (actor, secrets) = actor();
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Ready);

        let imported = actor
            .import_secret_key(
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
            .confirm_account_removal(removal)
            .await
            .expect("remove");
        assert!(removed.accounts().is_empty());
        assert!(!secrets.contains(public_key).expect("credential removed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generated_key_stage_is_exclusive_cancelable_and_snapshot_free() {
        let (actor, secrets) = actor();
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
        let (actor, secrets) = actor();
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
            .acknowledge_generated_key_stage(handle.id())
            .await
            .expect("acknowledge");
        assert_eq!(committed.accounts().len(), 1);
        assert_eq!(committed.selected_account(), Some(public_key));
        assert!(secrets.contains(public_key).expect("credential committed"));
        assert!(
            actor
                .acknowledge_generated_key_stage(handle.id())
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn session_generation_cancels_correlated_profile_work_on_sign_out() {
        let client = Arc::new(BlockingNostr::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::new(vec![RelayUrl::parse("ws://localhost:8080").expect("relay")]),
            Arc::new(InMemorySecretStore::default()),
            Arc::new(FixedClock),
            client.clone(),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");
        let imported = actor
            .import_secret_key(
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
    async fn bounded_runtime_rejects_saturation_without_dropping_accepted_commands() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            secrets.clone(),
            Arc::new(FixedClock),
            Arc::new(OfflineNostr),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");

        let first_actor = actor.clone();
        let first = tokio::spawn(async move {
            first_actor
                .import_secret_key(secret(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
                ))
                .await
        });
        secrets.wait_until_put_started().await;

        let second_actor = actor.clone();
        let second = tokio::spawn(async move {
            second_actor
                .import_secret_key(secret(
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
            .import_secret_key(secret(
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
        second.await.expect("second task").expect("second command");
        assert_eq!(
            actor.bootstrap().await.expect("snapshot").accounts().len(),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn queued_command_expiry_returns_timeout_and_prevents_late_mutation() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            secrets.clone(),
            Arc::new(FixedClock),
            Arc::new(OfflineNostr),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");

        let first_actor = actor.clone();
        let first = tokio::spawn(async move {
            first_actor
                .import_secret_key(secret(
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
        let (actor, _) = actor();
        actor.close().await.expect("close");
        assert_eq!(actor.lifecycle(), RuntimeLifecycle::Closed);

        for error in [
            actor.bootstrap().await.expect_err("bootstrap after close"),
            actor.close().await.expect_err("repeated close"),
        ] {
            assert_eq!(
                error.message().as_str(),
                "The application runtime is closed."
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn actor_subscription_atomically_delivers_initial_then_ordered_changes() {
        let (actor, _) = actor();
        let mut subscription = actor
            .subscribe_changes(NonZeroUsize::new(4).expect("capacity"))
            .await
            .expect("subscribe");
        let initial = subscription.receive().await.expect("initial snapshot");
        assert_eq!(initial.revision(), actor.snapshot().revision());
        assert!(initial.previous_revision().is_none());

        actor
            .import_secret_key(secret(
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
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn expired_queued_shutdown_does_not_close_runtime_later() {
        let secrets = Arc::new(BlockingSecretStore::new());
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            secrets.clone(),
            Arc::new(FixedClock),
            Arc::new(OfflineNostr),
            NonZeroUsize::new(1).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");
        let import_actor = actor.clone();
        let import = tokio::spawn(async move {
            import_actor
                .import_secret_key(secret(
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
            RelayConfiguration::new(vec![RelayUrl::parse("ws://localhost:8080").expect("relay")]),
            Arc::new(InMemorySecretStore::default()),
            Arc::new(FixedClock),
            client.clone(),
            NonZeroUsize::new(8).expect("capacity"),
            &tokio::runtime::Handle::current(),
        )
        .expect("actor");
        let imported = actor
            .import_secret_key(secret(
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
