//! Later explicit association of existing restricted returned custody. Neither
//! this arithmetic plan nor its consistency seals establish native authority,
//! historical completeness, source truth or a new physical effect.
use super::*;
use crate::hub::{
    HubId,
    actuals::{
        ActualKnownCountExplanation, ActualKnownInterpretation, CountExplanationSourceWitness,
    },
    reference::{ReferenceCatalog, SourceProvenance},
    returns::{ReturnIdentities, ReturnLinkResolution, ReturnPlan, ReturnTiming},
    stock::{HUB_TRACE_CAPACITY, Occurrence},
};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountReturnAssociationCommand {
    pub return_operation: HubRecordId,
    pub explanation_operation: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_account_prior: Option<HubRecordId>,
    pub expected_returns_prior: Option<HubRecordId>,
    pub source_operation: HubRecordId,
    pub source_group: HubRecordId,
    pub source_evidence: SourceProvenance,
    pub quantity_evidence: SourceProvenance,
    pub timing_evidence: SourceProvenance,
}

/// A separately retained current interpretation, never a replacement Return or
/// a physical debit. Native projections must bind its full original/capture.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountReturnAssociation {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub return_operation: HubRecordId,
    pub return_stock_revision: HubRevision,
    pub returned: HubRecordId,
    pub returned_population: HubRecordId,
    pub returned_group: HubRecordId,
    pub explanation_operation: HubRecordId,
    pub explanation_stock_revision: HubRevision,
    pub explanation_trace: HubRecordId,
    pub count_operation: HubRecordId,
    pub count_stock_revision: HubRevision,
    pub anchor_group: HubRecordId,
    pub variant: HubRecordId,
    pub packs: WholePacks,
    pub return_occurred: Occurrence,
    pub explanation_occurred: Occurrence,
    pub timing: ReturnTiming,
    pub return_original_digest: [u8; 32],
    pub explanation_source_digest: [u8; 32],
}

pub struct CountReturnAssociationContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub latest_recorded_at: UnixTimestamp,
    pub returned: &'a ReturnPlan,
    /// Independently scoped native Return origin; the digest is not a hub grant.
    pub return_hub: HubId,
    /// Native original result identity, after the existing full Return verifier.
    /// This data alone neither authenticates an original nor grants an effect.
    pub return_original_digest: [u8; 32],
    pub explanation: &'a CountExplanationPlan,
    pub account: &'a CountLossQuantityAccount,
    pub recipient_original: &'a OriginalRecipientAttribution,
    pub recipient_history: &'a [RecipientAttributionCorrection],
    /// Complete native association prefix, verified before scope filtering.
    pub associations: &'a [CountReturnAssociation],
    /// Complete established normal-return prefix from fully verified originals.
    /// Unknown and unresolved manual claims never supply graph edges.
    pub normal_returns: &'a [NormalReturnProvenance],
    /// Native global lookup includes all departure families, not just this E.
    pub prior_association_for_return: Option<HubRecordId>,
    pub uncertain_submissions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountReturnAssociationPlan {
    pub command: CountReturnAssociationCommand,
    pub association: CountReturnAssociation,
    pub returned_original: ReturnPlan,
    pub explanation_original: CountExplanationPlan,
    pub source: CountExplanationSourceWitness,
    pub recipient_original: OriginalRecipientAttribution,
    pub recipient_history: Vec<RecipientAttributionCorrection>,
    pub prior_associations: Vec<CountReturnAssociation>,
    pub normal_returns: Vec<NormalReturnProvenance>,
    pub before: CountLossQuantityAccount,
    pub after: CountLossQuantityAccount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountReturnAssociationError {
    InvalidHistory,
    InvalidFacts,
    StaleRevision,
    UncertainSubmission,
    AlreadyAssociated,
    WrongSource,
    ConflictingClaim,
    ContradictoryTiming,
    Capacity,
    Accounting(CorrectionError),
}

/// Descriptive provenance only. Construction does not authenticate the supplied
/// original, establish prefix completeness or grant current mutation authority.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct NormalReturnProvenance {
    hub: HubId,
    return_operation: HubRecordId,
    stock_revision: HubRevision,
    catalog_revision: HubRevision,
    recorded_at: UnixTimestamp,
    population: HubRecordId,
    source_trace: HubRecordId,
    departure_operation: HubRecordId,
    departure_stock_revision: HubRevision,
    original_digest: [u8; 32],
}

