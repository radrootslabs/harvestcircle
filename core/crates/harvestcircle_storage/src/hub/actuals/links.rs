//! Explicit zero-effect links retain the complete immutable known departure.
use super::{common::*, model::*, reports, reviews, write};
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{handling, reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::actuals::{ActualDuplicateLink, actual_duplicate_result_digest},
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use stock::read::OperationRecord;

pub(in crate::hub) struct Context {
    pub state: stock::read::StockState,
    pub catalog: harvestcircle_domain::hub::reference::ReferenceCatalog,
    pub report: harvestcircle_domain::hub::actuals::ActualReportRecord,
    pub interpretation: harvestcircle_domain::hub::actuals::ActualKnownInterpretation,
}

impl Context {
    pub fn material(
        &self,
        id: harvestcircle_domain::hub::reference::HubRecordId,
    ) -> harvestcircle_application::hub_submission::HubSubmittedActualLink {
        harvestcircle_application::hub_submission::HubSubmittedActualLink {
            id,
            report: self.report.clone(),
            known: self.interpretation.original.clone(),
            interpretation: Some(self.interpretation.clone()),
        }
    }
    pub fn plan(
        &self,
        command: &ActualDuplicateLink,
        id: harvestcircle_domain::hub::reference::HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<harvestcircle_domain::hub::actuals::ActualDuplicateLinkPlan, SafeError> {
        command
            .plan_with_interpretation(&self.report, &self.interpretation, None, id, actor, now)
            .map_err(error)
    }
}

pub(in crate::hub) async fn current_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubId,
    command: &ActualDuplicateLink,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    super::mode::epoch_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    let report = reports::by_id_on(tx, hub, command.report).await?.original;
    let reviewed: Option<String> = sqlx::query_scalar(
        "SELECT review_operation_id FROM hub_actual_report_reviews WHERE report_id=? LIMIT 1",
    )
    .bind(report.id.as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if reviewed.is_some() {
        return Err(stock::conflict());
    }
    let target = operation(command.known_operation)?;
    let interpretation = {
        let mut reader = super::proofs::OriginalReader::new(tx);
        let named = stock::read::operation_on(&mut reader, hub, &target)
            .await?
            .ok_or_else(stock::invalid_input)?;
        // Verify the named original before classifying caller scope. A damaged
        // denormalized trace or variant must not become merely invalid input.
        let scope_matches = match named.kind.as_str() {
            "actual_review" => {
                if !Box::pin(reader.actual_is_departure(hub, &named)).await? {
                    return Err(stock::invalid_input());
                }
                let known = Box::pin(reader.actual_known(hub, &named)).await?;
                known.trace == command.known_trace && known.variant.id == command.known_variant
            }
            "departure" => {
                let original = Box::pin(handling::departure::read::original_on(
                    &mut reader,
                    hub,
                    named,
                ))
                .await?
                .record;
                let material = &original
                    .paper
                    .versions
                    .last()
                    .ok_or_else(corrupt_storage)?
                    .material;
                material.lines.iter().any(|line| {
                    line.trace.id == command.known_trace && line.variant.id == command.known_variant
                })
            }
            _ => return Err(stock::invalid_input()),
        };
        if !scope_matches {
            return Err(stock::invalid_input());
        }
        Box::pin(
            crate::hub::corrections::selected_actual_interpretation_at_with_proofs_on(
                &mut reader,
                hub,
                command.known_trace,
                &target,
                command.known_variant,
                state.revision,
                catalog.revision(),
            ),
        )
        .await?
    };
    Ok(Context {
        state,
        catalog,
        report,
        interpretation,
    })
}

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ActualDuplicateLink,
    now: UnixTimestamp,
) -> Result<HubActualReviewReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let id = operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, id);
    if let Some(prior) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if prior.kind != "actual_review" || prior.actor != actor || prior.hash != hash {
            return Err(stock::conflict());
        }
        return reviews::original_on(tx, hub.id(), prior).await;
    }
    require_admission_on(tx, hub.id(), actor, &operation, "actual_review", &hash, now).await?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::ActualReview { physical: false },
    )
    .await?;
    let context = current_on(tx, hub.id(), &command).await?;
    let plan = context.plan(&command, id, actor, now)?;
    let Context {
        state,
        catalog,
        report,
        interpretation,
    } = context;
    let known = interpretation.original;
    let revision = harvestcircle_domain::hub::HubRevision::from_value(
        state
            .revision
            .value()
            .checked_add(1)
            .ok_or_else(stock::capacity)?,
    )
    .map_err(|_| stock::capacity())?;
    let digest =
        actual_duplicate_result_digest(&hash, &command, &plan, revision, catalog.revision())
            .map_err(error)?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "actual_review".into(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision,
        result: report.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    super::mode::persist_on(tx, hub.id(), &op).await?;
    let (kind, start, end) = occurrence_fields(known.occurred);
    let known_operation = super::common::operation(known.operation)?;
    let group = known.lines.first().ok_or_else(corrupt_storage)?.group;
    sqlx::query(concat!(
        "INSERT INTO hub_actual_reviews (operation_id,report_id,policy_version,review_kind,trace_id,group_id,variant_id,variant_revision,recipient_id,recipient_revision,packs,",
        "known_operation_id,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,verification,lineage_count,scope_count,result_hash) ",
        "VALUES (?,?,1,'link',?,?,?,?,?,?,?,?,?,?,?,?,0,0,?)"
    )).bind(operation.as_str()).bind(report.id.as_bytes().as_slice()).bind(known.trace.as_bytes().as_slice()).bind(group.as_bytes().as_slice())
        .bind(known.variant.id.as_bytes().as_slice()).bind(integer(known.variant.revision)?).bind(known.recipient.id.as_bytes().as_slice()).bind(integer(known.recipient.revision)?)
        .bind(i64::from(known.packs().map_err(error)?.value())).bind(known_operation.as_str()).bind(kind).bind(start).bind(end)
        .bind(command.verification.as_str()).bind(digest.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    write::close_report_on(tx, &operation, report.id).await?;
    advance_journal_only_on(tx, hub.id(), &state, &operation, revision).await?;
    let restored = reviews::original_on(tx, hub.id(), op).await?;
    if restored.original != HubActualReviewOriginal::Duplicate(Box::new(plan))
        || restored.lineage.is_some()
        || !restored.historical_holds.is_empty()
    {
        return Err(corrupt_storage());
    }
    Ok(restored)
}
