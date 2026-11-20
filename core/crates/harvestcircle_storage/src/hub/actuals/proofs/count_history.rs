//! Fixed-size shallow account prefixes, owned privately by one native reader.
//! Full originals, exact-cut Return floors and current authority stay separate.
use super::*;
use crate::hub::corrections::{CountHistory, count_history_in_phase_on};
use harvestcircle_domain::hub::{
    HubRevision, corrections::HUB_CORRECTION_ROWS, counts::CountPostPlan,
};

type HistoryKey = ([u8; 16], [u8; 32]);

// No Default/Clone/public constructor or accessor on OriginalReader: a caller
// cannot supply this store independently of the reader's transaction borrow.
pub(in crate::hub) struct CountHistoryCache {
    accounts: BTreeMap<HistoryKey, (HubRevision, CountHistory)>,
    rows: usize,
    #[cfg(test)]
    limits: Option<(usize, usize)>,
}
impl CountHistoryCache {
    pub(super) fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            rows: 0,
            #[cfg(test)]
            limits: None,
        }
    }

    pub(in crate::hub) fn prefix(
        &self,
        hub: HubId,
        digest: [u8; 32],
    ) -> Option<(HubRevision, CountHistory)> {
        self.accounts.get(&(*hub.as_bytes(), digest)).cloned()
    }

    pub(in crate::hub) fn remember(
        &mut self,
        hub: HubId,
        digest: [u8; 32],
        cut: HubRevision,
        history: &CountHistory,
    ) {
        let key = (*hub.as_bytes(), digest);
        #[cfg(test)]
        let (account_limit, row_limit) = self
            .limits
            .unwrap_or((HUB_CORRECTION_ROWS, HUB_CORRECTION_ROWS));
        #[cfg(not(test))]
        let (account_limit, row_limit) = (HUB_CORRECTION_ROWS, HUB_CORRECTION_ROWS);
        let rows = history
            .explanations
            .len()
            .saturating_add(history.amendments.len());
        if account_limit == 0 || rows > row_limit {
            return;
        }
        let previous = self.accounts.get(&key).map_or(0, |(_, history)| {
            history.explanations.len() + history.amendments.len()
        });
        let next = self.rows.saturating_sub(previous).saturating_add(rows);
        if next > row_limit
            || (self.accounts.len() >= account_limit && !self.accounts.contains_key(&key))
        {
            // Memo pressure is a miss, never a new product capacity failure.
            self.accounts.clear();
            self.rows = 0;
        }
        self.accounts.insert(key, (cut, history.clone()));
        self.rows = self
            .accounts
            .values()
            .map(|(_, h)| h.explanations.len() + h.amendments.len())
            .sum();
    }
}

impl OriginalReader<'_, '_> {
    #[cfg(test)]
    pub(in crate::hub) fn test_history_limits(&mut self, accounts: usize, rows: usize) {
        self.count_history.limits = Some((
            accounts.min(HUB_CORRECTION_ROWS),
            rows.min(HUB_CORRECTION_ROWS),
        ));
    }

    #[cfg(test)]
    pub(in crate::hub) fn test_history_stats(&self) -> (usize, usize, Option<HubRevision>) {
        (
            self.count_history.accounts.len(),
            self.count_history.rows,
            self.count_history
                .accounts
                .values()
                .map(|(cut, _)| *cut)
                .max_by_key(|cut| cut.value()),
        )
    }

    pub(in crate::hub) async fn count_history_on(
        &mut self,
        hub: HubId,
        count: &CountPostPlan,
        cut: HubRevision,
    ) -> Result<CountHistory, SafeError> {
        self.require_count_read_phase().await?;
        count_history_in_phase_on(self.tx, &mut self.count_history, hub, count, cut).await
    }
}