impl NormalReturnProvenance {
    pub fn from_original(
        hub: HubId,
        returned: &ReturnPlan,
        original_digest: [u8; 32],
    ) -> Result<Self, CountReturnAssociationError> {
        use CountReturnAssociationError as Error;
        let departure = returned
            .linkage
            .departure
            .as_ref()
            .ok_or(Error::InvalidHistory)?;
        let source = returned
            .linkage
            .claimed_source
            .as_ref()
            .ok_or(Error::InvalidHistory)?;
        let claim = returned.linkage.claim.ok_or(Error::InvalidHistory)?;
        departure
            .occurred
            .validate(departure.recorded_at)
            .map_err(|_| Error::InvalidHistory)?;
        returned
            .intake
            .occurred
            .validate(returned.intake.recorded_at)
            .map_err(|_| Error::InvalidHistory)?;
        if returned.linkage.resolution != ReturnLinkResolution::EstablishedScope
            || departure.hub != hub
            || claim.departure != departure.departure
            || claim.group != Some(source.original.group)
            || source.original.variant.id != returned.intake.variant.id
            || returned.identities
                != ReturnIdentities::from_operation_bytes(returned.identities.operation)
                    .map_err(|_| Error::InvalidHistory)?
            || returned.intake.population != returned.identities.population
            || returned.group.trace != returned.identities.population
            || returned.group.id != returned.identities.group
            || departure.catalog_revision.value() > returned.catalog_revision.value()
            || departure.recorded_at > returned.intake.recorded_at
            || returned.linkage.timing
                != crate::hub::returns::return_timing(departure.occurred, returned.intake.occurred)
            || returned.linkage.timing == ReturnTiming::Contradictory
        {
            return Err(Error::InvalidHistory);
        }
        let row = Self {
            hub,
            return_operation: returned.identities.operation,
            stock_revision: returned.stock_revision,
            catalog_revision: returned.catalog_revision,
            recorded_at: returned.intake.recorded_at,
            population: returned.identities.population,
            source_trace: source.original.trace.id,
            departure_operation: departure.operation,
            departure_stock_revision: departure.stock_revision,
            original_digest,
        };
        row.validate()?;
        Ok(row)
    }

    fn validate(&self) -> Result<(), CountReturnAssociationError> {
        if self.population
            != ReturnIdentities::from_operation_bytes(self.return_operation)
                .map_err(|_| CountReturnAssociationError::InvalidHistory)?
                .population
            || self.population == self.source_trace
            || self.return_operation == self.departure_operation
            || self.departure_stock_revision == HubRevision::INITIAL
            || self.departure_stock_revision.value() >= self.stock_revision.value()
        {
            return Err(CountReturnAssociationError::InvalidHistory);
        }
        Ok(())
    }
}

