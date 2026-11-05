//! Exact observed classification of an existing count explanation. This plan
//! changes descriptive quantity only; native completeness, fresh admission and
//! append-only atomic persistence remain separate prerequisites.
use super::*;
use crate::hub::actuals::{ActualReportSources, CountExplanationSourceWitness};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountClassificationCommand {
    pub observation_operation: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_account_prior: Option<HubRecordId>,
    pub expected_prior: Option<HubRecordId>,
}
pub struct CountClassificationContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub latest_recorded_at: UnixTimestamp,
    pub original: &'a CountExplanationPlan,
    pub account: &'a CountLossQuantityAccount,
    pub recipient_original: &'a OriginalRecipientAttribution,
    pub recipient_history: &'a [RecipientAttributionCorrection],
    pub observation: &'a CorrectionObservationRecord,
    pub observation_successor: Option<HubRecordId>,
    pub observation_used_by: Option<HubRecordId>,
    pub report: Option<&'a ActualReportRecord>,
    pub prior_report_review: Option<HubRecordId>,
    pub known_departures: Vec<CorrectionKnownDeparture<'a>>,
    pub uncertain_submissions: bool,
    pub unresolved_return_association: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountClassificationPlan {
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub observation: CorrectionObservationRecord,
    pub original: ActualKnownDeparture,
    pub count: crate::hub::counts::CountPostPlan,
    pub source: CountExplanationSourceWitness,
    pub report: Option<ActualReportRecord>,
    pub amendment: CountExplanationAmendment,
    pub before: CountLossQuantityAccount,
    pub after: CountLossQuantityAccount,
}
impl CountClassificationCommand {
    pub fn plan(
        &self,
        context: &CountClassificationContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CountClassificationPlan, QuantityEffectError> {
        if context.uncertain_submissions {
            return Err(QuantityEffectError::UncertainSubmission);
        }
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
        {
            return Err(QuantityEffectError::StaleRevision);
        }
        if context.observation_successor.is_some()
            || context.observation_used_by.is_some()
            || context.prior_report_review.is_some()
        {
            return Err(QuantityEffectError::AlreadyReviewed);
        }
        let original = &context.original.known_event;
        let observation = context.observation;
        let source = CountExplanationSourceWitness::from_explanation(context.original)
            .map_err(QuantityEffectError::Actual)?;
        let account = context.account;
        let account_head = account
            .explanations()
            .iter()
            .map(|row| (row.stock_revision, row.operation))
            .chain(
                account
                    .amendments()
                    .iter()
                    .map(|row| (row.stock_revision, row.operation)),
            )
            .max_by_key(|(r, _)| r.value())
            .map(|(_, id)| id);
        let target_head = account
            .amendments()
            .iter()
            .rfind(|row| row.explanation == original.operation)
            .map(|row| row.operation);
        if self.expected_account_prior != account_head || self.expected_prior != target_head {
            return Err(QuantityEffectError::StaleRevision);
        }
        let input = observation
            .observation
            .fingerprint(observation.hub, observation.actor, observation.operation)
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        correction_observation_result_digest(&input, observation)
            .map_err(|_| QuantityEffectError::InvalidHistory)?;
        let CorrectionObservedFacts::CountClassification {
            explanation_operation,
            corrected_packs,
            report,
            ..
        } = &observation.observation.facts
        else {
            return Err(QuantityEffectError::InvalidFacts);
        };
        if observation.operation != self.observation_operation
            || observation.hub != context.hub
            || original.hub != context.hub
            || *explanation_operation != original.operation
            || observation.stock_revision.value() > context.stock_revision.value()
            || observation.catalog_revision.value() > context.catalog.revision().value()
            || observation.recorded_at > context.latest_recorded_at
            || now < context.latest_recorded_at
            || report.is_some() != context.report.is_some()
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let recipients = CorrectionKnownDeparture {
            original,
            recipient_original: context.recipient_original,
            recipient_history: context.recipient_history,
            quantity: KnownCorrectionQuantity::CountExplanation {
                count: &context.original.count,
                account,
            },
        }
        .associations_through(
            context.hub,
            context.stock_revision,
            context.catalog.revision(),
            context.latest_recorded_at,
        )?;
        let target = account
            .effective_explanations()
            .iter()
            .find(|row| row.original.operation == original.operation)
            .ok_or(QuantityEffectError::InvalidHistory)?;
        if *corrected_packs == target.packs {
            return Err(QuantityEffectError::InvalidFacts);
        }
        if context.unresolved_return_association && corrected_packs.value() < target.packs.value() {
            return Err(QuantityEffectError::Accounting(
                CorrectionError::ReturnConflict,
            ));
        }
        let mut dependencies = vec![
            (
                context.original.count.operation,
                context.original.count.stock_revision,
            ),
            (
                context.original.count.count.operation,
                context.original.count.count.recorded_revision,
            ),
            (
                context.original.report.operation,
                context.original.report.stock_revision,
            ),
            (
                source.historical().operation,
                source.historical().stock_revision,
            ),
        ];
        dependencies.extend(
            account
                .explanations()
                .iter()
                .map(|row| (row.operation, row.stock_revision)),
        );
        dependencies.extend(
            account
                .amendments()
                .iter()
                .map(|row| (row.operation, row.stock_revision)),
        );
        dependencies.extend(
            context
                .recipient_history
                .iter()
                .map(|row| (row.stamp.operation, row.stamp.stock_revision)),
        );
        if dependencies
            .iter()
            .any(|(id, _)| *id == observation.operation)
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        dependencies.push((observation.operation, observation.stock_revision));
        if let Some(raw) = context.report {
            raw.validate().map_err(QuantityEffectError::Actual)?;
            if raw.id == context.original.report.id
                || raw.operation == context.original.report.operation
            {
                return Err(QuantityEffectError::AlreadyReviewed);
            }
            if Some(raw.id) != *report
                || raw.hub != context.hub
                || raw.stock_revision.value() > context.stock_revision.value()
                || raw.catalog_revision.value() > context.catalog.revision().value()
                || raw.recorded_at > context.latest_recorded_at
            {
                return Err(QuantityEffectError::InvalidHistory);
            }
            let claim = &raw.report;
            if claim.variant.is_some_and(|id| id != original.variant.id)
                || claim.packs.is_some_and(|p| p != *corrected_packs)
                || !recipients.could_match(claim.recipient)
                || !occurrences_overlap(claim.occurred, original.occurred)
            {
                return Err(QuantityEffectError::InvalidFacts);
            }
            if let Some(party) = claim.recipient {
                context
                    .catalog
                    .require_kind(party, ReferenceKind::Party)
                    .map_err(|_| QuantityEffectError::InvalidScope)?;
            }
            let anchor = |claim: &crate::hub::actuals::ReportedActualSource| {
                claim.trace == original.trace
                    && original.lines.iter().any(|line| {
                        claim.group.is_none_or(|id| id == line.group)
                            && claim.location.is_none_or(|id| id == line.location.id)
                    })
            };
            if !matches!(&claim.sources, ActualReportSources::Unknown)
                && !claim
                    .sources
                    .candidates()
                    .iter()
                    .any(|claim| anchor(claim) || source.matches_source(claim))
            {
                return Err(QuantityEffectError::Actual(
                    ActualReportError::HistoricalSourceUnresolved,
                ));
            }
            if dependencies.iter().any(|(id, _)| *id == raw.operation) {
                return Err(QuantityEffectError::InvalidHistory);
            }
            dependencies.push((raw.operation, raw.stock_revision));
        }
        if operation == observation.id
            || observation.observation.previous == Some(operation)
            || context.report.is_some_and(|raw| operation == raw.id)
            || operation == context.original.count.id
            || operation == context.original.report.id
        {
            return Err(QuantityEffectError::InvalidHistory);
        }
        if context.known_departures.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
            return Err(QuantityEffectError::Actual(ActualReportError::Capacity));
        }
        let inventory = checked_inventory(context)?;
        let protected = [
            context.original.report.operation,
            source.historical().operation,
            observation.operation,
        ];
        let mut operations = std::collections::BTreeMap::new();
        let mut revisions = std::collections::BTreeMap::new();
        for (id, revision) in dependencies {
            if operation == id {
                return Err(QuantityEffectError::InvalidHistory);
            }
            if operations
                .insert(id, revision)
                .is_some_and(|old| old != revision)
                || revisions
                    .insert(revision.value(), id)
                    .is_some_and(|old| old != id)
            {
                return Err(QuantityEffectError::InvalidHistory);
            }
        }
        let mut target_seen = false;
        let mut audited_counts = BTreeSet::new();
        for (other, checked) in context
            .known_departures
            .iter()
            .zip(&inventory.facts().records)
        {
            // checked_inventory has already validated every immutable quantity
            // snapshot at this exact cut; retain each event's recipient proof.
            let associations = other.recipient_associations_through(
                context.hub,
                context.stock_revision,
                context.catalog.revision(),
                context.latest_recorded_at,
            )?;
            let event = other.original;
            let include_count = checked
                .count_explanation
                .as_ref()
                .is_none_or(|value| audited_counts.insert(value.count.operation));
            for (id, revision) in checked.operations_with_count(include_count) {
                if id == operation
                    || protected.contains(&id)
                    || context.report.is_some_and(|raw| raw.operation == id)
                    || operations
                        .insert(id, revision)
                        .is_some_and(|old| old != revision)
                    || revisions
                        .insert(revision.value(), id)
                        .is_some_and(|old| old != id)
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
            }
            if event.operation == original.operation {
                if event != original
                    || other.recipient_original != context.recipient_original
                    || other.recipient_history != context.recipient_history
                    || !matches!(other.quantity, KnownCorrectionQuantity::CountExplanation { count, account: selected }
                        if count == &context.original.count && selected == account)
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                target_seen = true;
            }
            if corrected_packs.value() > target.packs.value()
                && event.operation != original.operation
                && event.trace == original.trace
                && event.variant.id == original.variant.id
                && occurrences_overlap(event.occurred, original.occurred)
                && (recipients.has_unknown()
                    || associations.has_unknown()
                    || recipients
                        .parties()
                        .any(|party| associations.could_match(Some(party))))
            {
                return Err(QuantityEffectError::Actual(
                    ActualReportError::DuplicateReviewRequired,
                ));
            }
        }
        if !target_seen {
            return Err(QuantityEffectError::InvalidHistory);
        }
        let amendment = CountExplanationAmendment {
            operation,
            stock_revision: context
                .stock_revision
                .next()
                .map_err(|_| QuantityEffectError::InvalidHistory)?,
            explanation: original.operation,
            before: target.packs,
            after: *corrected_packs,
        };
        let mut amendments = account.amendments().to_vec();
        amendments.push(amendment);
        let returns = account
            .effective_explanations()
            .iter()
            .filter(|row| row.established_returns.value() > 0)
            .map(|row| CountExplanationReturns {
                explanation: row.original.operation,
                packs: row.established_returns,
            })
            .collect();
        let after = CountLossQuantityAccount::restore_corrected(
            account.original(),
            account.explanations().to_vec(),
            amendments,
            returns,
        )
        .map_err(QuantityEffectError::Accounting)?;
        Ok(CountClassificationPlan {
            operation,
            actor,
            recorded_at: now,
            observation: observation.clone(),
            original: original.clone(),
            count: context.original.count.clone(),
            source,
            report: context.report.cloned(),
            amendment,
            before: account.clone(),
            after,
        })
    }
}

