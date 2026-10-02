use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};

use tokio::sync::{mpsc, watch};

use crate::{AppSnapshot, SnapshotRevision};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ChangeSubscriptionId(NonZeroU64);

impl ChangeSubscriptionId {
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotChange {
    snapshot: AppSnapshot,
    previous_revision: Option<SnapshotRevision>,
}

impl SnapshotChange {
    #[must_use]
    pub const fn revision(&self) -> SnapshotRevision {
        self.snapshot.revision()
    }

    #[must_use]
    pub const fn snapshot(&self) -> &AppSnapshot {
        &self.snapshot
    }

    #[must_use]
    pub fn into_snapshot(self) -> AppSnapshot {
        self.snapshot
    }

    #[must_use]
    pub const fn previous_revision(&self) -> Option<SnapshotRevision> {
        self.previous_revision
    }

    #[must_use]
    pub fn recovers_gap_after(&self, observed: SnapshotRevision) -> bool {
        self.previous_revision
            .is_some_and(|previous| previous != observed)
    }
}

pub struct SnapshotChangeReceiver {
    receiver: mpsc::Receiver<SnapshotChange>,
    latest: watch::Receiver<SnapshotChange>,
    last_delivered_revision: Option<SnapshotRevision>,
}

impl SnapshotChangeReceiver {
    pub async fn receive(&mut self) -> Option<SnapshotChange> {
        loop {
            let queue_open = match self.receiver.try_recv() {
                Ok(change) => {
                    if let Some(change) = self.deliver_newer(change) {
                        return Some(change);
                    }
                    continue;
                }
                Err(mpsc::error::TryRecvError::Empty) => true,
                Err(mpsc::error::TryRecvError::Disconnected) => false,
            };
            let retained = self.latest.borrow().clone();
            if let Some(change) = self.deliver_newer(retained) {
                return Some(change);
            }
            if !queue_open {
                return None;
            }
            if let Some(change) = self.receiver.recv().await
                && let Some(change) = self.deliver_newer(change)
            {
                return Some(change);
            }
        }
    }

    fn deliver_newer(&mut self, change: SnapshotChange) -> Option<SnapshotChange> {
        let revision = change.revision();
        if self
            .last_delivered_revision
            .is_some_and(|delivered| revision <= delivered)
        {
            return None;
        }
        self.last_delivered_revision = Some(revision);
        Some(change)
    }
}

struct SnapshotChangeSubscriber {
    sender: mpsc::Sender<SnapshotChange>,
    latest: watch::Sender<SnapshotChange>,
}

pub struct OrderedSnapshotChanges {
    latest: AppSnapshot,
    next_subscription: u64,
    subscribers: BTreeMap<ChangeSubscriptionId, SnapshotChangeSubscriber>,
    closed: bool,
}

impl OrderedSnapshotChanges {
    #[must_use]
    pub fn new(initial_snapshot: AppSnapshot) -> Self {
        Self {
            latest: initial_snapshot,
            next_subscription: 1,
            subscribers: BTreeMap::new(),
            closed: false,
        }
    }

    #[must_use]
    pub const fn last_revision(&self) -> SnapshotRevision {
        self.latest.revision()
    }

    /// Registers a bounded consumer for future changes.
    ///
    /// # Errors
    ///
    /// Returns `None` if the subscription identifier space is exhausted.
    pub fn subscribe(
        &mut self,
        capacity: NonZeroUsize,
    ) -> Option<(ChangeSubscriptionId, SnapshotChangeReceiver)> {
        if self.closed {
            return None;
        }
        let id = ChangeSubscriptionId(NonZeroU64::new(self.next_subscription)?);
        self.next_subscription = self.next_subscription.checked_add(1)?;
        let initial = SnapshotChange {
            snapshot: self.latest.clone(),
            previous_revision: None,
        };
        let (sender, receiver) = mpsc::channel(capacity.get());
        let (latest, retained) = watch::channel(initial.clone());
        sender.try_send(initial).ok()?;
        self.subscribers
            .insert(id, SnapshotChangeSubscriber { sender, latest });
        Some((
            id,
            SnapshotChangeReceiver {
                receiver,
                latest: retained,
                last_delivered_revision: None,
            },
        ))
    }

