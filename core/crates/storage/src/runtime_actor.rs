use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radroots_studio_application::{
    ActorMailbox, AppObserver, AppSnapshot, Clock, CommandContext, CommandEnvelope, CommandReceipt,
    CommandResult, CommandSubmission, GenerateAccountReceipt, ImportAccountReceipt, LifecycleGate,
    NostrClient, ObserverHandle, ProfileRefreshPlan, RelayConfiguration, RemovalConfirmationToken,
    RequestId, RuntimeCommandClass, RuntimeLifecycle, SecretStore, SessionGeneration,
    SnapshotRevision, TaskCorrelation,
};
use radroots_studio_domain::{
    Kind0ProfileCandidate, PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput,
};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use crate::PersistentAppCore;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TASK_CAPACITY: usize = 64;

enum RuntimeCommand {
    Snapshot,
    GenerateAccount,
    ImportSecretKey(SecretKeyInput),
    SelectAccount(PublicKey),
    ActivateAccount(PublicKey),
    SignOut,
    RefreshActiveProfile,
    RequestAccountRemoval(PublicKey),
    ConfirmAccountRemoval(RemovalConfirmationToken),
}

enum RuntimeCommandValue {
    Snapshot(Box<AppSnapshot>),
    Generated(GenerateAccountReceipt),
    Imported(ImportAccountReceipt),
    RemovalRequest(RemovalConfirmationToken),
}

impl RuntimeCommand {
    const fn class(&self) -> RuntimeCommandClass {
        match self {
            Self::Snapshot => RuntimeCommandClass::Observe,
            Self::GenerateAccount
            | Self::ImportSecretKey(_)
            | Self::ActivateAccount(_)
            | Self::ConfirmAccountRemoval(_) => RuntimeCommandClass::UseCredential,
            Self::SelectAccount(_) | Self::SignOut | Self::RequestAccountRemoval(_) => {
                RuntimeCommandClass::MutateLocalState
            }
            Self::RefreshActiveProfile => RuntimeCommandClass::UseRelay,
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
    next_request: Arc<AtomicU64>,
    session_generation: Arc<AtomicU64>,
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
        };
        drop(runtime.spawn(actor.run(receiver)));
        Ok(Self {
            mailbox,
            adapter,
            lifecycle,
            next_request: Arc::new(AtomicU64::new(1)),
            session_generation,
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
    pub fn snapshot(&self) -> AppSnapshot {
        self.adapter.core().snapshot()
    }

    /// Registers a read-only snapshot observer.
    ///
    /// # Errors
    ///
    /// Returns a safe observer-registration error.
    pub fn subscribe(&self, observer: Arc<dyn AppObserver>) -> Result<ObserverHandle, SafeError> {
        self.adapter.core().subscribe(observer)
    }

    #[must_use]
    pub fn unsubscribe(&self, handle: ObserverHandle) -> bool {
        self.adapter.core().unsubscribe(handle)
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
            .dispatch(RuntimeCommand::ImportSecretKey(input), None)
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

    async fn dispatch(
        &self,
        command: RuntimeCommand,
        expected_revision: Option<SnapshotRevision>,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let raw_request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::new(raw_request).ok_or_else(request_space_exhausted)?;
        let context = CommandContext::new(
            request_id,
            expected_revision,
            Instant::now() + DEFAULT_COMMAND_TIMEOUT,
        );
        let receipt = match self.mailbox.submit(context, command) {
            CommandSubmission::Accepted(ticket) => ticket.receipt().await,
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
                    self.handle_command(envelope, &completion_sender);
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
    ) {
        let (context, command, reply) = envelope.into_parts();
        if let Some(result) = self.preflight(context, &command) {
            let _ = reply.send(CommandReceipt::new(context.request_id(), result));
            return;
        }
        if matches!(command, RuntimeCommand::RefreshActiveProfile) {
            self.start_profile_task(context, reply, completion_sender.clone());
            return;
        }
        let changes_session = matches!(
            command,
            RuntimeCommand::ActivateAccount(_)
                | RuntimeCommand::SignOut
                | RuntimeCommand::ConfirmAccountRemoval(_)
        );
        let result = self.execute_sync(command);
        if changes_session && matches!(result, CommandResult::Completed(_)) {
            self.advance_session_generation();
        }
        let _ = reply.send(CommandReceipt::new(context.request_id(), result));
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

    fn execute_sync(&self, command: RuntimeCommand) -> CommandResult<RuntimeCommandValue> {
        let result = match command {
            RuntimeCommand::Snapshot => Ok(RuntimeCommandValue::Snapshot(Box::new(
                self.adapter.core().snapshot(),
            ))),
            RuntimeCommand::GenerateAccount => self
                .adapter
                .generate_account(self.secrets.as_ref(), self.clock.as_ref())
                .map(RuntimeCommandValue::Generated),
            RuntimeCommand::ImportSecretKey(input) => self
                .adapter
                .import_secret_key(input, self.secrets.as_ref(), self.clock.as_ref())
                .map(RuntimeCommandValue::Imported),
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
            RuntimeCommand::RefreshActiveProfile => Err(invalid_actor_response()),
            RuntimeCommand::RequestAccountRemoval(public_key) => self
                .adapter
                .request_account_removal(public_key)
                .map(RuntimeCommandValue::RemovalRequest),
            RuntimeCommand::ConfirmAccountRemoval(token) => self
                .adapter
                .confirm_account_removal(token, self.secrets.as_ref(), self.clock.as_ref())
                .map(Box::new)
                .map(RuntimeCommandValue::Snapshot),
        };
        result.map_or_else(CommandResult::Failed, CommandResult::Completed)
    }

    fn start_profile_task(
        &mut self,
        context: CommandContext,
        reply: oneshot::Sender<CommandReceipt<RuntimeCommandValue>>,
        completion_sender: mpsc::Sender<ProfileCompletion>,
    ) {
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
        let correlation = TaskCorrelation::new(
            context.request_id(),
            plan.public_key(),
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

    fn complete_profile_task(&mut self, completion: ProfileCompletion) {
        let Some(task) = self.profile_tasks.remove(&completion.request_id) else {
            return;
        };
        let current = self.adapter.core().snapshot();
        let correlated = task.correlation.session_generation() == self.session_generation
            && task.correlation.expected_revision() == current.revision()
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

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;

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
        assert!(secrets.contains(public_key).expect("credential"));

        let signed_out = actor.sign_out().await.expect("sign out");
        assert_eq!(signed_out.session(), SessionState::SignedOut);
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
}
