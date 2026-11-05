//! Full review originals are reconstructed at their sealed pre-effect cut.
//! Physical history uses the separate projection, never recursive review reads.
use super::{common::*, frozen, model::*, projection, reports};
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{handling, stock},
};
use harvestcircle_domain::{
    SafeError,
    hub::{
        HubId, HubRevision,
        actuals::{
            ActualDuplicateLink, ActualNewDepartureCommand, ActualReviewFacts,
            HistoricalHoldScopeContext, actual_duplicate_result_digest,
        },
        reference::SourceProvenance,
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::{Row, sqlite::SqliteRow};
use stock::read::{OperationRecord, record_id, revision_field, whole_packs};

impl crate::Database {
    /// Current owner reads an immutable original. This never authorizes an
    /// effect or decodes a pending admission into an executable command.
    pub async fn hub_actual_review_operation(
        &self,
        actor: harvestcircle_domain::PublicKey,
        operation: harvestcircle_application::DurableRequestId,
    ) -> Result<Option<HubActualReviewReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = crate::hub::reference::authorize_on(tx, actor).await?;
                    crate::hub::reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        None => Ok(None),
                        Some(op) => Ok(Some(original_on(tx, hub.id(), op).await?)),
                    }
                })
            })
            .await
            .map_err(crate::db::map_transaction_error)
    }
}

fn evidence(row: &SqliteRow, name: &str) -> Result<SourceProvenance, SafeError> {
    SourceProvenance::new(stock::read::text(row, name)?).map_err(|_| corrupt_storage())
}

