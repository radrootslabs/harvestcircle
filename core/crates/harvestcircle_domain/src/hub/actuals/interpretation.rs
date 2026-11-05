//! Original-bound current physical-event interpretations. This data is neither
//! a complete native inventory by itself nor a grant to repeat a physical effect.
use super::*;
use crate::hub::corrections::{
    CorrectionKnownDeparture, CountLossQuantityAccount, DepartureQuantityAccount,
    HUB_CORRECTION_ROWS, KnownCorrectionQuantity, KnownRecipients, OriginalRecipientAttribution,
    RecipientAttributionCorrection,
};
use std::collections::BTreeMap;

/// Complete original shortage and shared classification history. This never
/// represents a physical outflow, birth or Found credit.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualKnownCountExplanation {
    pub count: crate::hub::counts::CountPostPlan,
    pub account: CountLossQuantityAccount,
}

/// One population/variant of an immutable event and its current interpretation.
/// Native adapters must independently establish original/history completeness.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualKnownInterpretation {
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub latest_recorded_at: UnixTimestamp,
    pub original: ActualKnownDeparture,
    pub recipient_original: OriginalRecipientAttribution,
    pub recipient_history: Vec<RecipientAttributionCorrection>,
    pub quantities: Vec<DepartureQuantityAccount>,
    /// Absent on every physical20 record, preserving its exact capture encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_explanation: Option<Box<ActualKnownCountExplanation>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualKnownInterpretationView {
    pub recipients: KnownRecipients,
    pub original_packs: WholePacks,
    pub effective_packs: WholePacks,
}
impl ActualKnownInterpretationView {
    pub fn could_match_packs(&self, packs: Option<WholePacks>) -> bool {
        packs.is_none_or(|value| value == self.original_packs || value == self.effective_packs)
    }
}

impl ActualKnownInterpretation {
    /// Complete captured facts, purpose separated from old actual-result seals.
    /// Recomputing this public consistency digest cannot create a native grant.
    pub fn digest(&self) -> Result<[u8; 32], ActualReportError> {
        self.view()?;
        super::result::interpretation_digest(self)
    }

    pub(super) fn operations(&self) -> impl Iterator<Item = (HubRecordId, HubRevision)> + '_ {
        std::iter::once((self.original.operation, self.original.stock_revision))
            .chain(
                self.recipient_history
                    .iter()
                    .map(|r| (r.stamp.operation, r.stamp.stock_revision)),
            )
            .chain(self.quantities.iter().flat_map(|line| {
                line.amendments()
                    .iter()
                    .map(|r| (r.operation, r.stock_revision))
            }))
            .chain(self.count_explanation.iter().flat_map(|value| {
                std::iter::once((value.count.operation, value.count.stock_revision))
                    .chain(std::iter::once((
                        value.count.count.operation,
                        value.count.count.recorded_revision,
                    )))
                    .chain(
                        value
                            .account
                            .explanations()
                            .iter()
                            .filter(|row| row.operation != self.original.operation)
                            .map(|row| (row.operation, row.stock_revision)),
                    )
                    .chain(
                        value
                            .account
                            .amendments()
                            .iter()
                            .map(|row| (row.operation, row.stock_revision)),
                    )
            }))
    }

    pub fn view(&self) -> Result<ActualKnownInterpretationView, ActualReportError> {
        if self.original.lines.len() > crate::hub::dispatch::HUB_DISPATCH_LINES
            || self.quantities.len() > crate::hub::dispatch::HUB_DISPATCH_LINES
            || self.recipient_history.len() > HUB_CORRECTION_ROWS
        {
            return Err(ActualReportError::Capacity);
        }
        let rows = self
            .quantities
            .iter()
            .try_fold(self.recipient_history.len(), |n, line| {
                n.checked_add(line.amendments().len())
                    .ok_or(ActualReportError::Capacity)
            })?;
        let rows = self
            .count_explanation
            .as_ref()
            .map_or(Some(rows), |value| {
                rows.checked_add(value.account.explanations().len())?
                    .checked_add(value.account.amendments().len())
            })
            .ok_or(ActualReportError::Capacity)?;
        if rows > HUB_CORRECTION_ROWS {
            return Err(ActualReportError::Capacity);
        }
        let quantity = match self.count_explanation.as_deref() {
            Some(value) if self.quantities.is_empty() => {
                KnownCorrectionQuantity::CountExplanation {
                    count: &value.count,
                    account: &value.account,
                }
            }
            Some(_) => return Err(ActualReportError::InvalidHistory),
            None => KnownCorrectionQuantity::Physical(&self.quantities),
        };
        let recipients = CorrectionKnownDeparture {
            original: &self.original,
            recipient_original: &self.recipient_original,
            recipient_history: &self.recipient_history,
            quantity,
        }
        .associations_through(
            self.original.hub,
            self.stock_revision,
            self.catalog_revision,
            self.latest_recorded_at,
        )
        .map_err(|_| ActualReportError::InvalidHistory)?;
        let mut ids = BTreeSet::new();
        let mut revisions = BTreeMap::new();
        let mut operations = BTreeMap::new();
        for (id, revision) in self.operations() {
            if !ids.insert(id) {
                return Err(ActualReportError::InvalidHistory);
            }
            observe_revision(&mut revisions, &mut operations, id, revision)?;
        }
        let effective_packs = if let Some(value) = self.count_explanation.as_deref() {
            value
                .account
                .effective_explanations()
                .iter()
                .find(|row| row.original.operation == self.original.operation)
                .ok_or(ActualReportError::InvalidHistory)?
                .packs
        } else {
            self.quantities.iter().try_fold(
                WholePacks::from_value(0).expect("zero"),
                |sum, line| {
                    sum.checked_add(line.effective())
                        .map_err(|_| ActualReportError::InvalidHistory)
                },
            )?
        };
        Ok(ActualKnownInterpretationView {
            recipients,
            original_packs: self.original.packs()?,
            effective_packs,
        })
    }
}

