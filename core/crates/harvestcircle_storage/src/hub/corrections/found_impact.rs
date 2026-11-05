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
    let mut reader = OriginalReader::new(tx);
    Box::pin(load_with_proofs_on(&mut reader, context)).await
}

pub(super) async fn load_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    context: &super::found_context::Context,
) -> Result<CorrectionImpactSet, SafeError> {
    for_line_with_proofs_on(
        tx,
        context.hub,
        context.stock_revision,
        &context.funding.quantity.original(),
    )
    .await
}

pub(super) async fn for_line_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: HubRevision,
    target: &harvestcircle_domain::hub::corrections::OriginalDepartureQuantity,
) -> Result<CorrectionImpactSet, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(for_line_with_proofs_on(&mut reader, hub, cut, target)).await
}

pub(super) async fn for_line_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: HubRevision,
    target: &harvestcircle_domain::hub::corrections::OriginalDepartureQuantity,
) -> Result<CorrectionImpactSet, SafeError> {
    let operation =
        crate::hub::actuals::common::operation(target.operation).map_err(|_| corrupt_storage())?;
    let recipient_records = Box::pin(super::recipient_records_with_proofs_on(tx, hub, cut)).await?;
    let mut impact = BTreeSet::from([
        CorrectionImpact::Population(target.trace),
        CorrectionImpact::Departure(target.operation),
    ]);
    let population = handling::read::trace_on(tx, hub, target.trace).await?;
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
    let source_history = Box::pin(super::source_history_with_proofs_on(
        tx,
        hub,
        target.trace,
        cut,
    ))
    .await?;
    for correction in source_history {
        match &correction.after.source {
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
    }
    let op = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    match op.kind.as_str() {
        "departure" => {
            let original = handling::departure::read::original_on(tx, hub, op)
                .await?
                .record;
            let issued = original.paper.versions.last().ok_or_else(corrupt_storage)?;
            impact.insert(CorrectionImpact::Recipient(issued.material.recipient.id));
            for version in original.paper.versions {
                impact.insert(CorrectionImpact::IssuedVersion(version.id));
            }
        }
        "actual_review" => {
            let known = tx.actual_known(hub, &op).await?;
            impact.insert(CorrectionImpact::Recipient(known.recipient.id));
        }
        _ => return Err(corrupt_storage()),
    }
    for row in recipient_records
        .iter()
        .filter(|r| r.original.operation == target.operation)
    {
        match &row.after {
            harvestcircle_domain::hub::corrections::AmendedRecipient::Unknown => {
                impact.insert(CorrectionImpact::UnresolvedRecipient(target.operation));
            }
            harvestcircle_domain::hub::corrections::AmendedRecipient::Known(party) => {
                impact.insert(CorrectionImpact::Recipient(party.id));
            }
        }
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
