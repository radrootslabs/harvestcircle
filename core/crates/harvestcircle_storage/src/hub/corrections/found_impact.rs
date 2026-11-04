//! Immutable initial follow-up scope. Later issued links retain their own
//! introduction operation and cannot rewrite a correction's original scope.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{CorrectionImpact, CorrectionImpactSet, HUB_CORRECTION_IMPACTS},
    reference::{SourceAttribution, SourceCertainty},
};
use std::collections::BTreeSet;

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    context: &super::found_context::Context,
) -> Result<CorrectionImpactSet, SafeError> {
    let target = context.funding.quantity.original();
    let operation =
        crate::hub::actuals::common::operation(target.operation).map_err(|_| corrupt_storage())?;
    // These attribution families are still fenced globally. Never silently
    // omit one if a future writer becomes available before its current reader.
    let unsupported:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_corrections correction JOIN hub_stock_operations original ON original.operation_id=correction.operation_id ",
        "WHERE original.hub_id=? AND original.stock_revision<=? AND ((correction.family='source' AND correction.subject_id=?) OR (correction.family='recipient' AND correction.target_operation_id=?))"
    )).bind(context.hub.as_bytes().as_slice()).bind(i64::try_from(context.stock_revision.value()).map_err(|_|corrupt_storage())?).bind(target.trace.as_bytes().as_slice()).bind(operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    let mut impact = BTreeSet::from([
        CorrectionImpact::Population(target.trace),
        CorrectionImpact::Departure(target.operation),
    ]);
    let population = handling::read::trace_on(tx, context.hub, target.trace).await?;
    match &population.source {
        SourceAttribution::Unknown => {
            impact.insert(CorrectionImpact::UnresolvedSource(target.trace));
        }
        SourceAttribution::Referenced { source, certainty } => {
            impact.insert(CorrectionImpact::Source(source.id));
            if source.issuer.is_none() || *certainty == SourceCertainty::ReportedPossible {
                impact.insert(CorrectionImpact::UnresolvedSource(target.trace));
            }
        }
    }
    let op = stock::read::operation_on(tx, context.hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    match op.kind.as_str() {
        "departure" => {
            let original = handling::departure::read::original_on(tx, context.hub, op)
                .await?
                .record;
            let issued = original.paper.versions.last().ok_or_else(corrupt_storage)?;
            impact.insert(CorrectionImpact::Recipient(issued.material.recipient.id));
            for version in original.paper.versions {
                impact.insert(CorrectionImpact::IssuedVersion(version.id));
            }
        }
        "actual_review" => {
            let original = crate::hub::actuals::reviews::original_on(tx, context.hub, op).await?;
            let crate::HubActualReviewOriginal::Departure(plan) = original.original else {
                return Err(corrupt_storage());
            };
            impact.insert(CorrectionImpact::Recipient(
                plan.comparison.scope.recipient.id,
            ));
        }
        _ => return Err(corrupt_storage()),
    }
    CorrectionImpactSet::new(impact.into_iter().collect()).map_err(|_| corrupt_storage())
}

fn parts(value: CorrectionImpact) -> (&'static str, HubRecordId) {
    match value {
        CorrectionImpact::Population(id) => ("population", id),
        CorrectionImpact::Source(id) => ("source", id),
        CorrectionImpact::Departure(id) => ("departure", id),
        CorrectionImpact::IssuedVersion(id) => ("issued", id),
        CorrectionImpact::Recipient(id) => ("recipient", id),
        CorrectionImpact::UnresolvedSource(id) => ("unknown_source", id),
        CorrectionImpact::UnresolvedRecipient(id) => ("unknown_recipient", id),
    }
}

pub(super) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    impact: &CorrectionImpactSet,
) -> Result<(), SafeError> {
    for value in impact.rows() {
        let (kind, id) = parts(*value);
        sqlx::query("INSERT INTO hub_correction_scope_links (correction_operation_id,target_kind,target_id,introduced_operation_id) VALUES (?,?,?,?)")
            .bind(operation.as_str()).bind(kind).bind(id.as_bytes().as_slice()).bind(operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}

pub(super) async fn verify_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    impact: &CorrectionImpactSet,
) -> Result<(), SafeError> {
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM hub_correction_scope_links WHERE correction_operation_id=? AND introduced_operation_id=?")
        .bind(operation.as_str()).bind(operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if count != i64::try_from(impact.rows().len()).map_err(|_| corrupt_storage())?
        || count > HUB_CORRECTION_IMPACTS as i64
    {
        return Err(corrupt_storage());
    }
    for value in impact.rows() {
        let (kind, id) = parts(*value);
        let found:i64=sqlx::query_scalar("SELECT count(*) FROM hub_correction_scope_links WHERE correction_operation_id=? AND introduced_operation_id=? AND target_kind=? AND target_id=?")
            .bind(operation.as_str()).bind(operation.as_str()).bind(kind).bind(id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if found != 1 {
            return Err(corrupt_storage());
        }
    }
    Ok(())
}
