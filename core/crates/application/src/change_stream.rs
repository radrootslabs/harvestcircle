use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};

use tokio::sync::mpsc;

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
}

impl SnapshotChangeReceiver {
    pub async fn receive(&mut self) -> Option<SnapshotChange> {
        self.receiver.recv().await
    }
}

pub struct OrderedSnapshotChanges {
    latest: AppSnapshot,
    next_subscription: u64,
    subscribers: BTreeMap<ChangeSubscriptionId, mpsc::Sender<SnapshotChange>>,
}

impl OrderedSnapshotChanges {
    #[must_use]
    pub fn new(initial_snapshot: AppSnapshot) -> Self {
        Self {
            latest: initial_snapshot,
            next_subscription: 1,
            subscribers: BTreeMap::new(),
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
        let id = ChangeSubscriptionId(NonZeroU64::new(self.next_subscription)?);
        self.next_subscription = self.next_subscription.checked_add(1)?;
        let (sender, receiver) = mpsc::channel(capacity.get());
        sender
            .try_send(SnapshotChange {
                snapshot: self.latest.clone(),
                previous_revision: None,
            })
            .ok()?;
        self.subscribers.insert(id, sender);
        Some((id, SnapshotChangeReceiver { receiver }))
    }

    #[must_use]
    pub fn unsubscribe(&mut self, id: ChangeSubscriptionId) -> bool {
        self.subscribers.remove(&id).is_some()
    }

    pub fn publish(&mut self, snapshot: AppSnapshot) {
        if snapshot.revision() <= self.latest.revision() {
            return;
        }
        let change = SnapshotChange {
            previous_revision: Some(self.latest.revision()),
            snapshot,
        };
        self.latest = change.snapshot.clone();
        self.subscribers
            .retain(|_, sender| match sender.try_send(change.clone()) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            });
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use crate::{
        AppSnapshot, OrderedSnapshotChanges, RelayConfiguration, SessionState, SnapshotRevision,
    };

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
