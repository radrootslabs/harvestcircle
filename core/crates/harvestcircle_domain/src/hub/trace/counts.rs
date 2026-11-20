//! Count interpretation changes meaning, never the original physical shortage.
//! These checks do not authenticate native prefix completeness or grant effects.
use super::*;
use crate::hub::{actuals::ActualKnownInterpretation, corrections::CountLossQuantityAccount};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    Departure(HubRecordId),
    Actual(HubRecordId),
    Report(HubRecordId),
    CountObserve(HubRecordId),
    CountPost(HubRecordId, HubRecordId, HubRecordId, HubRecordId),
    Explanation(HubRecordId, HubRecordId, HubRecordId),
    Classification(HubRecordId),
    Quantity(HubRecordId, HubRecordId),
    Source(HubRecordId),
    Recipient(HubRecordId),
    Disposition(HubRecordId),
    Return(HubRecordId, [u8; 32]),
    Association(HubRecordId, HubRecordId),
}

#[derive(Default)]
struct Roots {
    operations: BTreeMap<HubRecordId, (HubRevision, Role)>,
    revisions: BTreeMap<u64, HubRecordId>,
}
impl Roots {
    fn insert(
        &mut self,
        operation: HubRecordId,
        revision: HubRevision,
        role: Role,
    ) -> Result<(), TraceError> {
        if self
            .operations
            .insert(operation, (revision, role))
            .is_some_and(|old| old != (revision, role))
            || self
                .revisions
                .insert(revision.value(), operation)
                .is_some_and(|old| old != operation)
        {
            return Err(TraceError::InvalidSnapshot);
        }
        Ok(())
    }
}

impl TraceSnapshot {
    fn count_root_identities(&self) -> Result<(), TraceError> {
        let mut roots = Roots::default();
        for exit in &self.exits {
            roots.insert(
                exit.departure.operation,
                exit.departure.stock_revision,
                Role::Departure(exit.departure.id),
            )?;
        }
        for actual in &self.actual_departures {
            roots.insert(
                actual.operation,
                actual.stock_revision,
                Role::Actual(actual.report.id),
            )?;
            roots.insert(
                actual.report.operation,
                actual.report.stock_revision,
                Role::Report(actual.report.id),
            )?;
        }
        for account in &self.quantity_corrections {
            for amendment in account.amendments() {
                roots.insert(
                    amendment.operation,
                    amendment.stock_revision,
                    Role::Quantity(account.original().operation, account.original().group),
                )?;
            }
        }
        for loss in &self.count_losses {
            roots.insert(
                loss.operation,
                loss.stock_revision,
                Role::CountPost(
                    loss.reconciliation,
                    self.trace.id,
                    self.trace.variant.id,
                    loss.group,
                ),
            )?;
        }
        for disposition in &self.dispositions {
            roots.insert(
                disposition.operation,
                disposition.stock_revision,
                Role::Disposition(disposition.population),
            )?;
        }
        for correction in &self.source_corrections {
            roots.insert(
                correction.stamp.operation,
                correction.stamp.stock_revision,
                Role::Source(correction.original.trace),
            )?;
        }
        for correction in &self.recipient_corrections {
            roots.insert(
                correction.stamp.operation,
                correction.stamp.stock_revision,
                Role::Recipient(correction.original.operation),
            )?;
        }
        let mut shared = BTreeSet::new();
        if let Some(inventory) = &self.count_interpretations {
            for record in &inventory.facts().records {
                let value = record
                    .count_explanation
                    .as_ref()
                    .ok_or(TraceError::InvalidSnapshot)?;
                roots.insert(
                    record.original.operation,
                    record.original.stock_revision,
                    Role::Explanation(
                        value.count.operation,
                        value.count.before.trace,
                        value.count.before.variant,
                    ),
                )?;
                if shared.insert(value.count.operation) {
                    roots.insert(
                        value.count.count.operation,
                        value.count.count.recorded_revision,
                        Role::CountObserve(value.count.count.id),
                    )?;
                    roots.insert(
                        value.count.operation,
                        value.count.stock_revision,
                        Role::CountPost(
                            value.count.id,
                            value.count.before.trace,
                            value.count.before.variant,
                            value.count.before.id,
                        ),
                    )?;
                    for explanation in value.account.explanations() {
                        roots.insert(
                            explanation.operation,
                            explanation.stock_revision,
                            Role::Explanation(
                                value.count.operation,
                                value.count.before.trace,
                                value.count.before.variant,
                            ),
                        )?;
                    }
                    for amendment in value.account.amendments() {
                        roots.insert(
                            amendment.operation,
                            amendment.stock_revision,
                            Role::Classification(amendment.explanation),
                        )?;
                    }
                }
            }
        }
        for association in &self.count_return_associations {
            roots.insert(
                association.operation,
                association.stock_revision,
                Role::Association(
                    association.return_operation,
                    association.explanation_operation,
                ),
            )?;
            roots.insert(
                association.return_operation,
                association.return_stock_revision,
                Role::Return(
                    association.returned_population,
                    association.return_original_digest,
                ),
            )?;
            roots.insert(
                association.explanation_operation,
                association.explanation_stock_revision,
                Role::Explanation(
                    association.count_operation,
                    association.explanation_trace,
                    association.variant,
                ),
            )?;
            roots.insert(
                association.count_operation,
                association.count_stock_revision,
                Role::CountPost(
                    crate::hub::counts::count_reconciliation_id(association.count_operation)
                        .map_err(|_| TraceError::InvalidSnapshot)?,
                    association.explanation_trace,
                    association.variant,
                    association.anchor_group,
                ),
            )?;
        }
        Ok(())
    }