// Reuse the complete inventory's canonical role/group/owner, shared-account,
// member-closure and global capacity rules. Intern each borrowed account before
// cloning records, so a common count history is copied only once.
fn checked_inventory(
    context: &CountClassificationContext<'_>,
) -> Result<crate::hub::actuals::ActualKnownCountInventory, QuantityEffectError> {
    use crate::hub::actuals::{
        ActualKnownCountExplanation, ActualKnownCountInventory, ActualKnownInterpretation,
        ActualKnownInterpretations,
    };
    use std::{
        collections::{BTreeMap, btree_map::Entry},
        sync::Arc,
    };
    struct Shared<'a> {
        count: &'a crate::hub::counts::CountPostPlan,
        account: &'a CountLossQuantityAccount,
        snapshot: Arc<ActualKnownCountExplanation>,
    }
    let mut counts: BTreeMap<HubRecordId, Shared<'_>> = BTreeMap::new();
    let mut changes = BTreeSet::new();
    let mut records = Vec::with_capacity(context.known_departures.len());
    for known in &context.known_departures {
        changes.extend(known.recipient_history.iter().map(|r| r.stamp.operation));
        let (quantities, count_explanation) = match known.quantity {
            KnownCorrectionQuantity::Physical(accounts) => {
                changes.extend(
                    accounts
                        .iter()
                        .flat_map(|a| a.amendments().iter().map(|r| r.operation)),
                );
                (accounts.to_vec(), None)
            }
            KnownCorrectionQuantity::CountExplanation { count, account } => {
                let shared = match counts.entry(count.operation) {
                    Entry::Vacant(entry) => {
                        changes.extend(account.explanations().iter().map(|r| r.operation));
                        changes.extend(account.amendments().iter().map(|r| r.operation));
                        entry.insert(Shared {
                            count,
                            account,
                            snapshot: Arc::new(ActualKnownCountExplanation {
                                count: count.clone(),
                                account: account.clone(),
                            }),
                        })
                    }
                    Entry::Occupied(entry) => entry.into_mut(),
                };
                // References are immutable and local to this one construction.
                // A distinct object must still pass full value equality.
                if (!std::ptr::eq(shared.count, count) && shared.count != count)
                    || (!std::ptr::eq(shared.account, account) && shared.account != account)
                {
                    return Err(QuantityEffectError::InvalidHistory);
                }
                (vec![], Some(Arc::clone(&shared.snapshot)))
            }
        };
        // Reserve the new classification row in the same global history bound.
        if changes.len() >= HUB_CORRECTION_ROWS {
            return Err(QuantityEffectError::Actual(ActualReportError::Capacity));
        }
        records.push(ActualKnownInterpretation {
            stock_revision: context.stock_revision,
            catalog_revision: context.catalog.revision(),
            latest_recorded_at: context.latest_recorded_at,
            original: known.original.clone(),
            recipient_original: known.recipient_original.clone(),
            recipient_history: known.recipient_history.to_vec(),
            quantities,
            count_explanation,
        });
    }
    ActualKnownCountInventory::new(ActualKnownInterpretations {
        hub: context.hub,
        stock_revision: context.stock_revision,
        catalog_revision: context.catalog.revision(),
        latest_recorded_at: context.latest_recorded_at,
        records,
    })
    .map_err(|error| {
        if error == ActualReportError::Capacity {
            QuantityEffectError::Actual(error)
        } else {
            QuantityEffectError::InvalidHistory
        }
    })
}
