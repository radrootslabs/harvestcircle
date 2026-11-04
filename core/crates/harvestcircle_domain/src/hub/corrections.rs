//! Effective quantity accounting for append-only linked corrections.
//!
//! Originals and issued material remain immutable. These checked arithmetic
//! projections grant no source, stock, treatment, work or execution authority.
//! Native adapters must load complete original-bound ledgers and independently
//! justify each balancing debit or separately identified found remainder.
use super::HubRevision;
use super::reference::{HubRecordId, WholePacks};
use super::trace::TraceBalance;
use std::collections::{BTreeMap, BTreeSet};

mod physical;
pub use physical::*;

mod quantity;
pub use quantity::*;
mod observation;
pub use observation::*;

mod attribution;
pub use attribution::*;
mod intent;

pub const HUB_CORRECTION_ROWS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrectionError {
    Capacity,
    InvalidHistory,
    InvalidQuantity,
    Duplicate,
    ReturnConflict,
    Accounting,
}

/// One original outflow line. Normal and reviewed actual departures retain their
/// separate native families; neither is a generic loss or a mutable stock row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OriginalDepartureQuantity {
    pub operation: HubRecordId,
    pub group: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub stock_revision: HubRevision,
    pub packs: WholePacks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum DepartureQuantityDelta {
    /// Requires a separate current balancing stock debit and original source proof.
    AdditionalDeparture(WholePacks),
    /// Requires observed, separately identified review-required found stock.
    /// It never restores the old physical group or its retired/completed work.
    IdentifiedFoundRemainder(WholePacks),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DepartureQuantityAmendment {
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub before: WholePacks,
    pub after: WholePacks,
    pub delta: DepartureQuantityDelta,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepartureQuantityAccount {
    original: OriginalDepartureQuantity,
    amendments: Vec<DepartureQuantityAmendment>,
    effective: WholePacks,
    additional: WholePacks,
    found: WholePacks,
    established_returns: WholePacks,
}
impl DepartureQuantityAccount {
    /// Reconstruct a complete, ordered native ledger. This is not a physical
    /// plan: observing the right arithmetic cannot establish balancing scope.
    pub fn restore(
        original: OriginalDepartureQuantity,
        amendments: Vec<DepartureQuantityAmendment>,
        established_returns: WholePacks,
    ) -> Result<Self, CorrectionError> {
        if amendments.len() > HUB_CORRECTION_ROWS {
            return Err(CorrectionError::Capacity);
        }
        if original.packs.value() == 0 || original.stock_revision == HubRevision::INITIAL {
            return Err(CorrectionError::InvalidHistory);
        }
        let mut effective = original.packs;
        let mut revision = original.stock_revision;
        let mut operations = BTreeSet::from([original.operation]);
        let mut additional = zero();
        let mut found = zero();
        for change in &amendments {
            if !operations.insert(change.operation) {
                return Err(CorrectionError::Duplicate);
            }
            if change.stock_revision.value() <= revision.value() || change.before != effective {
                return Err(CorrectionError::InvalidHistory);
            }
            effective = match change.delta {
                DepartureQuantityDelta::AdditionalDeparture(packs) => {
                    positive(packs)?;
                    additional = add(additional, packs)?;
                    add(effective, packs)?
                }
                DepartureQuantityDelta::IdentifiedFoundRemainder(packs) => {
                    positive(packs)?;
                    found = add(found, packs)?;
                    sub(effective, packs)?
                }
            };
            if change.after != effective {
                return Err(CorrectionError::InvalidHistory);
            }
            revision = change.stock_revision;
        }
        // Current established returns may have followed an earlier upward
        // amendment, so compare them with the final effective quantity only.
        if established_returns.value() > effective.value() {
            return Err(CorrectionError::ReturnConflict);
        }
        Ok(Self {
            original,
            amendments,
            effective,
            additional,
            found,
            established_returns,
        })
    }
    #[must_use]
    pub const fn original(&self) -> OriginalDepartureQuantity {
        self.original
    }
    #[must_use]
    pub fn amendments(&self) -> &[DepartureQuantityAmendment] {
        &self.amendments
    }
    #[must_use]
    pub const fn effective(&self) -> WholePacks {
        self.effective
    }
    #[must_use]
    pub const fn additional_departure(&self) -> WholePacks {
        self.additional
    }
    #[must_use]
    pub const fn found_remainder(&self) -> WholePacks {
        self.found
    }
    #[must_use]
    pub const fn established_returns(&self) -> WholePacks {
        self.established_returns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OriginalCountLossQuantity {
    pub reconciliation: HubRecordId,
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub packs: WholePacks,
}
/// A distinct native known-departure explanation of one exact counted shortage.
/// Its own operation is the new known-departure event. It cannot link a second
/// physical debit. It consumes classification allowance, never physical/work allowance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountLossExplanation {
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub packs: WholePacks,
}
/// Correct an earlier classification only. No stock/work/floor effect follows.
/// Native review must preserve the original and the explicit corrected facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountExplanationAmendment {
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub explanation: HubRecordId,
    pub before: WholePacks,
    pub after: WholePacks,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountExplanationReturns {
    pub explanation: HubRecordId,
    pub packs: WholePacks,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveCountExplanation {
    pub original: CountLossExplanation,
    pub packs: WholePacks,
    pub established_returns: WholePacks,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountLossQuantityAccount {
    original: OriginalCountLossQuantity,
    explanations: Vec<CountLossExplanation>,
    amendments: Vec<CountExplanationAmendment>,
    effective: Vec<EffectiveCountExplanation>,
    unexplained: WholePacks,
    explained: WholePacks,
}
impl CountLossQuantityAccount {
    pub fn restore(
        original: OriginalCountLossQuantity,
        explanations: Vec<CountLossExplanation>,
    ) -> Result<Self, CorrectionError> {
        Self::restore_corrected(original, explanations, Vec::new(), Vec::new())
    }
    /// Complete native classification history and current linked-return totals.
    /// Legacy restoration has no amended classification or return records.
    pub fn restore_corrected(
        original: OriginalCountLossQuantity,
        explanations: Vec<CountLossExplanation>,
        amendments: Vec<CountExplanationAmendment>,
        returns: Vec<CountExplanationReturns>,
    ) -> Result<Self, CorrectionError> {
        if explanations
            .len()
            .checked_add(amendments.len())
            .is_none_or(|n| n > HUB_CORRECTION_ROWS)
            || returns.len() > HUB_CORRECTION_ROWS
        {
            return Err(CorrectionError::Capacity);
        }
        positive(original.packs)?;
        if original.stock_revision == HubRevision::INITIAL {
            return Err(CorrectionError::InvalidHistory);
        }
        let mut operations = BTreeSet::from([original.operation]);
        let mut events = Vec::new();
        let mut revision = original.stock_revision;
        for (index, item) in explanations.iter().enumerate() {
            if item.stock_revision.value() <= revision.value() {
                return Err(CorrectionError::InvalidHistory);
            }
            revision = item.stock_revision;
            events.push((revision.value(), false, index));
        }
        revision = original.stock_revision;
        for (index, item) in amendments.iter().enumerate() {
            if item.stock_revision.value() <= revision.value() {
                return Err(CorrectionError::InvalidHistory);
            }
            revision = item.stock_revision;
            events.push((revision.value(), true, index));
        }
        events.sort_unstable();
        let mut quantities = BTreeMap::new();
        revision = original.stock_revision;
        let mut explained = zero();
        for (at, amendment, index) in events {
            if at <= revision.value() {
                return Err(CorrectionError::InvalidHistory);
            }
            revision = HubRevision::from_value(at).map_err(|_| CorrectionError::InvalidHistory)?;
            let operation = if amendment {
                let item = amendments[index];
                let current = quantities
                    .get_mut(&item.explanation)
                    .ok_or(CorrectionError::InvalidHistory)?;
                if *current != item.before || item.before == item.after {
                    return Err(CorrectionError::InvalidHistory);
                }
                explained = add(sub(explained, *current)?, item.after)?;
                *current = item.after;
                item.operation
            } else {
                let item = explanations[index];
                positive(item.packs)?;
                quantities.insert(item.operation, item.packs);
                explained = add(explained, item.packs)?;
                item.operation
            };
            if !operations.insert(operation) {
                return Err(CorrectionError::Duplicate);
            }
            if explained.value() > original.packs.value() {
                return Err(CorrectionError::InvalidQuantity);
            }
        }
        let mut reservations = BTreeMap::new();
        for reservation in returns {
            let current = quantities
                .get(&reservation.explanation)
                .ok_or(CorrectionError::InvalidHistory)?;
            if reservations
                .insert(reservation.explanation, reservation.packs)
                .is_some()
            {
                return Err(CorrectionError::Duplicate);
            }
            if reservation.packs.value() > current.value() {
                return Err(CorrectionError::ReturnConflict);
            }
        }
        let effective = explanations
            .iter()
            .map(|item| EffectiveCountExplanation {
                original: *item,
                packs: quantities[&item.operation],
                established_returns: reservations
                    .get(&item.operation)
                    .copied()
                    .unwrap_or_else(zero),
            })
            .collect();
        let unexplained = sub(original.packs, explained)?;
        Ok(Self {
            original,
            explanations,
            amendments,
            effective,
            unexplained,
            explained,
        })
    }
    #[must_use]
    pub const fn original(&self) -> OriginalCountLossQuantity {
        self.original
    }
    #[must_use]
    pub fn explanations(&self) -> &[CountLossExplanation] {
        &self.explanations
    }
    #[must_use]
    pub fn amendments(&self) -> &[CountExplanationAmendment] {
        &self.amendments
    }
    #[must_use]
    pub fn effective_explanations(&self) -> &[EffectiveCountExplanation] {
        &self.effective
    }
    #[must_use]
    pub const fn unexplained(&self) -> WholePacks {
        self.unexplained
    }
    #[must_use]
    pub const fn explained(&self) -> WholePacks {
        self.explained
    }
}

/// Complete native trace inputs, not a client-supplied adjustment proposal.
/// Additional departures already reduced on_hand; found remainders already added
/// separately identified on_hand. A count explanation does neither. Returned
/// intakes belong to their own population and are not credited into this trace.
pub struct CorrectedTraceQuantityContext<'a> {
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub original: WholePacks,
    pub on_hand: WholePacks,
    pub disposed: WholePacks,
    pub departures: &'a [DepartureQuantityAccount],
    pub actual_departures: &'a [DepartureQuantityAccount],
    pub count_losses: &'a [CountLossQuantityAccount],
}
impl CorrectedTraceQuantityContext<'_> {
    pub fn balance(&self) -> Result<TraceBalance, CorrectionError> {
        if self.departures.len() > super::portions::HUB_LINEAGE_GROUPS
            || self.actual_departures.len() > super::actuals::HUB_ACTUAL_REPORT_CAPACITY
            || self.count_losses.len() > super::counts::HUB_COUNT_CAPACITY
        {
            return Err(CorrectionError::Capacity);
        }
        let mut correction_rows = 0_usize;
        for rows in self
            .departures
            .iter()
            .chain(self.actual_departures)
            .map(|a| a.amendments.len())
            .chain(
                self.count_losses
                    .iter()
                    .flat_map(|a| [a.explanations.len(), a.amendments.len()]),
            )
        {
            correction_rows = correction_rows
                .checked_add(rows)
                .ok_or(CorrectionError::Capacity)?;
            if correction_rows > HUB_CORRECTION_ROWS {
                return Err(CorrectionError::Capacity);
            }
        }
        positive(self.original)?;
        let mut lines = BTreeSet::new();
        let mut departure_operations = BTreeMap::new();
        let mut revisions = BTreeMap::new();
        let mut amendment_operations = BTreeSet::new();
        let mut totals = [zero(), zero()];
        for (family, accounts) in [self.departures, self.actual_departures]
            .into_iter()
            .enumerate()
        {
            for account in accounts {
                let original = account.original;
                if original.trace != self.trace || original.variant != self.variant {
                    return Err(CorrectionError::InvalidHistory);
                }
                if !lines.insert((original.operation, original.group)) {
                    return Err(CorrectionError::Duplicate);
                }
                if departure_operations
                    .insert(original.operation, (family, original.stock_revision))
                    .is_some_and(|prior| prior != (family, original.stock_revision))
                {
                    return Err(CorrectionError::Duplicate);
                }
                record_revision(&mut revisions, original.stock_revision, original.operation)?;
                for change in account.amendments() {
                    record_revision(&mut revisions, change.stock_revision, change.operation)?;
                    if !amendment_operations.insert(change.operation) {
                        return Err(CorrectionError::Duplicate);
                    }
                }
                totals[family] = add(totals[family], account.effective)?;
            }
        }
        if amendment_operations
            .iter()
            .any(|op| departure_operations.contains_key(op))
        {
            return Err(CorrectionError::Duplicate);
        }
        let mut losses = BTreeSet::new();
        let mut explanations = BTreeSet::new();
        let mut count_operations = BTreeSet::new();
        let mut unexplained = zero();
        for account in self.count_losses {
            if account.original.trace != self.trace || account.original.variant != self.variant {
                return Err(CorrectionError::InvalidHistory);
            }
            if !losses.insert(account.original.reconciliation) {
                return Err(CorrectionError::Duplicate);
            }
            if !count_operations.insert(account.original.operation)
                || departure_operations.contains_key(&account.original.operation)
                || amendment_operations.contains(&account.original.operation)
            {
                return Err(CorrectionError::Duplicate);
            }
            record_revision(
                &mut revisions,
                account.original.stock_revision,
                account.original.operation,
            )?;
            for item in &account.explanations {
                record_revision(&mut revisions, item.stock_revision, item.operation)?;
                if !explanations.insert(item.operation)
                    || amendment_operations.contains(&item.operation)
                    || departure_operations.contains_key(&item.operation)
                {
                    return Err(CorrectionError::Duplicate);
                }
            }
            for item in &account.amendments {
                record_revision(&mut revisions, item.stock_revision, item.operation)?;
                if !amendment_operations.insert(item.operation)
                    || explanations.contains(&item.operation)
                    || count_operations.contains(&item.operation)
                    || departure_operations.contains_key(&item.operation)
                {
                    return Err(CorrectionError::Duplicate);
                }
            }
            unexplained = add(unexplained, account.unexplained)?;
            totals[1] = add(totals[1], account.explained)?;
        }
        if explanations.iter().any(|op| count_operations.contains(op)) {
            return Err(CorrectionError::Duplicate);
        }
        if add(
            add(
                add(add(self.on_hand, self.disposed)?, totals[0])?,
                totals[1],
            )?,
            unexplained,
        )? != self.original
        {
            return Err(CorrectionError::Accounting);
        }
        Ok(TraceBalance {
            variant: self.variant,
            original: self.original,
            on_hand: self.on_hand,
            departed: totals[0],
            counted_loss: unexplained,
            disposed: self.disposed,
            actual_departed: totals[1],
        })
    }
}
fn record_revision(
    revisions: &mut BTreeMap<u64, HubRecordId>,
    revision: HubRevision,
    operation: HubRecordId,
) -> Result<(), CorrectionError> {
    if revisions
        .insert(revision.value(), operation)
        .is_some_and(|prior| prior != operation)
    {
        Err(CorrectionError::InvalidHistory)
    } else {
        Ok(())
    }
}
fn zero() -> WholePacks {
    WholePacks::from_value(0).expect("zero packs")
}
fn positive(packs: WholePacks) -> Result<(), CorrectionError> {
    if packs.value() == 0 {
        Err(CorrectionError::InvalidQuantity)
    } else {
        Ok(())
    }
}
fn add(a: WholePacks, b: WholePacks) -> Result<WholePacks, CorrectionError> {
    a.checked_add(b)
        .map_err(|_| CorrectionError::InvalidQuantity)
}
fn sub(a: WholePacks, b: WholePacks) -> Result<WholePacks, CorrectionError> {
    a.checked_sub(b)
        .map_err(|_| CorrectionError::InvalidQuantity)
}
#[cfg(test)]
mod tests;
