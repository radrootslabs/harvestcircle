use std::num::{NonZeroU64, NonZeroUsize};
use std::time::Instant;

use radroots_studio_domain::{PublicKey, SafeError};
use tokio::sync::{mpsc, oneshot};

use crate::SnapshotRevision;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskCorrelation {
    request_id: RequestId,
    account: PublicKey,
    expected_revision: SnapshotRevision,
    session_generation: SessionGeneration,
}

impl TaskCorrelation {
    #[must_use]
    pub const fn new(
        request_id: RequestId,
        account: PublicKey,
        expected_revision: SnapshotRevision,
        session_generation: SessionGeneration,
    ) -> Self {
        Self {
            request_id,
            account,
            expected_revision,
            session_generation,
        }
    }

    #[must_use]
    pub const fn request_id(self) -> RequestId {
        self.request_id
    }

    #[must_use]
    pub const fn account(self) -> PublicKey {
        self.account
    }

    #[must_use]
    pub const fn expected_revision(self) -> SnapshotRevision {
        self.expected_revision
    }

    #[must_use]
    pub const fn session_generation(self) -> SessionGeneration {
        self.session_generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeLifecycle {
    Opening,
    CompatibilityChecking,
    AcquiringOwnership,
    Migrating,
    Recovering,
    Ready,
    Degraded(SafeError),
    Blocked(SafeError),
    ShuttingDown,
    Closed,
    Fatal(SafeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeCommandClass {
    Observe,
    MutateLocalState,
    UseCredential,
    UseRelay,
    RetryOpening,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleGate {
    lifecycle: RuntimeLifecycle,
}

impl Default for LifecycleGate {
    fn default() -> Self {
        Self::opening()
    }
}

impl LifecycleGate {
    #[must_use]
    pub const fn opening() -> Self {
        Self {
            lifecycle: RuntimeLifecycle::Opening,
        }
    }

    #[must_use]
    pub const fn lifecycle(self) -> RuntimeLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn allows(self, command: RuntimeCommandClass) -> bool {
        match self.lifecycle {
            RuntimeLifecycle::Opening
            | RuntimeLifecycle::CompatibilityChecking
            | RuntimeLifecycle::AcquiringOwnership
            | RuntimeLifecycle::Migrating
            | RuntimeLifecycle::Recovering => {
                matches!(
                    command,
                    RuntimeCommandClass::Observe | RuntimeCommandClass::Shutdown
                )
            }
            RuntimeLifecycle::Ready => !matches!(command, RuntimeCommandClass::RetryOpening),
            RuntimeLifecycle::Degraded(_) => !matches!(
                command,
                RuntimeCommandClass::UseRelay | RuntimeCommandClass::RetryOpening
            ),
            RuntimeLifecycle::Blocked(_) => matches!(
                command,
                RuntimeCommandClass::Observe
                    | RuntimeCommandClass::RetryOpening
                    | RuntimeCommandClass::Shutdown
            ),
            RuntimeLifecycle::ShuttingDown => matches!(command, RuntimeCommandClass::Observe),
            RuntimeLifecycle::Closed | RuntimeLifecycle::Fatal(_) => false,
        }
    }

    /// Advances the required open sequence to compatibility checking.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the stage is out of order.
    pub fn begin_compatibility_check(&mut self) -> Result<(), SafeError> {
        self.advance(
            RuntimeLifecycle::Opening,
            RuntimeLifecycle::CompatibilityChecking,
        )
    }

    /// Records compatibility acceptance and begins ownership acquisition.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the stage is out of order.
    pub fn compatibility_accepted(&mut self) -> Result<(), SafeError> {
        self.advance(
            RuntimeLifecycle::CompatibilityChecking,
            RuntimeLifecycle::AcquiringOwnership,
        )
    }

    /// Records exclusive ownership and begins migration.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the stage is out of order.
    pub fn ownership_acquired(&mut self) -> Result<(), SafeError> {
        self.advance(
            RuntimeLifecycle::AcquiringOwnership,
            RuntimeLifecycle::Migrating,
        )
    }

    /// Records migration completion and begins recovery.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the stage is out of order.
    pub fn migration_complete(&mut self) -> Result<(), SafeError> {
        self.advance(RuntimeLifecycle::Migrating, RuntimeLifecycle::Recovering)
    }

    /// Records recovery completion and admits normal commands.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the stage is out of order.
    pub fn recovery_complete(&mut self) -> Result<(), SafeError> {
        self.advance(RuntimeLifecycle::Recovering, RuntimeLifecycle::Ready)
    }

    pub fn block(&mut self, error: SafeError) {
        self.lifecycle = RuntimeLifecycle::Blocked(error);
    }

    pub fn fail(&mut self, error: SafeError) {
        self.lifecycle = RuntimeLifecycle::Fatal(error);
    }

    /// Moves a ready runtime into a nonfatal degraded state.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the runtime is not ready.
    pub fn degrade(&mut self, error: SafeError) -> Result<(), SafeError> {
        self.advance(RuntimeLifecycle::Ready, RuntimeLifecycle::Degraded(error))
    }

    /// Restores local and relay command availability after degradation.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error when the runtime is not degraded.
    pub fn restore_ready(&mut self) -> Result<(), SafeError> {
        if !matches!(self.lifecycle, RuntimeLifecycle::Degraded(_)) {
            return Err(invalid_lifecycle_transition());
        }
        self.lifecycle = RuntimeLifecycle::Ready;
        Ok(())
    }

    /// Begins actor-owned shutdown.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error after shutdown or close has begun.
    pub fn begin_shutdown(&mut self) -> Result<(), SafeError> {
        if matches!(
            self.lifecycle,
            RuntimeLifecycle::ShuttingDown | RuntimeLifecycle::Closed
        ) {
            return Err(invalid_lifecycle_transition());
        }
        self.lifecycle = RuntimeLifecycle::ShuttingDown;
        Ok(())
    }

    /// Completes actor-owned shutdown.
    ///
    /// # Errors
    ///
    /// Returns a safe lifecycle error unless shutdown already began.
    pub fn finish_shutdown(&mut self) -> Result<(), SafeError> {
        self.advance(RuntimeLifecycle::ShuttingDown, RuntimeLifecycle::Closed)
    }

    fn advance(
        &mut self,
        expected: RuntimeLifecycle,
        next: RuntimeLifecycle,
    ) -> Result<(), SafeError> {
        if self.lifecycle != expected {
            return Err(invalid_lifecycle_transition());
        }
        self.lifecycle = next;
        Ok(())
    }
}

const fn invalid_lifecycle_transition() -> SafeError {
    SafeError::new(
        radroots_studio_domain::SafeErrorCode::InvalidApplicationState,
        radroots_studio_domain::SafeMessage::new("The runtime lifecycle transition is invalid."),
    )
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandContext {
    request_id: RequestId,
    expected_revision: Option<SnapshotRevision>,
    deadline: Instant,
}

impl CommandContext {
    #[must_use]
    pub const fn new(
        request_id: RequestId,
        expected_revision: Option<SnapshotRevision>,
        deadline: Instant,
    ) -> Self {
        Self {
            request_id,
            expected_revision,
            deadline,
        }
    }

    #[must_use]
    pub const fn request_id(self) -> RequestId {
        self.request_id
    }

    #[must_use]
    pub const fn expected_revision(self) -> Option<SnapshotRevision> {
        self.expected_revision
    }

    #[must_use]
    pub const fn deadline(self) -> Instant {
        self.deadline
    }

    #[must_use]
    pub fn is_expired(self, now: Instant) -> bool {
        now >= self.deadline
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRejection {
    MailboxSaturated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandResult<T> {
    Completed(T),
    Rejected(CommandRejection),
    Conflicted { current_revision: SnapshotRevision },
    TimedOut,
    Closed,
    Failed(SafeError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandReceipt<T> {
    request_id: RequestId,
    result: CommandResult<T>,
}

impl<T> CommandReceipt<T> {
    #[must_use]
    pub const fn new(request_id: RequestId, result: CommandResult<T>) -> Self {
        Self { request_id, result }
    }

    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    #[must_use]
    pub const fn result(&self) -> &CommandResult<T> {
        &self.result
    }

    #[must_use]
    pub fn into_result(self) -> CommandResult<T> {
        self.result
    }
}

pub struct CommandTicket<T> {
    request_id: RequestId,
    receiver: oneshot::Receiver<CommandReceipt<T>>,
}

impl<T> CommandTicket<T> {
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub async fn receipt(self) -> CommandReceipt<T> {
        self.receiver
            .await
            .unwrap_or_else(|_| CommandReceipt::new(self.request_id, CommandResult::Closed))
    }
}

pub enum CommandSubmission<T> {
    Accepted(CommandTicket<T>),
    Rejected(CommandReceipt<T>),
}

impl<T> CommandSubmission<T> {
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        match self {
            Self::Accepted(ticket) => ticket.request_id(),
            Self::Rejected(receipt) => receipt.request_id(),
        }
    }
}

pub struct CommandEnvelope<C, R> {
    context: CommandContext,
    command: C,
    reply: oneshot::Sender<CommandReceipt<R>>,
}

impl<C, R> CommandEnvelope<C, R> {
    #[must_use]
    pub const fn context(&self) -> CommandContext {
        self.context
    }

    #[must_use]
    pub const fn command(&self) -> &C {
        &self.command
    }

    #[must_use]
    pub fn into_parts(self) -> (CommandContext, C, oneshot::Sender<CommandReceipt<R>>) {
        (self.context, self.command, self.reply)
    }
}

pub struct ActorMailbox<C, R> {
    sender: mpsc::Sender<CommandEnvelope<C, R>>,
}

impl<C, R> Clone for ActorMailbox<C, R> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<C, R> ActorMailbox<C, R> {
    #[must_use]
    pub fn bounded(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<CommandEnvelope<C, R>>) {
        let (sender, receiver) = mpsc::channel(capacity.get());
        (Self { sender }, receiver)
    }

    #[must_use]
    pub fn available_capacity(&self) -> usize {
        self.sender.capacity()
    }

    #[must_use]
    pub fn submit(&self, context: CommandContext, command: C) -> CommandSubmission<R> {
        let request_id = context.request_id();
        if context.is_expired(Instant::now()) {
            return CommandSubmission::Rejected(CommandReceipt::new(
                request_id,
                CommandResult::TimedOut,
            ));
        }
        let (reply, receiver) = oneshot::channel();
        let envelope = CommandEnvelope {
            context,
            command,
            reply,
        };
        match self.sender.try_send(envelope) {
            Ok(()) => CommandSubmission::Accepted(CommandTicket {
                request_id,
                receiver,
            }),
            Err(mpsc::error::TrySendError::Full(_)) => {
                CommandSubmission::Rejected(CommandReceipt::new(
                    request_id,
                    CommandResult::Rejected(CommandRejection::MailboxSaturated),
                ))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                CommandSubmission::Rejected(CommandReceipt::new(request_id, CommandResult::Closed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::time::{Duration, Instant};

    use crate::{
        ActorMailbox, CommandContext, CommandReceipt, CommandRejection, CommandResult,
        CommandSubmission, LifecycleGate, RequestId, RuntimeCommandClass, RuntimeLifecycle,
    };

    fn context(id: u64) -> CommandContext {
        CommandContext::new(
            RequestId::new(id).expect("nonzero request"),
            None,
            Instant::now() + Duration::from_secs(1),
        )
    }

    #[tokio::test]
    async fn bounded_mailbox_accepts_one_and_rejects_saturation() {
        let (mailbox, mut receiver) =
            ActorMailbox::<u8, u8>::bounded(NonZeroUsize::new(1).expect("capacity"));
        let CommandSubmission::Accepted(ticket) = mailbox.submit(context(1), 7) else {
            panic!("first command must be accepted");
        };
        let CommandSubmission::Rejected(rejected) = mailbox.submit(context(2), 8) else {
            panic!("second command must be rejected");
        };
        assert_eq!(
            rejected.into_result(),
            CommandResult::Rejected(CommandRejection::MailboxSaturated)
        );

        let envelope = receiver.recv().await.expect("command");
        assert_eq!(envelope.context().request_id().get(), 1);
        assert_eq!(*envelope.command(), 7);
        let (context, command, reply) = envelope.into_parts();
        reply
            .send(CommandReceipt::new(
                context.request_id(),
                CommandResult::Completed(command + 1),
            ))
            .expect("ticket remains open");
        assert_eq!(
            ticket.receipt().await.into_result(),
            CommandResult::Completed(8)
        );
    }

    #[test]
    fn expired_and_closed_mailboxes_reject_without_enqueuing() {
        let (mailbox, receiver) =
            ActorMailbox::<u8, u8>::bounded(NonZeroUsize::new(1).expect("capacity"));
        let expired =
            CommandContext::new(RequestId::new(1).expect("request"), None, Instant::now());
        let CommandSubmission::Rejected(receipt) = mailbox.submit(expired, 1) else {
            panic!("expired command must be rejected");
        };
        assert_eq!(receipt.into_result(), CommandResult::TimedOut);

        drop(receiver);
        let CommandSubmission::Rejected(receipt) = mailbox.submit(context(2), 2) else {
            panic!("closed mailbox must be rejected");
        };
        assert_eq!(receipt.into_result(), CommandResult::Closed);
    }

    #[tokio::test]
    async fn dropped_actor_reply_becomes_closed_receipt() {
        let (mailbox, mut receiver) =
            ActorMailbox::<u8, u8>::bounded(NonZeroUsize::new(1).expect("capacity"));
        let CommandSubmission::Accepted(ticket) = mailbox.submit(context(1), 1) else {
            panic!("command must be accepted");
        };
        drop(receiver.recv().await.expect("command"));

        assert_eq!(ticket.receipt().await.into_result(), CommandResult::Closed);
    }

    #[test]
    fn opening_sequence_gates_mutation_until_recovery_completes() {
        let mut lifecycle = LifecycleGate::opening();
        for expected in [
            RuntimeLifecycle::Opening,
            RuntimeLifecycle::CompatibilityChecking,
            RuntimeLifecycle::AcquiringOwnership,
            RuntimeLifecycle::Migrating,
            RuntimeLifecycle::Recovering,
        ] {
            assert_eq!(lifecycle.lifecycle(), expected);
            assert!(lifecycle.allows(RuntimeCommandClass::Observe));
            assert!(lifecycle.allows(RuntimeCommandClass::Shutdown));
            assert!(!lifecycle.allows(RuntimeCommandClass::MutateLocalState));
            match expected {
                RuntimeLifecycle::Opening => {
                    lifecycle
                        .begin_compatibility_check()
                        .expect("compatibility");
                }
                RuntimeLifecycle::CompatibilityChecking => {
                    lifecycle
                        .compatibility_accepted()
                        .expect("compatibility accepted");
                }
                RuntimeLifecycle::AcquiringOwnership => {
                    lifecycle.ownership_acquired().expect("ownership");
                }
                RuntimeLifecycle::Migrating => {
                    lifecycle.migration_complete().expect("migration");
                }
                RuntimeLifecycle::Recovering => {
                    lifecycle.recovery_complete().expect("recovery");
                }
                _ => unreachable!("opening states only"),
            }
        }
        assert_eq!(lifecycle.lifecycle(), RuntimeLifecycle::Ready);
        assert!(lifecycle.allows(RuntimeCommandClass::MutateLocalState));
        assert!(lifecycle.allows(RuntimeCommandClass::UseCredential));
        assert!(lifecycle.allows(RuntimeCommandClass::UseRelay));
    }

    #[test]
    fn blocked_degraded_fatal_and_closed_states_fail_safe() {
        let problem = radroots_studio_domain::SafeError::new(
            radroots_studio_domain::SafeErrorCode::StorageUnavailable,
            radroots_studio_domain::SafeMessage::new("The runtime is unavailable."),
        );
        let mut blocked = LifecycleGate::opening();
        blocked.block(problem);
        assert!(blocked.allows(RuntimeCommandClass::RetryOpening));
        assert!(!blocked.allows(RuntimeCommandClass::MutateLocalState));

        let mut degraded = LifecycleGate::opening();
        degraded.begin_compatibility_check().expect("compatibility");
        degraded.compatibility_accepted().expect("accepted");
        degraded.ownership_acquired().expect("ownership");
        degraded.migration_complete().expect("migration");
        degraded.recovery_complete().expect("recovery");
        degraded.degrade(problem).expect("degraded");
        assert!(degraded.allows(RuntimeCommandClass::MutateLocalState));
        assert!(!degraded.allows(RuntimeCommandClass::UseRelay));
        degraded.restore_ready().expect("restored");

        let mut fatal = LifecycleGate::opening();
        fatal.fail(problem);
        assert!(!fatal.allows(RuntimeCommandClass::Observe));
        fatal.begin_shutdown().expect("fatal can close");
        fatal.finish_shutdown().expect("closed");
        assert_eq!(fatal.lifecycle(), RuntimeLifecycle::Closed);
        assert!(!fatal.allows(RuntimeCommandClass::Shutdown));
    }

    #[test]
    fn opening_stages_reject_out_of_order_and_repeated_transitions() {
        let mut lifecycle = LifecycleGate::opening();
        assert!(lifecycle.migration_complete().is_err());
        lifecycle.begin_compatibility_check().expect("first stage");
        assert!(lifecycle.begin_compatibility_check().is_err());
        assert!(lifecycle.recovery_complete().is_err());
    }
}
