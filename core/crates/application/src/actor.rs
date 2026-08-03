use std::num::{NonZeroU64, NonZeroUsize};
use std::time::Instant;

use radroots_studio_domain::SafeError;
use tokio::sync::{mpsc, oneshot};

use crate::SnapshotRevision;

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

#[derive(Clone)]
pub struct ActorMailbox<C, R> {
    sender: mpsc::Sender<CommandEnvelope<C, R>>,
}

impl<C, R> ActorMailbox<C, R> {
    #[must_use]
    pub fn bounded(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<CommandEnvelope<C, R>>) {
        let (sender, receiver) = mpsc::channel(capacity.get());
        (Self { sender }, receiver)
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
        CommandSubmission, RequestId,
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
}
