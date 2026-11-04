//! Complete immutable events and current quantity interpretations at one cut.
//! Returned data conveys neither fresh admission nor permission for an effect.
use super::*;
use harvestcircle_domain::hub::{
    actuals::ActualKnownDeparture,
    corrections::{
        CorrectedDepartureFamily, CorrectionKnownDeparture, DepartureQuantityAccount,
        KnownCorrectionQuantity, OriginalRecipientAttribution, PhysicalDebitFunding,
        RecipientAttributionCorrection, current_recipient_attribution,
    },
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionKnownQuery {
    pub trace: HubRecordId,
    pub expected_stock_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionKnownDeparture {
    pub original: ActualKnownDeparture,
    pub recipient_original: OriginalRecipientAttribution,
    pub recipient_history: Vec<RecipientAttributionCorrection>,
    /// Every original line, including unamended and zero-effective lines.
    pub quantities: Vec<DepartureQuantityAccount>,
    /// Exact physical bounds also retain unresolved return scope; an unresolved
    /// return is never silently converted into a zero reservation or clearance.
    pub funding: Vec<PhysicalDebitFunding>,
}

impl HubCorrectionKnownDeparture {
    pub fn as_domain(&self) -> CorrectionKnownDeparture<'_> {
        CorrectionKnownDeparture {
            original: &self.original,
            recipient_original: &self.recipient_original,
            recipient_history: &self.recipient_history,
            quantity: KnownCorrectionQuantity::Physical(&self.quantities),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionKnownEvidence {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub departures: Vec<HubCorrectionKnownDeparture>,
    pub uncertain_intent: bool,
    pub unresolved_actual: bool,
}

impl Database {
    /// Complete population evidence. Zero quantity does not erase an event or
    /// establish that a newly reported event is unrelated to its original.
    pub async fn hub_correction_known_departures(
        &self,
        actor: PublicKey,
        query: HubCorrectionKnownQuery,
    ) -> Result<HubCorrectionKnownEvidence, SafeError> {
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
                    handling::read::trace_on(tx, hub.id(), query.trace).await?;
                    let departures = load_on(tx, hub.id(), query.trace, state.revision).await?;
                    let uncertain = crate::hub::submission::uncertainty_all_on(tx).await?;
                    Ok(HubCorrectionKnownEvidence {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        departures,
                        uncertain_intent: uncertain.uncertain_intent,
                        unresolved_actual: uncertain.unresolved_actual,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn recipient_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
) -> Result<OriginalRecipientAttribution, SafeError> {
    let op = stock::read::operation_on(tx, hub, operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    let (stock_revision, catalog_revision, recorded_at, actor) =
        (op.revision, op.catalog_revision, op.recorded_at, op.actor);
    let (family, recipient, populations, issued_versions) = match op.kind.as_str() {
        "departure" => {
            let record = handling::departure::read::original_on(tx, hub, op)
                .await?
                .record;
            let material = &record
                .paper
                .versions
                .last()
                .ok_or_else(corrupt_storage)?
                .material;
            (
                CorrectedDepartureFamily::Normal,
                material.recipient.clone(),
                material
                    .lines
                    .iter()
                    .map(|line| line.trace.id)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                record.paper.versions.iter().map(|v| v.id).collect(),
            )
        }
        "actual_review" => {
            let receipt = Box::pin(crate::hub::actuals::reviews::original_on(tx, hub, op)).await?;
            let crate::HubActualReviewOriginal::Departure(plan) = receipt.original else {
                return Err(corrupt_storage());
            };
            (
                CorrectedDepartureFamily::ReviewedActual,
                plan.comparison.scope.recipient,
                vec![plan.reduction.before.trace],
                Vec::new(),
            )
        }
        _ => return Err(corrupt_storage()),
    };
    let original = OriginalRecipientAttribution {
        hub,
        family,
        operation: crate::hub::actuals::common::operation_id(operation)
            .map_err(|_| corrupt_storage())?,
        stock_revision,
        catalog_revision,
        recorded_at,
        actor,
        recipient,
        populations,
        issued_versions,
    };
    current_recipient_attribution(&original, &[]).map_err(|_| corrupt_storage())?;
    Ok(original)
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<Vec<HubCorrectionKnownDeparture>, SafeError> {
    // No native reader for these families is qualified yet. Reject them rather
    // than using a gross-only list that silently loses a known interpretation.
    let unsupported: i64 = sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_corrections c JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE o.hub_id=? AND o.stock_revision<=? AND c.family IN ('recipient','count_explain','count_classify')"
    )).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
        .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    // Verify complete originals before any denormalized trace filter. A damaged
    // event's trace/variant cannot hide an otherwise relevant immutable event.
    let limit = harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY
        + harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY;
    let operations: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE hub_id=? ",
        "AND stock_revision<=? AND kind IN ('departure','actual_review') ORDER BY stock_revision LIMIT ?"
    )).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
        .bind(i64::try_from(limit + 1).map_err(|_| corrupt_storage())?)
        .fetch_all(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if operations.len() > limit {
        return Err(stock::capacity());
    }
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        match op.kind.as_str() {
            "departure" => {
                Box::pin(handling::departure::read::original_on(tx, hub, op)).await?;
            }
            "actual_review" => {
                Box::pin(crate::hub::actuals::reviews::original_on(tx, hub, op)).await?;
            }
            _ => return Err(corrupt_storage()),
        }
    }
    let originals = Box::pin(handling::actuals::known_departures_on(tx, hub, trace, cut)).await?;
    let mut result = Vec::with_capacity(originals.len());
    let mut ids = BTreeSet::new();
    for original in originals {
        if !ids.insert(original.operation) {
            return Err(corrupt_storage());
        }
        let operation = crate::hub::actuals::common::operation(original.operation)
            .map_err(|_| corrupt_storage())?;
        let recipient_original = Box::pin(recipient_on(tx, hub, &operation)).await?;
        if recipient_original.operation != original.operation
            || recipient_original.recipient != original.recipient
            || recipient_original.stock_revision != original.stock_revision
            || recipient_original.catalog_revision != original.catalog_revision
            || recipient_original.recorded_at != original.recorded_at
            || recipient_original.actor != original.actor
            || !recipient_original.populations.contains(&trace)
        {
            return Err(corrupt_storage());
        }
        let mut quantities = Vec::with_capacity(original.lines.len());
        let mut funding = Vec::new();
        for line in &original.lines {
            let loaded = Box::pin(super::target_funding::load_on(
                tx, hub, &operation, line.group, cut,
            ))
            .await?;
            let quantity = loaded.quantity;
            if loaded.family != recipient_original.family
                || quantity.original().operation != original.operation
                || quantity.original().group != line.group
                || quantity.original().trace != original.trace
                || quantity.original().variant != original.variant.id
                || quantity.original().stock_revision != original.stock_revision
                || quantity.original().packs != line.packs
            {
                return Err(corrupt_storage());
            }
            quantities.push(quantity);
            funding.extend(loaded.debits);
        }
        result.push(HubCorrectionKnownDeparture {
            original,
            recipient_original,
            recipient_history: Vec::new(),
            quantities,
            funding,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