async fn verify_capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    mode: super::mode::Mode,
    request: harvestcircle_application::hub_submission::HubSubmittedRequest<'_>,
    cut: HubRevision,
    material: &[harvestcircle_application::hub_submission::HubSubmittedMaterial],
) -> Result<(), SafeError> {
    let hash = crate::hub::submission::verify_recorded_capture_on(
        tx,
        hub,
        op,
        request,
        cut,
        op.catalog_revision,
        material,
    )
    .await?;
    if matches!(mode, super::mode::Mode::Qualified { capture, .. } if capture != hash) {
        return Err(corrupt_storage());
    }
    Ok(())
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubActualReviewReceipt, SafeError> {
    if op.kind != "actual_review" {
        return Err(stock::conflict());
    }
    // A genuine outer read phase. Recursive readers use the existing owner.
    Ok((*super::proofs::OriginalReader::new(tx)
        .actual_review(hub, op)
        .await?)
        .clone())
}

pub(in crate::hub) async fn rebuild_on(
    tx: &mut super::proofs::OriginalReader<'_, '_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubActualReviewReceipt, SafeError> {
    if op.kind != "actual_review" {
        return Err(stock::conflict());
    }
    let mode = super::mode::original_on(tx, hub, &op).await?;
    let row=sqlx::query(concat!(
        "SELECT policy_version,substr(review_kind,1,33) AS review_kind,substr(report_id,1,17) AS report_id,",
        "substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,substr(variant_id,1,17) AS variant_id,variant_revision,",
        "substr(recipient_id,1,17) AS recipient_id,recipient_revision,packs,substr(source_operation_id,1,37) AS source_operation_id,",
        "substr(source_group_id,1,17) AS source_group_id,substr(known_operation_id,1,37) AS known_operation_id,",
        "before_group_revision,after_group_revision,before_packs,before_causal_floor_unix_s,after_causal_floor_unix_s,",
        "substr(current_location_id,1,17) AS current_location_id,current_location_revision,",
        "occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,",
        "substr(CAST(source_evidence AS BLOB),1,513) AS source_evidence,substr(CAST(quantity_evidence AS BLOB),1,513) AS quantity_evidence,",
        "substr(CAST(recipient_evidence AS BLOB),1,513) AS recipient_evidence,substr(CAST(timing_evidence AS BLOB),1,513) AS timing_evidence,",
        "substr(CAST(verification AS BLOB),1,513) AS verification,lineage_count,scope_count,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_actual_reviews WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<i64, _>("policy_version")
        .map_err(|_| corrupt_storage())?
        != 1
        || record_id(&row, "report_id")? != op.result
    {
        return Err(corrupt_storage());
    }
    let report = reports::by_id_on(tx, hub, op.result).await?.original;
    if report.stock_revision.value() >= op.revision.value() || report.recorded_at > op.recorded_at {
        return Err(corrupt_storage());
    }
    let id = operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let saved: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    let kind: String = row.try_get("review_kind").map_err(|_| corrupt_storage())?;
    let (original, lineage, historical_holds, digest) = match kind.as_str() {
        "departure" => {
            let cut = HubRevision::from_value(
                op.revision
                    .value()
                    .checked_sub(1)
                    .ok_or_else(corrupt_storage)?,
            )
            .map_err(|_| corrupt_storage())?;
            let command = ActualNewDepartureCommand {
                facts: ActualReviewFacts {
                    report: report.id,
                    expected_stock_revision: cut,
                    expected_catalog_revision: op.catalog_revision,
                    group: record_id(&row, "group_id")?,
                    expected_group_revision: revision_field(&row, "before_group_revision")?,
                    packs: whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?,
                    recipient: record_id(&row, "recipient_id")?,
                    occurred: stock::read::occurrence(&row)?,
                    source_evidence: evidence(&row, "source_evidence")?,
                    quantity_evidence: evidence(&row, "quantity_evidence")?,
                    recipient_evidence: evidence(&row, "recipient_evidence")?,
                    timing_evidence: evidence(&row, "timing_evidence")?,
                },
                source_operation: operation_id(
                    &harvestcircle_application::DurableRequestId::parse(
                        row.try_get::<String, _>("source_operation_id")
                            .map_err(|_| corrupt_storage())?,
                    )
                    .map_err(|_| corrupt_storage())?,
                )
                .map_err(|_| corrupt_storage())?,
                source_group: record_id(&row, "source_group_id")?,
                duplicate_review_basis: evidence(&row, "verification")?,
            };
            let context = frozen::original_on(tx, hub, &op, &row, &command).await?;
            let plan = context
                .plan(&command, id, op.actor, op.recorded_at)
                .map_err(|_| corrupt_storage())?;
            let findings = context
                .treatment
                .holds()
                .iter()
                .map(|hold| {
                    HistoricalHoldScopeContext {
                        lineage: &context.lineage,
                        group: command.facts.group,
                        occurred: command.facts.occurred,
                        births: &context.births,
                        now: op.recorded_at,
                    }
                    .verify(hold)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| corrupt_storage())?;
            let floor = row
                .try_get::<Option<i64>, _>("after_causal_floor_unix_s")
                .map_err(|_| corrupt_storage())?
                .map(stock::read::timestamp)
                .transpose()?;
            if plan.stock_revision != op.revision
                || plan.reduction.after_revision != revision_field(&row, "after_group_revision")?
                || plan.after_causal_floor != floor
                || plan.comparison.scope.variant.id != record_id(&row, "variant_id")?
                || plan.comparison.scope.variant.revision
                    != revision_field(&row, "variant_revision")?
                || plan.comparison.scope.recipient.revision
                    != revision_field(&row, "recipient_revision")?
                || plan.comparison.scope.current_location.revision
                    != revision_field(&row, "current_location_revision")?
                || projection::known_on(tx, hub, &op).await?
                    != plan.physical_projection().map_err(|_| corrupt_storage())?
            {
                return Err(corrupt_storage());
            }
            let digest = context
                .digest(&op.hash, &command, &findings, &plan)
                .map_err(|_| corrupt_storage())?;
            let mut material = vec![
                harvestcircle_application::hub_submission::HubSubmittedMaterial::ActualReview(
                    Box::new(context.material(id)),
                ),
            ];
            crate::hub::submission::capture_references(
                &context.catalog,
                [
                    context.group.variant,
                    context.group.location,
                    command.facts.recipient,
                ]
                .into_iter()
                .collect(),
                &mut material,
            )
            .map_err(|_| corrupt_storage())?;
            verify_capture_on(
                tx,
                hub,
                &op,
                mode,
                harvestcircle_application::hub_submission::HubSubmittedRequest::ActualReview(
                    &command,
                ),
                cut,
                &material,
            )
            .await?;
            verify_work_changes_on(tx, &op, &plan.reduction.work_changes).await?;
            (
                HubActualReviewOriginal::Departure(Box::new(plan)),
                Some(context.lineage),
                findings,
                digest,
            )
        }
        "link" => {
            let trace = record_id(&row, "trace_id")?;
            let known_operation = harvestcircle_application::DurableRequestId::parse(
                row.try_get::<String, _>("known_operation_id")
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?;
            let cut = HubRevision::from_value(
                op.revision
                    .value()
                    .checked_sub(1)
                    .ok_or_else(corrupt_storage)?,
            )
            .map_err(|_| corrupt_storage())?;
            let command = ActualDuplicateLink {
                report: report.id,
                known_operation: operation_id(&known_operation).map_err(|_| corrupt_storage())?,
                known_trace: trace,
                known_variant: record_id(&row, "variant_id")?,
                verification: evidence(&row, "verification")?,
            };
            let interpretation = if mode.qualified() {
                Some(
                    Box::pin(
                        crate::hub::corrections::selected_actual_interpretation_at_with_proofs_on(
                            tx,
                            hub,
                            trace,
                            &known_operation,
                            command.known_variant,
                            cut,
                            op.catalog_revision,
                        ),
                    )
                    .await?,
                )
            } else {
                None
            };
            let known = if let Some(record) = &interpretation {
                record.original.clone()
            } else {
                handling::actuals::known_departures_on(tx, hub, trace, cut)
                    .await?
                    .into_iter()
                    .find(|known| operation(known.operation).is_ok_and(|id| id == known_operation))
                    .ok_or_else(corrupt_storage)?
            };
            let plan = match &interpretation {
                Some(record) => command.plan_with_interpretation(
                    &report,
                    record,
                    None,
                    id,
                    op.actor,
                    op.recorded_at,
                ),
                None => command.plan(&report, &known, None, id, op.actor, op.recorded_at),
            }
            .map_err(|_| corrupt_storage())?;
            verify_capture_on(
                tx,
                hub,
                &op,
                mode,
                harvestcircle_application::hub_submission::HubSubmittedRequest::ActualLink(
                    &command,
                ),
                cut,
                &[
                    harvestcircle_application::hub_submission::HubSubmittedMaterial::ActualLink(
                        Box::new(
                            harvestcircle_application::hub_submission::HubSubmittedActualLink {
                                id,
                                report: report.clone(),
                                known: known.clone(),
                                interpretation,
                            },
                        ),
                    ),
                ],
            )
            .await?;
            let group = record_id(&row, "group_id")?;
            if known.variant.revision != revision_field(&row, "variant_revision")?
                || known.recipient.id != record_id(&row, "recipient_id")?
                || known.recipient.revision != revision_field(&row, "recipient_revision")?
                || known.packs().map_err(|_| corrupt_storage())?
                    != whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?
                || known.occurred != stock::read::occurrence(&row)?
                || known.lines.first().is_none_or(|line| line.group != group)
            {
                return Err(corrupt_storage());
            }
            let events: i64 =
                sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
                    .bind(op.id.as_str())
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
            let scopes:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_actual_review_lineage WHERE operation_id=?)+(SELECT count(*) FROM hub_actual_review_work_scope WHERE operation_id=?)")
                .bind(op.id.as_str()).bind(op.id.as_str()).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
            if events != 0
                || scopes != 0
                || row
                    .try_get::<i64, _>("lineage_count")
                    .map_err(|_| corrupt_storage())?
                    != 0
                || row
                    .try_get::<i64, _>("scope_count")
                    .map_err(|_| corrupt_storage())?
                    != 0
            {
                return Err(corrupt_storage());
            }
            verify_work_changes_on(tx, &op, &[]).await?;
            let digest = actual_duplicate_result_digest(
                &op.hash,
                &command,
                &plan,
                op.revision,
                op.catalog_revision,
            )
            .map_err(|_| corrupt_storage())?;
            (
                HubActualReviewOriginal::Duplicate(Box::new(plan)),
                None,
                Vec::new(),
                digest,
            )
        }
        _ => return Err(corrupt_storage()),
    };
    if digest.as_slice() != saved.as_slice() {
        return Err(corrupt_storage());
    }
    let closed:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE report_id=? AND operation_id=? AND review_operation_id=?")
        .bind(report.id.as_bytes().as_slice()).bind(operation(report.operation).map_err(|_|corrupt_storage())?.as_str()).bind(op.id.as_str())
        .fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if closed != 1 {
        return Err(corrupt_storage());
    }
    Ok(HubActualReviewReceipt {
        operation: op.id,
        revision: op.revision,
        catalog_revision: op.catalog_revision,
        original,
        lineage,
        historical_holds,
    })
}

/// The result seal commits to the expected work deltas. Independently require
/// their exact journal rows, including absence on a zero-effect duplicate link.
pub(in crate::hub) async fn verify_work_changes_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    op: &OperationRecord,
    expected: &[harvestcircle_domain::hub::departure::DepartureWorkChange],
) -> Result<(), SafeError> {
    let rows = sqlx::query(concat!(
        "SELECT substr(work_id,1,17) AS work_id,before_revision,after_revision,",
        "before_completed,before_no_longer_required,before_remaining,",
        "after_completed,after_no_longer_required,after_remaining ",
        "FROM hub_work_changes WHERE operation_id=? ORDER BY work_id LIMIT 65"
    ))
    .bind(op.id.as_str())
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if rows.len() != expected.len() || rows.len() > 64 {
        return Err(corrupt_storage());
    }
    for row in rows {
        let id = record_id(&row, "work_id")?;
        let change = expected
            .iter()
            .find(|change| change.before.id == id)
            .ok_or_else(corrupt_storage)?;
        if revision_field(&row, "before_revision")? != change.before.revision
            || revision_field(&row, "after_revision")? != change.after.revision
        {
            return Err(corrupt_storage());
        }
        for (name, value) in [
            ("before_completed", change.before.quantity.completed()),
            (
                "before_no_longer_required",
                change.before.quantity.no_longer_required(),
            ),
            ("before_remaining", change.before.quantity.remaining()),
            ("after_completed", change.after.quantity.completed()),
            (
                "after_no_longer_required",
                change.after.quantity.no_longer_required(),
            ),
            ("after_remaining", change.after.quantity.remaining()),
        ] {
            if whole_packs(row.try_get(name).map_err(|_| corrupt_storage())?)? != value {
                return Err(corrupt_storage());
            }
        }
    }
    Ok(())
}
