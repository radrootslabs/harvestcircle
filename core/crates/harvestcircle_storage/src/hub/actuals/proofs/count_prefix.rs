//! A completed Count prefix belongs to one unchanged transaction read phase.
//! This scalar stores no bodies, current authority or admission permission.
use super::*;
use harvestcircle_domain::hub::HubRevision;

#[derive(Default)]
pub(super) struct CountPrefix {
    phase: Option<(i64, i64)>,
    completed: Option<(HubId, HubRevision)>,
}

impl OriginalReader<'_, '_> {
    pub(in crate::hub) async fn require_count_read_phase(&mut self) -> Result<(), SafeError> {
        // Drop this reader before any write, even in the same transaction.
        // DML (including rolled-back writes) and main DDL invalidate reuse.
        // The sealed host separately requires a main-only connection before
        // and after every transaction; TEMP/attached namespaces are outside
        // that accepted contract. No connection-control bypass belongs here.
        // External writers cannot change this transaction's SQL snapshot.
        let phase: (i64, i64) =
            sqlx::query_as("SELECT total_changes(),schema_version FROM pragma_schema_version")
                .fetch_one(&mut *self.tx)
                .await
                .map_err(|_| storage_unavailable())?;
        if phase.0 < 0 || phase.1 < 0 || self.count_prefix.phase.is_some_and(|prior| prior != phase)
        {
            return Err(corrupt_storage());
        }
        self.count_prefix.phase = Some(phase);
        Ok(())
    }

    pub(in crate::hub) async fn count_prefix_covers(
        &mut self,
        hub: HubId,
        cut: HubRevision,
    ) -> Result<bool, SafeError> {
        self.require_count_read_phase().await?;
        // If this cut could include its active parent, retain the per-root
        // canonical/decreasing-edge path. Individual original, body and
        // certificate lookups always retain their existing canonical guards.
        Ok(self.count_prefix.completed.is_some_and(|(owner, through)| {
            owner == hub
                && cut.value() <= through.value()
                && self.proofs.active.len() < CERTIFICATES
                && self.proofs.active.last().is_none_or(|parent| {
                    parent.hub == *hub.as_bytes() && cut.value() < parent.stock
                })
        }))
    }

    pub(in crate::hub) const fn count_prefix_checkpoint(&self) -> Option<(HubId, HubRevision)> {
        self.count_prefix.completed
    }

    pub(in crate::hub) fn restore_count_prefix(
        &mut self,
        checkpoint: Option<(HubId, HubRevision)>,
    ) {
        self.count_prefix.completed = checkpoint;
    }

    pub(in crate::hub) fn complete_count_prefix(&mut self, hub: HubId, cut: HubRevision) {
        if self
            .count_prefix
            .completed
            .is_none_or(|(owner, prior)| owner != hub || cut.value() > prior.value())
        {
            self.count_prefix.completed = Some((hub, cut));
        }
    }
}
