//! Full immutable original debit plus canonical credits and return reservations.
//! Read-only evidence is never a current effect or saved-admission grant.
use super::*;
use harvestcircle_domain::hub::corrections::{
    CorrectedDepartureFamily, DepartureQuantityAccount, FoundFundingCredit, OriginalPhysicalDebit,
    PhysicalDebitFunding, PhysicalDebitKey, PhysicalDebitKind,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionFundingQuery {
    pub target_operation: DurableRequestId,
    pub original_group: HubRecordId,
    pub expected_stock_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionFundingEvidence {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub family: CorrectedDepartureFamily,
    pub quantity: DepartureQuantityAccount,
    pub debits: Vec<PhysicalDebitFunding>,
    pub uncertain_intent: bool,
    /// Includes both unresolved actual reports and raw correction observations.
    pub unresolved_actual: bool,
}

impl Database {
    /// Complete current read for one named immutable line. An available balance
    /// is historical funding evidence only, never permission to post a correction.
    pub async fn hub_correction_target_funding(
        &self,
        actor: PublicKey,
        query: HubCorrectionFundingQuery,
    ) -> Result<HubCorrectionFundingEvidence, SafeError> {
        if query.expected_stock_revision.is_some() != query.expected_catalog_revision.is_some() {
            return Err(stock::invalid_input());
        }
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let state = stock::read::state_on(tx, hub.id()).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    if query
                        .expected_stock_revision
                        .is_some_and(|v| v != state.revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|v| v != catalog.revision())
                    {
                        return Err(stock::conflict());
                    }
                    let loaded = load_on(
                        tx,
                        hub.id(),
                        &query.target_operation,
                        query.original_group,
                        state.revision,
                    )
                    .await?;
                    let uncertain = crate::hub::submission::uncertainty_all_on(tx).await?;
                    Ok(HubCorrectionFundingEvidence {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        family: loaded.family,
                        quantity: loaded.quantity,
                        debits: loaded.debits,
                        uncertain_intent: uncertain.uncertain_intent,
                        unresolved_actual: uncertain.unresolved_actual,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) struct LoadedFunding {
    pub family: CorrectedDepartureFamily,
    pub quantity: DepartureQuantityAccount,
    pub debits: Vec<PhysicalDebitFunding>,
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    target: &DurableRequestId,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<LoadedFunding, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(load_with_proofs_on(&mut reader, hub, target, group, cut)).await
}

pub(super) async fn load_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    target: &DurableRequestId,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<LoadedFunding, SafeError> {
    let line = Box::pin(quantity::original_on(tx, hub, target, group, cut)).await?;
    let history = quantity::history_from_original_on(tx, hub, target, group, cut, &line).await?;
    // Scalar metadata alone remains insufficient. Complete purpose-sealed
    // physical outputs are required for every additional amendment. Current
    // authority audits full roots first; immutable recovery anchors all of
    // these reconstructed facts to its exact historical admission material.
    let additional =
        super::physical_balance::additional_debits_on(tx, hub, &line, &history.amendments, cut)
            .await?;
    let returns = Box::pin(funding::returns_on(tx, hub, &line, cut)).await?;
    let changes = history.amendments.clone();
    let quantity = history.with_verified_returns(returns.total)?;
    let op = stock::read::operation_on(tx, hub, target)
        .await?
        .ok_or_else(corrupt_storage)?;
    let revision = op.revision;
    let recorded_at = op.recorded_at;
    let (family, kind, before, packs) = match op.kind.as_str() {
        "departure" => {
            let original = handling::departure::read::original_on(tx, hub, op)
                .await?
                .record;
            let consumed = original
                .consumed
                .into_iter()
                .find(|line| line.before.id == group)
                .ok_or_else(corrupt_storage)?;
            let packs = consumed.before.packs;
            (
                CorrectedDepartureFamily::Normal,
                PhysicalDebitKind::NormalDeparture,
                consumed.before,
                packs,
            )
        }
        "actual_review" => {
            let original = tx.actual_review(hub, op).await?;
            let crate::HubActualReviewOriginal::Departure(plan) = &original.original else {
                return Err(corrupt_storage());
            };
            (
                CorrectedDepartureFamily::ReviewedActual,
                PhysicalDebitKind::ReviewedActual,
                plan.reduction.before.clone(),
                plan.reduction.actual_departed,
            )
        }
        _ => return Err(corrupt_storage()),
    };
    if packs != line.original.packs
        || before.id != group
        || before.trace != line.original.trace
        || before.variant != line.original.variant
        || revision != line.original.stock_revision
    {
        return Err(corrupt_storage());
    }
    let key = PhysicalDebitKey {
        operation: line.original.operation,
        group,
    };
    let mut originals = vec![OriginalPhysicalDebit {
        key,
        kind,
        target: line.original,
        before,
        packs,
        stock_revision: revision,
        recorded_at,
    }];
    originals.extend(additional);
    let mut keys = std::collections::BTreeSet::new();
    for original in &originals {
        if original.target != line.original || !keys.insert(original.key) {
            return Err(corrupt_storage());
        }
    }
    let mut credits = Vec::new();
    for credit in super::return_allowances::credits_on(tx, hub, &line, &changes).await? {
        if !keys.contains(&credit.debit) {
            return Err(corrupt_storage());
        }
        let id = crate::hub::actuals::common::operation(credit.operation)
            .map_err(|_| corrupt_storage())?;
        let root = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if root.kind != "correction_found"
            || root.revision != credit.stock_revision
            || root.revision.value() > cut.value()
        {
            return Err(corrupt_storage());
        }
        credits.push(FoundFundingCredit {
            operation: credit.operation,
            allocation: credit.allocation,
            debit: credit.debit,
            packs: credit.packs,
            stock_revision: credit.stock_revision,
            recorded_at: root.recorded_at,
        });
    }
    let zero = harvestcircle_domain::hub::reference::WholePacks::from_value(0).expect("zero");
    let mut recovered = zero;
    let mut returned = zero;
    let mut effective = zero;
    let mut debits = Vec::with_capacity(originals.len());
    for original in originals {
        let key = original.key;
        let funding = PhysicalDebitFunding::restore(
            original,
            credits
                .iter()
                .filter(|credit| credit.debit == key)
                .copied()
                .collect(),
            returns
                .established
                .iter()
                .filter(|reservation| reservation.debit == key)
                .copied()
                .collect(),
            returns.unresolved,
        )
        .map_err(|_| corrupt_storage())?;
        recovered = recovered
            .checked_add(funding.recovered())
            .map_err(|_| corrupt_storage())?;
        returned = returned
            .checked_add(funding.returned())
            .map_err(|_| corrupt_storage())?;
        effective = effective
            .checked_add(
                funding
                    .original()
                    .packs
                    .checked_sub(funding.recovered())
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?;
        debits.push(funding);
    }
    if recovered != quantity.found_remainder()
        || returned != quantity.established_returns()
        || effective != quantity.effective()
    {
        return Err(corrupt_storage());
    }
    Ok(LoadedFunding {
        family,
        quantity,
        debits,
    })
}