    #[must_use]
    pub fn unsubscribe(&mut self, id: ChangeSubscriptionId) -> bool {
        self.subscribers.remove(&id).is_some()
    }

    pub fn publish(&mut self, snapshot: AppSnapshot) {
        self.subscribers
            .retain(|_, subscriber| !subscriber.sender.is_closed());
        if self.closed || snapshot.revision() <= self.latest.revision() {
            return;
        }
        let change = SnapshotChange {
            previous_revision: Some(self.latest.revision()),
            snapshot,
        };
        self.latest = change.snapshot.clone();
        self.subscribers.retain(|_, subscriber| {
            // Retain before enqueue so saturation and sender closure preserve the tail.
            drop(subscriber.latest.send_replace(change.clone()));
            match subscriber.sender.try_send(change.clone()) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.subscribers.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::num::NonZeroUsize;
    use std::task::Poll;
    use std::time::Duration;

    use crate::{
        AppSnapshot, OrderedSnapshotChanges, RelayConfiguration, SessionState, SnapshotChange,
        SnapshotChangeReceiver, SnapshotRevision,
    };

    const RECEIVE_TIMEOUT: Duration = Duration::from_secs(1);

    #[tokio::test]
    async fn change_stream_publishes_monotonic_revisions_to_multiple_consumers() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut first) = changes
            .subscribe(NonZeroUsize::new(4).expect("capacity"))
            .expect("first subscription");
        let (_, mut second) = changes
            .subscribe(NonZeroUsize::new(4).expect("capacity"))
            .expect("second subscription");

        assert_eq!(
            first.receive().await.expect("initial").revision(),
            revision(0)
        );
        assert_eq!(
            second.receive().await.expect("initial").revision(),
            revision(0)
        );
        changes.publish(snapshot(1));
        changes.publish(snapshot(1));
        changes.publish(snapshot(2));

        for receiver in [&mut first, &mut second] {
            assert_eq!(
                receiver.receive().await.expect("revision 1").revision(),
                revision(1)
            );
            assert_eq!(
                receiver.receive().await.expect("revision 2").revision(),
                revision(2)
            );
        }
        assert_eq!(changes.last_revision(), revision(2));
    }

    #[tokio::test]
    async fn slow_consumers_expose_a_revision_gap_without_blocking_publication() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");

        assert_eq!(
            receiver.receive().await.expect("initial").revision(),
            revision(0)
        );
        changes.publish(snapshot(1));
        changes.publish(snapshot(2));
        let first = receiver.receive().await.expect("first");
        assert_eq!(first.revision(), revision(1));
        changes.publish(snapshot(3));
        let recovered = receiver.receive().await.expect("gap recovery");
        assert_eq!(recovered.revision(), revision(3));
        assert!(recovered.recovers_gap_after(first.revision()));
    }

    #[tokio::test]
    async fn close_terminates_consumers_and_rejects_later_subscriptions() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        receiver.receive().await.expect("initial");

