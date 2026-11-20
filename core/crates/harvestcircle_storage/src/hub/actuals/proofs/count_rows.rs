//! Completed historical inventory points, private to one unchanged read phase.
//! These counts carry no original body, present-day capacity or execution grant.
use super::*;
use crate::hub::corrections;
use harvestcircle_domain::hub::{
    HubRevision, corrections::HUB_CORRECTION_ROWS, reference::HubRecordId,
};

type Point = ([u8; 16], [u8; 16], u64);

// Only this read-phase owner can construct a seed. No constructor, Clone,
// Default or reader accessor permits another caller to invent a prior count.
pub(in crate::hub) struct CountRowSeed {
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
    total: usize,
}
impl CountRowSeed {
    pub(in crate::hub) fn point(
        &self,
        hub: HubId,
        trace: HubRecordId,
    ) -> Result<(HubRevision, usize), SafeError> {
        if self.hub != hub || self.trace != trace {
            return Err(corrupt_storage());
        }
        Ok((self.cut, self.total))
    }
}
pub(super) struct CountRows {
    points: BTreeMap<Point, usize>,
    #[cfg(test)]
    limit: usize,
}
impl CountRows {
    pub(super) fn new() -> Self {
        Self {
            points: BTreeMap::new(),
            #[cfg(test)]
            limit: HUB_CORRECTION_ROWS,
        }
    }
}
impl OriginalReader<'_, '_> {
    #[cfg(test)]
    pub(in crate::hub) fn test_count_row_limit(&mut self, limit: usize) {
        self.count_rows.limit = limit.min(HUB_CORRECTION_ROWS);
        self.count_rows.points.clear();
    }

    pub(in crate::hub) fn correction_rows_on<'a>(
        &'a mut self,
        hub: HubId,
        trace: HubRecordId,
        cut: HubRevision,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, SafeError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.require_count_read_phase().await?;
            let key = (*hub.as_bytes(), *trace.as_bytes(), cut.value());
            if let Some(total) = self.count_rows.points.get(&key) {
                return Ok(*total);
            }
            let seed = self
                .count_rows
                .points
                .range((key.0, key.1, 0)..key)
                .next_back()
                .map(|((_, _, revision), total)| {
                    Ok::<_, SafeError>(CountRowSeed {
                        hub,
                        trace,
                        cut: HubRevision::from_value(*revision).map_err(|_| corrupt_storage())?,
                        total: *total,
                    })
                })
                .transpose()?;
            let total =
                corrections::correction_rows_from_on(self.tx, hub, trace, cut, seed).await?;
            #[cfg(test)]
            let limit = self.count_rows.limit;
            #[cfg(not(test))]
            let limit = HUB_CORRECTION_ROWS;
            if limit != 0 {
                if self.count_rows.points.len() >= limit {
                    self.count_rows.points.pop_first();
                }
                self.count_rows.points.insert(key, total);
            }
            Ok(total)
        })
    }

    #[cfg(test)]
    pub(in crate::hub) fn test_count_row_points(&self) -> Vec<(Point, usize)> {
        self.count_rows
            .points
            .iter()
            .map(|(key, total)| (*key, *total))
            .collect()
    }

    pub(in crate::hub) async fn require_prior_correction_room_on(
        &mut self,
        hub: HubId,
        trace: HubRecordId,
        cut: HubRevision,
    ) -> Result<(), SafeError> {
        if self.correction_rows_on(hub, trace, cut).await? >= HUB_CORRECTION_ROWS {
            return Err(corrupt_storage());
        }
        Ok(())
    }
}