/// A complete captured cut is retained even when no known event exists. Native
/// producers must establish completeness independently of these descriptive facts.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualKnownInterpretations {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub latest_recorded_at: UnixTimestamp,
    #[serde(serialize_with = "physical20_records")]
    pub records: Vec<ActualKnownInterpretation>,
}

// The complete native20 inventory codec remains physical-only. A future native
// count producer must first supply a versioned, deduplicated account table; it
// cannot persist quadratic copies through this legacy capture codec.
fn physical20_records<S: serde::Serializer>(
    records: &[ActualKnownInterpretation],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if records
        .iter()
        .any(|record| record.count_explanation.is_some())
    {
        return Err(serde::ser::Error::custom(
            "count inventory requires shared account capture",
        ));
    }
    serde::Serialize::serialize(records, serializer)
}

/// The qualified path never treats an empty interpretation array as permission
/// to omit nonempty native originals. Legacy callers use the separate old path.
pub(super) fn validate_set(
    context: &ActualNewDepartureContext<'_>,
    set: &ActualKnownInterpretations,
    operation: HubRecordId,
    now: UnixTimestamp,
) -> Result<Vec<ActualKnownInterpretationView>, ActualReportError> {
    let known = context.known_departures;
    let records = &set.records;
    let hub = context.review.hub;
    let stock = context.review.stock_revision;
    let catalog = context.review.catalog.revision();
    if set.hub != hub
        || set.stock_revision != stock
        || set.catalog_revision != catalog
        || set.latest_recorded_at < context.review.original.recorded_at
        || set.latest_recorded_at > now
    {
        return Err(ActualReportError::InvalidNativeContext);
    }
    if records.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(ActualReportError::Capacity);
    }
    if records.len() != known.len() {
        return Err(ActualReportError::InvalidNativeContext);
    }
    let mut keys = BTreeSet::new();
    let mut events: BTreeMap<HubRecordId, &ActualKnownInterpretation> = BTreeMap::new();
    let mut groups = BTreeMap::new();
    let mut counts: BTreeMap<HubRecordId, &ActualKnownCountExplanation> = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut revisions = BTreeMap::new();
    let mut operation_revisions = BTreeMap::new();
    observe_revision(
        &mut revisions,
        &mut operation_revisions,
        context.review.original.operation,
        context.review.original.stock_revision,
    )?;
    let mut changes = BTreeSet::new();
    for (index, original) in known.iter().enumerate() {
        if original.operation == context.review.original.operation
            || original.operation == operation
        {
            return Err(ActualReportError::InvalidIdentity);
        }
        observe_revision(
            &mut revisions,
            &mut operation_revisions,
            original.operation,
            original.stock_revision,
        )?;
        let role = records[index]
            .count_explanation
            .as_deref()
            .map_or((0_u8, original.operation, None), |value| {
                (5, value.count.operation, None)
            });
        roles.insert(original.operation, role);
    }
    let mut views = Vec::with_capacity(records.len());
    for (original, record) in known.iter().zip(records) {
        if record.original != *original
            || original.hub != hub
            || record.stock_revision != stock
            || record.catalog_revision != catalog
            || record.latest_recorded_at != set.latest_recorded_at
            || !keys.insert((original.operation, original.trace, original.variant.id))
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        if events
            .insert(original.operation, record)
            .is_some_and(|prior| {
                prior.recipient_original != record.recipient_original
                    || prior.recipient_history != record.recipient_history
                    || prior.original.actor != original.actor
                    || prior.original.hub != original.hub
                    || prior.original.stock_revision != original.stock_revision
                    || prior.original.catalog_revision != original.catalog_revision
                    || prior.original.recorded_at != original.recorded_at
                    || prior.original.occurred != original.occurred
            })
        {
            return Err(ActualReportError::InvalidHistory);
        }
        let view = record.view()?;
        if let Some(value) = record.count_explanation.as_deref()
            && counts
                .insert(value.count.operation, value)
                .is_some_and(|prior| prior != value)
        {
            return Err(ActualReportError::InvalidHistory);
        }
        for (id, revision, role) in record
            .recipient_history
            .iter()
            .map(|r| {
                (
                    r.stamp.operation,
                    r.stamp.stock_revision,
                    (1, original.operation, None),
                )
            })
            .chain(record.quantities.iter().flat_map(|line| {
                line.amendments().iter().map(|r| {
                    (
                        r.operation,
                        r.stock_revision,
                        (2, original.operation, Some(line.original().group)),
                    )
                })
            }))
            .chain(record.count_explanation.iter().flat_map(|value| {
                std::iter::once((
                    value.count.operation,
                    value.count.stock_revision,
                    (3, value.count.operation, None),
                ))
                .chain(std::iter::once((
                    value.count.count.operation,
                    value.count.count.recorded_revision,
                    (6, value.count.operation, None),
                )))
                .chain(value.account.explanations().iter().map(|row| {
                    (
                        row.operation,
                        row.stock_revision,
                        (5, value.count.operation, None),
                    )
                }))
                .chain(value.account.amendments().iter().map(|row| {
                    (
                        row.operation,
                        row.stock_revision,
                        (4, value.count.operation, Some(row.explanation)),
                    )
                }))
            }))
        {
            if id == operation || id == context.review.original.operation {
                return Err(ActualReportError::InvalidIdentity);
            }
            observe_revision(&mut revisions, &mut operation_revisions, id, revision)?;
            if roles.insert(id, role).is_some_and(|prior| prior != role) {
                return Err(ActualReportError::InvalidHistory);
            }
            if !matches!(role.0, 3 | 6) {
                changes.insert(id);
            }
            if changes.len() > HUB_CORRECTION_ROWS {
                return Err(ActualReportError::Capacity);
            }
        }
        for line in &original.lines {
            let key = (original.operation, line.group);
            if groups
                .insert(key, (original.trace, original.variant.id))
                .is_some()
            {
                return Err(ActualReportError::InvalidHistory);
            }
        }
        // Eagerly validate all entries before the caller selects a duplicate.
        views.push(view);
    }
    Ok(views)
}

fn observe_revision(
    revisions: &mut BTreeMap<u64, HubRecordId>,
    operations: &mut BTreeMap<HubRecordId, u64>,
    operation: HubRecordId,
    revision: HubRevision,
) -> Result<(), ActualReportError> {
    if revisions
        .insert(revision.value(), operation)
        .is_some_and(|prior| prior != operation)
        || operations
            .insert(operation, revision.value())
            .is_some_and(|prior| prior != revision.value())
    {
        return Err(ActualReportError::InvalidHistory);
    }
    Ok(())
}
