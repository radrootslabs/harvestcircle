//! Original identity plus complete effective interpretation; zero and unknown
//! remain retrievable duplicate candidates, never an excuse for a fresh event ID.
use super::*;
use crate::hub::counts::{CountEffect, CountPostPlan};

#[derive(Clone, Copy, Debug)]
pub enum KnownCorrectionQuantity<'a> {
    Physical(&'a [DepartureQuantityAccount]),
    CountExplanation {
        count: &'a CountPostPlan,
        account: &'a CountLossQuantityAccount,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct CorrectionKnownDeparture<'a> {
    pub original: &'a ActualKnownDeparture,
    pub recipient_original: &'a OriginalRecipientAttribution,
    pub recipient_history: &'a [RecipientAttributionCorrection],
    pub quantity: KnownCorrectionQuantity<'a>,
}
pub(super) struct KnownCut<'a> {
    pub hub: HubId,
    pub stock: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub latest: UnixTimestamp,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownRecipients {
    parties: BTreeSet<HubRecordId>,
    unresolved: bool,
}
impl KnownRecipients {
    pub fn could_match(&self, recipient: Option<HubRecordId>) -> bool {
        self.unresolved || recipient.is_none_or(|id| self.parties.contains(&id))
    }
    pub fn parties(&self) -> impl Iterator<Item = HubRecordId> + '_ {
        self.parties.iter().copied()
    }
    pub const fn has_unknown(&self) -> bool {
        self.unresolved
    }
}
impl CorrectionKnownDeparture<'_> {
    pub(super) fn validate(
        &self,
        cut: &KnownCut<'_>,
    ) -> Result<KnownRecipients, QuantityEffectError> {
        self.associations_through(cut.hub, cut.stock, cut.catalog.revision(), cut.latest)
    }

    /// Checked original/current associations at a native supplied cut. These
    /// facts neither establish native completeness nor authorize an effect.
    pub fn associations_through(
        &self,
        hub: HubId,
        stock: HubRevision,
        catalog: HubRevision,
        latest: UnixTimestamp,
    ) -> Result<KnownRecipients, QuantityEffectError> {
        let original = self.original;
        original.validate().map_err(QuantityEffectError::Actual)?;
        let recipient = self.recipient_original;
        if original.hub != hub
            || original.stock_revision.value() > stock.value()
            || original.catalog_revision.value() > catalog.value()
            || original.recorded_at > latest
            || recipient.hub != original.hub
            || recipient.operation != original.operation
            || recipient.stock_revision != original.stock_revision
            || recipient.catalog_revision != original.catalog_revision
            || recipient.actor != original.actor
            || recipient.recorded_at != original.recorded_at
            || recipient.recipient != original.recipient
            || !recipient.populations.contains(&original.trace)
            || self.recipient_history.iter().any(|r| {
                r.stamp.stock_revision.value() > stock.value()
                    || r.stamp.catalog_revision.value() > catalog.value()
                    || r.stamp.recorded_at > latest
            })
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        current_recipient_attribution(recipient, self.recipient_history)
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        match self.quantity {
            KnownCorrectionQuantity::Physical(accounts) => {
                if recipient.family == CorrectedDepartureFamily::ExplainedCountLoss
                    || accounts.len() != original.lines.len()
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                let mut groups = BTreeSet::new();
                for account in accounts {
                    let line = account.original();
                    if line.operation != original.operation
                        || line.stock_revision != original.stock_revision
                        || line.trace != original.trace
                        || line.variant != original.variant.id
                        || !groups.insert(line.group)
                        || !original
                            .lines
                            .iter()
                            .any(|l| l.group == line.group && l.packs == line.packs)
                        || account
                            .amendments()
                            .iter()
                            .any(|a| a.stock_revision.value() > stock.value())
                    {
                        return Err(QuantityEffectError::InvalidHistory);
                    }
                }
            }
            KnownCorrectionQuantity::CountExplanation { count, account } => {
                count
                    .validate()
                    .map_err(|_| QuantityEffectError::InvalidHistory)?;
                let loss = account.original();
                let CountEffect::Shortage { packs, .. } = count.effect else {
                    return Err(QuantityEffectError::InvalidHistory);
                };
                let explained = account
                    .effective_explanations()
                    .iter()
                    .find(|e| e.original.operation == original.operation)
                    .ok_or(QuantityEffectError::InvalidHistory)?;
                if recipient.family != CorrectedDepartureFamily::ExplainedCountLoss
                    || count.count.scope.hub != hub
                    || loss.reconciliation != count.id
                    || loss.operation != count.operation
                    || loss.stock_revision != count.stock_revision
                    || loss.trace != original.trace
                    || loss.variant != original.variant.id
                    || loss.packs != packs
                    || count.before.trace != loss.trace
                    || count.before.variant != loss.variant
                    || explained.original.stock_revision != original.stock_revision
                    || original.lines.len() != 1
                    || original.lines[0].group != count.before.id
                    || original.lines[0].packs != explained.original.packs
                    || count.recorded_at > original.recorded_at
                    || count.catalog_revision.value() > original.catalog_revision.value()
                    || count.recorded_at > latest
                    || count.catalog_revision.value() > catalog.value()
                    || account
                        .explanations()
                        .iter()
                        .any(|e| e.stock_revision.value() > stock.value())
                    || account
                        .amendments()
                        .iter()
                        .any(|a| a.stock_revision.value() > stock.value())
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
            }
        }
        let mut result = KnownRecipients {
            parties: BTreeSet::from([original.recipient.id]),
            unresolved: false,
        };
        for record in self.recipient_history {
            for recipient in [&record.before, &record.after] {
                match recipient {
                    AmendedRecipient::Known(r) => {
                        result.parties.insert(r.id);
                    }
                    AmendedRecipient::Unknown => result.unresolved = true,
                }
            }
        }
        Ok(result)
    }
}