        changes.close();
        changes.publish(snapshot(1));
        assert!(receiver.receive().await.is_none());
        assert!(
            changes
                .subscribe(NonZeroUsize::new(1).expect("capacity"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn capacity_one_recovers_final_tail_without_later_publication() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        let initial = next_change(&mut receiver, "initial").await;
        assert_eq!(initial.revision(), revision(0));

        changes.publish(snapshot(1));
        changes.publish(snapshot(2));
        let queued = next_change(&mut receiver, "queued revision 1").await;
        assert_eq!(queued.revision(), revision(1));
        assert_eq!(queued.previous_revision(), Some(revision(0)));
        let tail = next_change(&mut receiver, "final revision 2 without another publish").await;
        assert_eq!(tail.revision(), revision(2));
        assert_eq!(tail.snapshot(), &snapshot(2));
        assert_eq!(tail.previous_revision(), Some(revision(1)));
        assert!(!tail.recovers_gap_after(queued.revision()));
        assert_eq!(changes.last_revision(), revision(2));
    }

    #[tokio::test]
    async fn sustained_overflow_coalesces_latest_tail_with_original_gap_metadata() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        next_change(&mut receiver, "initial").await;

        for value in 1..=128 {
            changes.publish(snapshot(value));
        }
        assert_eq!(changes.last_revision(), revision(128));
        let queued = next_change(&mut receiver, "queued revision 1").await;
        assert_eq!(queued.revision(), revision(1));
        let latest = next_change(&mut receiver, "coalesced revision 128").await;
        assert_eq!(latest.revision(), revision(128));
        assert_eq!(latest.snapshot(), &snapshot(128));
        assert_eq!(latest.previous_revision(), Some(revision(127)));
        assert!(latest.recovers_gap_after(queued.revision()));

        changes.publish(snapshot(128));
        changes.publish(snapshot(127));
        changes.close();
        assert!(next_delivery(&mut receiver).await.is_none());
    }

    #[tokio::test]
    async fn queued_initial_snapshot_precedes_coalesced_latest_tail() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        changes.publish(snapshot(1));
        changes.publish(snapshot(2));

        let initial = next_change(&mut receiver, "queued initial snapshot").await;
        assert_eq!(initial.revision(), revision(0));
        assert_eq!(initial.previous_revision(), None);
        let latest = next_change(&mut receiver, "latest after queued initial").await;
        assert_eq!(latest.revision(), revision(2));
        assert_eq!(latest.previous_revision(), Some(revision(1)));
        assert!(latest.recovers_gap_after(initial.revision()));
        changes.close();
        assert!(next_delivery(&mut receiver).await.is_none());
    }

    #[tokio::test]
    async fn independently_paced_consumers_recover_their_own_latest_tail() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut slow) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("slow subscription");
        let (_, mut fast) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("fast subscription");
        next_change(&mut slow, "slow initial").await;
        next_change(&mut fast, "fast initial").await;