pub fn count_return_association_id(operation: HubRecordId) -> Result<HubRecordId, CorrectionError> {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count-return.association.identity.v1\0");
    hash.update(operation.as_bytes());
    let digest = hash.finalize();
    HubRecordId::from_bytes(digest[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| CorrectionError::InvalidHistory)
}

impl CountReturnAssociationCommand {
    pub fn plan(
        &self,
        context: &CountReturnAssociationContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CountReturnAssociationPlan, CountReturnAssociationError> {
        use CountReturnAssociationError as Error;
        if context.uncertain_submissions {
            return Err(Error::UncertainSubmission);
        }
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
        {
            return Err(Error::StaleRevision);
        }
        if now < context.latest_recorded_at {
            return Err(Error::InvalidFacts);
        }
        if context.prior_association_for_return.is_some() {
            return Err(Error::AlreadyAssociated);
        }
        let returned = context.returned;
        let original = context.explanation;
        let known = &original.known_event;
        if returned.linkage.resolution == ReturnLinkResolution::EstablishedScope {
            return Err(Error::AlreadyAssociated);
        }
        let source = CountExplanationSourceWitness::from_explanation(original)
            .map_err(|_| Error::InvalidHistory)?;
        if self.return_operation != returned.identities.operation
            || context.return_hub != context.hub
            || self.explanation_operation != original.operation
            || known.hub != context.hub
            || returned.stock_revision.value() > context.stock_revision.value()
            || known.stock_revision.value() > context.stock_revision.value()
            || returned.catalog_revision.value() > context.catalog.revision().value()
            || known.catalog_revision.value() > context.catalog.revision().value()
            || returned.intake.recorded_at > context.latest_recorded_at
            || original.recorded_at > context.latest_recorded_at
            || returned.intake.packs.value() == 0
            || returned.intake.variant.id != known.variant.id
            || ReturnIdentities::from_operation_bytes(returned.identities.operation)
                .map_err(|_| Error::InvalidHistory)?
                != returned.identities
            || returned.intake.population != returned.identities.population
            || returned.group.id != returned.identities.group
            || returned.group.trace != returned.identities.population
            || returned.group.variant != known.variant.id
            || returned.group.packs != returned.intake.packs
            || returned.acceptance != crate::hub::treatment::AcceptanceStatus::ReviewRequired
        {
            return Err(Error::InvalidFacts);
        }
        if self.source_operation != source.historical().operation
            || self.source_group != source.historical().group.id
        {
            return Err(Error::WrongSource);
        }
        // A shortage in this intake's own cohort cannot turn the same original
        // intake into evidence of a return from its later explained departure.
        // Equal physical timestamps must not conceal that scope cycle.
        if returned.identities.population == known.trace
            || source.historical().operation == returned.identities.operation
        {
            return Err(Error::InvalidFacts);
        }
        if returned.linkage.departure.is_some()
            || returned.current_allowances.is_some()
            || returned.linkage.claimed_source.is_some()
            || !returned.linkage.plausible_sources.is_empty()
        {
            return Err(Error::ConflictingClaim);
        }
        let expected_claim = count_explanation_id(original.operation).map_err(Error::Accounting)?;
        if returned.linkage.claim.is_some_and(|claim| {
            claim.departure != expected_claim
                || claim
                    .group
                    .is_some_and(|group| group != original.count.before.id)
        }) {
            return Err(Error::ConflictingClaim);
        }
        if !matches!(
            (returned.linkage.claim, returned.linkage.resolution),
            (None, ReturnLinkResolution::Unknown) | (Some(_), ReturnLinkResolution::Unrecorded)
        ) {
            return Err(Error::ConflictingClaim);
        }
        returned
            .intake
            .occurred
            .validate(returned.intake.recorded_at)
            .map_err(|_| Error::InvalidFacts)?;
        let timing = crate::hub::returns::return_timing(known.occurred, returned.intake.occurred);
        if timing == ReturnTiming::Contradictory {
            return Err(Error::ContradictoryTiming);
        }
        let account = context.account;
        let head = account
            .explanations()
            .iter()
            .map(|r| (r.stock_revision, r.operation))
            .chain(
                account
                    .amendments()
                    .iter()
                    .map(|r| (r.stock_revision, r.operation)),
            )
            .max_by_key(|(r, _)| r.value())
            .map(|(_, id)| id);
        if head != self.expected_account_prior {
            return Err(Error::StaleRevision);
        }
        let interpretation = ActualKnownInterpretation {
            stock_revision: context.stock_revision,
            catalog_revision: context.catalog.revision(),
            latest_recorded_at: context.latest_recorded_at,
            original: known.clone(),
            recipient_original: context.recipient_original.clone(),
            recipient_history: context.recipient_history.to_vec(),
            quantities: Vec::new(),
            count_explanation: Some(Arc::new(ActualKnownCountExplanation {
                count: original.count.clone(),
                account: account.clone(),
            })),
        };
        interpretation.view().map_err(|_| Error::InvalidHistory)?;
        audit_dependencies(context, &interpretation, &source, operation)?;
        let mut sums = BTreeMap::new();
        let mut operations = BTreeSet::new();
        let mut returns = BTreeSet::new();
        let mut previous = 0;
        let mut return_head = None;
        if context
            .associations
            .len()
            .saturating_add(context.normal_returns.len())
            >= HUB_TRACE_CAPACITY
        {
            return Err(Error::Capacity);
        }
        for row in context.normal_returns {
            row.validate()?;
            if row.hub != context.hub
                || row.stock_revision.value() > context.stock_revision.value()
                || row.catalog_revision.value() > context.catalog.revision().value()
                || row.recorded_at > context.latest_recorded_at
                || !returns.insert(row.return_operation)
            {
                return Err(Error::InvalidHistory);
            }
            if row.return_operation == self.return_operation {
                return Err(Error::AlreadyAssociated);
            }
        }
        for row in context.associations {
            row.validate()?;
            if row.hub != context.hub
                || row.stock_revision.value() <= previous
                || row.stock_revision.value() > context.stock_revision.value()
                || row.catalog_revision.value() > context.catalog.revision().value()
                || row.recorded_at > context.latest_recorded_at
                || !operations.insert(row.operation)
                || !returns.insert(row.return_operation)
            {
                return Err(Error::InvalidHistory);
            }
            previous = row.stock_revision.value();
            if row.return_operation == self.return_operation {
                return Err(Error::AlreadyAssociated);
            }
            if row.count_operation == account.original().operation {
                if row.anchor_group != original.count.before.id
                    || row.variant != account.original().variant
                    || row.explanation_trace != known.trace
                {
                    return Err(Error::InvalidHistory);
                }
                let total = sums.entry(row.explanation_operation).or_insert(zero());
                *total = total
                    .checked_add(row.packs)
                    .map_err(|_| Error::InvalidHistory)?;
                return_head = Some(row.operation);
            }
        }
        if return_head != self.expected_returns_prior {
            return Err(Error::StaleRevision);
        }
        for row in account.effective_explanations() {
            if sums.remove(&row.original.operation).unwrap_or_else(zero) != row.established_returns
            {
                return Err(Error::InvalidHistory);
            }
        }
        if !sums.is_empty() {
            return Err(Error::InvalidHistory);
        }
        audit_provenance(
            context.associations,
            context.normal_returns,
            returned.identities.population,
            known.trace,
        )?;
        let roles = [
            operation,
            self.return_operation,
            self.explanation_operation,
            account.original().operation,
        ];
        if roles
            .iter()
            .enumerate()
            .any(|(i, id)| roles[..i].contains(id))
            || operations.contains(&operation)
            || account
                .explanations()
                .iter()
                .any(|r| r.operation == operation)
            || account
                .amendments()
                .iter()
                .any(|r| r.operation == operation)
        {
            return Err(Error::InvalidFacts);
        }
        let mut totals = account
            .effective_explanations()
            .iter()
            .map(|row| CountExplanationReturns {
                explanation: row.original.operation,
                packs: row.established_returns,
            })
            .collect::<Vec<_>>();
        let selected = totals
            .iter_mut()
            .find(|r| r.explanation == original.operation)
            .ok_or(Error::InvalidHistory)?;
        selected.packs = selected
            .packs
            .checked_add(returned.intake.packs)
            .map_err(|_| Error::InvalidFacts)?;
        let after = CountLossQuantityAccount::restore_corrected(
            account.original(),
            account.explanations().to_vec(),
            account.amendments().to_vec(),
            totals,
        )
        .map_err(Error::Accounting)?;
        let association = CountReturnAssociation {
            id: count_return_association_id(operation).map_err(Error::Accounting)?,
            operation,
            hub: context.hub,
            stock_revision: context
                .stock_revision
                .next()
                .map_err(|_| Error::InvalidHistory)?,
            catalog_revision: context.catalog.revision(),
            actor,
            recorded_at: now,
            return_operation: self.return_operation,
            return_stock_revision: returned.stock_revision,
            returned: returned.identities.returned,
            returned_population: returned.identities.population,
            returned_group: returned.identities.group,
            explanation_operation: original.operation,
            explanation_stock_revision: known.stock_revision,
            explanation_trace: known.trace,
            count_operation: account.original().operation,
            count_stock_revision: account.original().stock_revision,
            anchor_group: original.count.before.id,
            variant: known.variant.id,
            packs: returned.intake.packs,
            return_occurred: returned.intake.occurred,
            explanation_occurred: known.occurred,
            timing,
            return_original_digest: context.return_original_digest,
            explanation_source_digest: source.digest().map_err(|_| Error::InvalidHistory)?,
        };
        association.validate()?;
        Ok(CountReturnAssociationPlan {
            command: self.clone(),
            association,
            returned_original: returned.clone(),
            explanation_original: original.clone(),
            source,
            recipient_original: context.recipient_original.clone(),
            recipient_history: context.recipient_history.to_vec(),
            prior_associations: context.associations.to_vec(),
            normal_returns: context.normal_returns.to_vec(),
            before: account.clone(),
            after,
        })
    }
}

impl CountReturnAssociation {
    /// Only field consistency. Native roots and complete original capture still
    /// need independent verification before this row can supply an allowance.
    pub fn validate(&self) -> Result<(), CountReturnAssociationError> {
        use CountReturnAssociationError as Error;
        let ids = ReturnIdentities::from_operation_bytes(self.return_operation)
            .map_err(|_| Error::InvalidHistory)?;
        if self.id != count_return_association_id(self.operation).map_err(Error::Accounting)?
            || self.returned != ids.returned
            || self.returned_population != ids.population
            || self.returned_group != ids.group
            || self.packs.value() == 0
            || self.returned_population == self.explanation_trace
            || self.stock_revision == HubRevision::INITIAL
            || self.count_stock_revision.value() >= self.explanation_stock_revision.value()
            || matches!(self.explanation_occurred, Occurrence::Unknown)
            || [
                self.return_stock_revision,
                self.explanation_stock_revision,
                self.count_stock_revision,
            ]
            .iter()
            .any(|revision| {
                *revision == HubRevision::INITIAL || revision.value() >= self.stock_revision.value()
            })
            || self.timing
                != crate::hub::returns::return_timing(
                    self.explanation_occurred,
                    self.return_occurred,
                )
            || self.timing == ReturnTiming::Contradictory
        {
            return Err(Error::InvalidHistory);
        }
        let roles = [
            self.operation,
            self.return_operation,
            self.explanation_operation,
            self.count_operation,
        ];
        if roles
            .iter()
            .enumerate()
            .any(|(index, id)| roles[..index].contains(id))
        {
            return Err(Error::InvalidHistory);
        }
        self.return_occurred
            .validate(self.recorded_at)
            .map_err(|_| Error::InvalidHistory)?;
        self.explanation_occurred
            .validate(self.recorded_at)
            .map_err(|_| Error::InvalidHistory)?;
        Ok(())
    }
}

// Same bounded bidirectional audit pattern as quantity classification. Repeated
// references to the same immutable operation/revision are permitted.
fn audit_dependencies(
    context: &CountReturnAssociationContext<'_>,
    interpretation: &ActualKnownInterpretation,
    source: &CountExplanationSourceWitness,
    proposed: HubRecordId,
) -> Result<(), CountReturnAssociationError> {
    use CountReturnAssociationError as Error;
    let mut operations = BTreeMap::new();
    let mut revisions = BTreeMap::new();
    let mut include = |operation: HubRecordId, revision: HubRevision| -> Result<(), Error> {
        if operation == proposed
            || revision == HubRevision::INITIAL
            || revision.value() > context.stock_revision.value()
        {
            return Err(Error::InvalidHistory);
        }
        if !operations.contains_key(&operation)
            && operations.len() >= crate::hub::stock::HUB_STOCK_EVENT_CAPACITY
        {
            return Err(Error::Capacity);
        }
        if operations
            .insert(operation, revision)
            .is_some_and(|prior| prior != revision)
            || revisions
                .insert(revision.value(), operation)
                .is_some_and(|prior| prior != operation)
        {
            return Err(Error::InvalidHistory);
        }
        Ok(())
    };
    for (operation, revision) in interpretation.operations() {
        include(operation, revision)?;
    }
    for (operation, revision) in [
        (
            context.returned.identities.operation,
            context.returned.stock_revision,
        ),
        (
            context.explanation.report.operation,
            context.explanation.report.stock_revision,
        ),
        (
            source.historical().operation,
            source.historical().stock_revision,
        ),
    ] {
        include(operation, revision)?;
    }
    for row in context.associations {
        for (operation, revision) in [
            (row.operation, row.stock_revision),
            (row.return_operation, row.return_stock_revision),
            (row.explanation_operation, row.explanation_stock_revision),
            (row.count_operation, row.count_stock_revision),
        ] {
            include(operation, revision)?;
        }
    }
    for row in context.normal_returns {
        include(row.return_operation, row.stock_revision)?;
        include(row.departure_operation, row.departure_stock_revision)?;
    }
    Ok(())
}

// Every established return has one outgoing provenance edge. Keep original
// edges after depletion or later quantity corrections; neither erases history.
fn audit_provenance(
    counts: &[CountReturnAssociation],
    normal: &[NormalReturnProvenance],
    population: HubRecordId,
    source: HubRecordId,
) -> Result<(), CountReturnAssociationError> {
    use CountReturnAssociationError as Error;
    let mut edges = BTreeMap::new();
    let mut nodes = BTreeSet::from([population, source]);
    for (from, to) in counts
        .iter()
        .map(|r| (r.returned_population, r.explanation_trace))
        .chain(normal.iter().map(|r| (r.population, r.source_trace)))
    {
        if edges.insert(from, to).is_some() {
            return Err(Error::InvalidHistory);
        }
        nodes.extend([from, to]);
        if nodes.len() > HUB_TRACE_CAPACITY {
            return Err(Error::Capacity);
        }
    }
    let mut complete = BTreeSet::new();
    for start in edges.keys() {
        let mut path = BTreeSet::new();
        let mut at = *start;
        while !complete.contains(&at) {
            if !path.insert(at) {
                return Err(Error::InvalidHistory);
            }
            let Some(next) = edges.get(&at) else {
                break;
            };
            at = *next;
        }
        complete.extend(path);
    }
    if edges.contains_key(&population) {
        return Err(Error::AlreadyAssociated);
    }
    let mut at = source;
    loop {
        if at == population {
            return Err(Error::InvalidFacts);
        }
        let Some(next) = edges.get(&at) else {
            break;
        };
        at = *next;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