    pub(super) fn count_accounts(
        &self,
    ) -> Result<BTreeMap<HubRecordId, &CountLossQuantityAccount>, TraceError> {
        if self.count_return_associations.len() > crate::hub::corrections::HUB_CORRECTION_ROWS
            || self.source_corrections.len() > crate::hub::corrections::HUB_CORRECTION_ROWS
            || self.recipient_corrections.len() > crate::hub::corrections::HUB_CORRECTION_ROWS
            || self.quantity_corrections.len() > crate::hub::corrections::HUB_CORRECTION_ROWS
            || self.count_losses.len() > crate::hub::counts::HUB_COUNT_CAPACITY
            || self.actual_departures.len() > crate::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY
            || self.exits.len() > crate::hub::portions::HUB_LINEAGE_GROUPS
            || self.dispositions.len() > crate::hub::portions::HUB_LINEAGE_GROUPS
        {
            return Err(TraceError::Capacity);
        }
        let mut accounts = BTreeMap::new();
        let Some(inventory) = &self.count_interpretations else {
            return if self.count_return_associations.is_empty() && !self.count_return_uncertain {
                Ok(accounts)
            } else {
                Err(TraceError::InvalidSnapshot)
            };
        };
        let facts = inventory.facts();
        self.count_root_identities()?;
        if facts.hub != self.hub
            || facts.stock_revision != self.stock_revision
            || facts.catalog_revision != self.catalog_revision
        {
            return Err(TraceError::InvalidSnapshot);
        }
        let mut events: BTreeMap<HubRecordId, &ActualKnownInterpretation> = BTreeMap::new();
        for record in &facts.records {
            let value = record
                .count_explanation
                .as_ref()
                .ok_or(TraceError::InvalidSnapshot)?;
            let count = &value.count;
            let loss = self
                .count_losses
                .iter()
                .find(|loss| loss.operation == count.operation)
                .ok_or(TraceError::InvalidSnapshot)?;
            let crate::hub::counts::CountEffect::Shortage { packs, .. } = count.effect else {
                return Err(TraceError::InvalidSnapshot);
            };
            if record.original.trace != self.trace.id
                || record.original.variant.id != self.trace.variant.id
                || count.before.trace != self.trace.id
                || count.before.variant != self.trace.variant.id
                || count.before.id != loss.group
                || count.id != loss.reconciliation
                || count.count.id != loss.count
                || count.stock_revision != loss.stock_revision
                || count.confirmed_on_hand_at != loss.confirmed_on_hand_at
                || count.recorded_at != loss.recorded_at
                || count.actor != loss.actor
                || count.reason != loss.reason
                || packs != loss.packs
                || value.account.original().packs != loss.packs
                || record.recipient_history
                    != self
                        .recipient_corrections
                        .iter()
                        .filter(|row| row.original.operation == record.original.operation)
                        .cloned()
                        .collect::<Vec<_>>()
                || events.insert(record.original.operation, record).is_some()
            {
                return Err(TraceError::InvalidSnapshot);
            }
            if let Some(prior) = accounts.insert(count.operation, &value.account)
                && prior != &value.account
            {
                return Err(TraceError::InvalidSnapshot);
            }
        }
        let mut operations = BTreeSet::new();
        let mut returns = BTreeSet::new();
        let mut prior = 0;
        let mut floors = BTreeMap::new();
        let mut claims = BTreeMap::new();
        for association in &self.count_return_associations {
            association
                .validate()
                .map_err(|_| TraceError::InvalidSnapshot)?;
            if association.hub != self.hub
                || association.variant != self.trace.variant.id
                || association.stock_revision.value() > self.stock_revision.value()
                || association.catalog_revision.value() > self.catalog_revision.value()
                || association.stock_revision.value() <= prior
                || !operations.insert(association.operation)
                || !returns.insert(association.return_operation)
                || (association.explanation_trace != self.trace.id
                    && association.returned_population != self.trace.id)
            {
                return Err(TraceError::InvalidSnapshot);
            }
            let claim = (
                association.count_operation,
                association.explanation_trace,
                association.variant,
                association.anchor_group,
                association.explanation_occurred,
                association.explanation_source_digest,
            );
            if claims
                .insert(association.explanation_operation, claim)
                .is_some_and(|old| old != claim)
            {
                return Err(TraceError::InvalidSnapshot);
            }
            prior = association.stock_revision.value();
            if association.explanation_trace == self.trace.id {
                let record = events
                    .get(&association.explanation_operation)
                    .ok_or(TraceError::InvalidSnapshot)?;
                let value = record
                    .count_explanation
                    .as_ref()
                    .ok_or(TraceError::InvalidSnapshot)?;
                if association.explanation_stock_revision != record.original.stock_revision
                    || association.count_operation != value.count.operation
                    || association.count_stock_revision != value.count.stock_revision
                    || association.anchor_group != value.count.before.id
                    || association.explanation_occurred != record.original.occurred
                {
                    return Err(TraceError::InvalidSnapshot);
                }
                let floor = floors
                    .entry(association.explanation_operation)
                    .or_insert_with(|| WholePacks::from_value(0).expect("zero"));
                *floor = floor
                    .checked_add(association.packs)
                    .map_err(|_| TraceError::Accounting)?;
            } else if self.trace.origin
                != (crate::hub::stock::PopulationOrigin::Returned {
                    returned: association.returned,
                })
                || association.packs != self.trace.original_packs
                || association.return_occurred != self.trace.occurred
            {
                return Err(TraceError::InvalidSnapshot);
            }
        }
        for account in accounts.values() {
            for effective in account.effective_explanations() {
                let floor = floors
                    .remove(&effective.original.operation)
                    .unwrap_or_else(|| WholePacks::from_value(0).expect("zero"));
                if effective.established_returns != floor {
                    return Err(TraceError::InvalidSnapshot);
                }
            }
        }
        if !floors.is_empty() {
            return Err(TraceError::InvalidSnapshot);
        }
        Ok(accounts)
    }
}

#[cfg(test)]
mod tests;