        for value in 1..=3 {
            changes.publish(snapshot(value));
            let delivered = next_change(&mut fast, "fast consumer revision").await;
            assert_eq!(delivered.revision(), revision(value));
            assert_eq!(delivered.previous_revision(), Some(revision(value - 1)));
            assert!(!delivered.recovers_gap_after(revision(value - 1)));
        }
        let queued = next_change(&mut slow, "slow queued revision 1").await;
        assert_eq!(queued.revision(), revision(1));
        let tail = next_change(&mut slow, "slow retained revision 3").await;
        assert_eq!(tail.revision(), revision(3));
        assert_eq!(tail.previous_revision(), Some(revision(2)));
        assert!(tail.recovers_gap_after(queued.revision()));
        changes.close();
        assert!(next_delivery(&mut slow).await.is_none());
        assert!(next_delivery(&mut fast).await.is_none());
    }

    #[tokio::test]
    async fn full_close_drains_queued_change_and_final_tail_before_none() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        next_change(&mut receiver, "initial").await;
        changes.publish(snapshot(1));
        changes.publish(snapshot(2));
        changes.close();
        changes.publish(snapshot(3));

        assert_eq!(
            next_change(&mut receiver, "queued revision before close")
                .await
                .revision(),
            revision(1)
        );
        let tail = next_change(&mut receiver, "retained final revision before close").await;
        assert_eq!(tail.revision(), revision(2));
        assert_eq!(tail.previous_revision(), Some(revision(1)));
        assert!(next_delivery(&mut receiver).await.is_none());
        assert!(next_delivery(&mut receiver).await.is_none());
        assert_eq!(changes.last_revision(), revision(2));
    }

    #[tokio::test]
    async fn unsubscribe_drains_retained_tail_without_future_updates() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (id, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        next_change(&mut receiver, "initial").await;
        changes.publish(snapshot(1));
        changes.publish(snapshot(2));
        assert!(changes.unsubscribe(id));
        assert!(!changes.unsubscribe(id));
        changes.publish(snapshot(3));

        assert_eq!(
            next_change(&mut receiver, "queued revision before unsubscribe")
                .await
                .revision(),
            revision(1)
        );
        let tail = next_change(&mut receiver, "retained revision before unsubscribe").await;
        assert_eq!(tail.revision(), revision(2));
        assert_eq!(tail.previous_revision(), Some(revision(1)));
        assert!(next_delivery(&mut receiver).await.is_none());
        assert_eq!(changes.last_revision(), revision(3));
    }

    #[tokio::test]
    async fn cancelled_pending_receive_preserves_future_queued_and_retained_tail() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (_, mut receiver) = changes
            .subscribe(NonZeroUsize::new(1).expect("capacity"))
            .expect("subscription");
        next_change(&mut receiver, "initial").await;
        {
            let pending = receiver.receive();
            tokio::pin!(pending);
            tokio::time::timeout(
                RECEIVE_TIMEOUT,
                std::future::poll_fn(|context| {
                    assert!(pending.as_mut().poll(context).is_pending());
                    Poll::Ready(())
                }),
            )
            .await
            .expect("empty receive was polled before cancellation");
        }

        changes.publish(snapshot(1));
        changes.publish(snapshot(2));
        assert_eq!(
            next_change(&mut receiver, "queued revision after cancellation")
                .await
                .revision(),
            revision(1)
        );
        let tail = next_change(&mut receiver, "retained revision after cancellation").await;
        assert_eq!(tail.revision(), revision(2));
        assert_eq!(tail.previous_revision(), Some(revision(1)));
        changes.publish(snapshot(3));
        assert_eq!(
            next_change(&mut receiver, "future revision after recovered tail")
                .await
                .revision(),
            revision(3)
        );
        changes.close();
        assert!(next_delivery(&mut receiver).await.is_none());
    }

    #[tokio::test]
    async fn unchanged_publish_reclaims_abandoned_receiver_registration() {
        let mut changes = OrderedSnapshotChanges::new(snapshot(0));
        let (id, receiver) = changes.subscribe(NonZeroUsize::MIN).expect("subscription");
        drop(receiver);

        changes.publish(snapshot(0));

        assert!(
            !changes.unsubscribe(id),
            "unchanged observation must reclaim a closed receiver"
        );
        assert_eq!(changes.last_revision(), revision(0));
        let (_, mut replacement) = changes.subscribe(NonZeroUsize::MIN).expect("replacement");
        let initial = next_change(&mut replacement, "replacement initial snapshot").await;
        assert_eq!(initial.revision(), revision(0));
        assert!(initial.previous_revision().is_none());
        changes.close();
        assert!(next_delivery(&mut replacement).await.is_none());
    }

    async fn next_delivery(receiver: &mut SnapshotChangeReceiver) -> Option<SnapshotChange> {
        tokio::time::timeout(RECEIVE_TIMEOUT, receiver.receive())
            .await
            .expect("snapshot receive completed within its bound")
    }

    async fn next_change(receiver: &mut SnapshotChangeReceiver, label: &str) -> SnapshotChange {
        next_delivery(receiver).await.expect(label)
    }

    fn revision(value: u64) -> SnapshotRevision {
        SnapshotRevision::from_value(value)
    }

    fn snapshot(value: u64) -> AppSnapshot {
        if value == 0 {
            AppSnapshot::booting()
        } else {
            AppSnapshot::ready(
                revision(value),
                RelayConfiguration::default(),
                Vec::new(),
                None,
                SessionState::SignedOut,
                None,
                None,
            )
            .expect("snapshot")
        }
    }
}
