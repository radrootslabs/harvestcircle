//! Canonical interpretation roots consume one row per affected trace. This
//! lower ownership inventory is not a full original proof or execution grant.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    corrections::{CorrectionObservedFacts, FoundCorrectionCommand, HUB_CORRECTION_ROWS},
    stock::HUB_STOCK_EVENT_CAPACITY,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Eq, PartialEq)]
struct CountHeader {
    actor: [u8; 32],
    intent: [u8; 32],
    stock: u64,
    catalog: u64,
    result: [u8; 16],
    recorded: i64,
}
type CountTraces = BTreeMap<String, (CountHeader, HubRecordId)>;

#[cfg(test)]
mod tests;

pub(super) async fn ensure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<(), SafeError> {
    let cut = stock::read::journal_state_on(tx, hub).await?.revision;
    if through_on(tx, hub, trace, cut).await? >= HUB_CORRECTION_ROWS {
        return Err(stock::capacity());
    }
    Ok(())
}

/// Retain stronger existing hub-wide guards without substituting them for the
/// complete per-trace cut or allowing separately stored associations to escape.
pub(super) async fn ensure_hub_on(tx: &mut ServiceSqliteTransaction<'_>) -> Result<(), SafeError> {
    let mut count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_corrections")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 22 {
        let associated: i64 =
            sqlx::query_scalar("SELECT count(*) FROM hub_count_return_associations")
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
        count = count.checked_add(associated).ok_or_else(corrupt_storage)?;
    }
    if count < 0 {
        return Err(corrupt_storage());
    }
    if count >= HUB_CORRECTION_ROWS as i64 {
        return Err(stock::capacity());
    }
    Ok(())
}

/// Existing originals must have had room at their own pre-effect cut. This is
/// immutable consistency validation, never a check of today's effect permission.
pub(super) async fn require_prior_room_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<(), SafeError> {
    if through_on(tx, hub, trace, cut).await? >= HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    Ok(())
}

// Erase the bounded inventory future at this shared caller boundary. New root
// families must not inflate every existing mutation's native stack frame.
pub(super) fn through_on<'a, 'c>(
    tx: &'a mut ServiceSqliteTransaction<'c>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, SafeError>> + Send + 'a>> {
    through_from_on(tx, hub, trace, cut, None)
}

/// A seed is supplied only by the unchanged read-phase owner after a completed
/// inventory. Writer callers always start with None through through_on.
pub(in crate::hub) fn through_from_on<'a, 'c>(
    tx: &'a mut ServiceSqliteTransaction<'c>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
    seed: Option<crate::hub::actuals::proofs::CountRowSeed>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, SafeError>> + Send + 'a>> {
    Box::pin(async move {
        let seed = seed.map(|seed| seed.point(hub, trace)).transpose()?;
        if seed.is_some_and(|(prior, total)| {
            prior.value() > cut.value() || total > HUB_CORRECTION_ROWS
        }) {
            return Err(corrupt_storage());
        }
        let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
        let (operations,maximum): (i64,i64) = sqlx::query_as("SELECT count(*),COALESCE(max(stock_revision),1) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?")
            .bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        if maximum != through || operations.checked_add(1) != Some(through) {
            return Err(corrupt_storage());
        }
        let invalid: i64 = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
            "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) AND ",
            "(o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>CASE c.family ",
            "WHEN 'source' THEN 'correction_source' WHEN 'recipient' THEN 'correction_recipient' ",
            "WHEN 'additional' THEN 'correction_additional' WHEN 'found' THEN 'correction_found' ",
            "WHEN 'count_explain' THEN 'correction_count_explain' WHEN 'count_classify' THEN 'correction_count_classify' ",
            "WHEN 'document' THEN 'correction_document' WHEN 'follow_up' THEN 'correction_follow_up' END)"
        )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        if invalid != 0 {
            return Err(corrupt_storage());
        }
        if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 22 {
            let invalid: i64 = sqlx::query_scalar(concat!(
                "SELECT count(*) FROM hub_count_return_associations a LEFT JOIN hub_stock_operations o ON o.operation_id=a.operation_id ",
                "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) ",
                "AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>'count_return_associate')"
            )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
            if invalid != 0 {
                return Err(corrupt_storage());
            }
        }
        let ids: Vec<(String, i64)> = sqlx::query_as(concat!(
            "SELECT substr(operation_id,1,37),stock_revision FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=? ",
            "AND ((kind LIKE 'correction_%' AND kind<>'correction_observe') OR kind='count_return_associate') ",
            "ORDER BY stock_revision LIMIT 32769"
        )).bind(hub.as_bytes().as_slice()).bind(through).fetch_all(&mut *tx).await.map_err(|_| storage_unavailable())?;
        if ids.len() > HUB_STOCK_EVENT_CAPACITY {
            return Err(corrupt_storage());
        }
        let mut seen = BTreeSet::new();
        // Private to this lexically read-only scan; never passed to a writer or
        // retained by a transaction/reader. Store only bounded header/trace data.
        let mut count_traces = CountTraces::new();
        let mut total = seed.map_or(0, |(_, total)| total);
        for (id, revision) in ids {
            let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
            if !seen.insert(id.as_str().to_owned()) {
                return Err(corrupt_storage());
            }
            let revision = u64::try_from(revision).map_err(|_| corrupt_storage())?;
            if seed.is_some_and(|(prior, _)| revision <= prior.value()) {
                continue;
            }
            let op = stock::read::operation_on(tx, hub, &id)
                .await?
                .ok_or_else(corrupt_storage)?;
            if op.revision.value() != revision || op.revision.value() > cut.value() {
                return Err(corrupt_storage());
            }
            // Attribution precedes selection: damaged final indices cannot
            // remove a root from its original trace's accounting budget.
            let traces = traces_on(tx, hub, op, &mut count_traces).await?;
            if traces.contains(&trace) {
                total = total.checked_add(1).ok_or_else(corrupt_storage)?;
                if total > HUB_CORRECTION_ROWS {
                    return Err(corrupt_storage());
                }
            }
        }
        Ok(total)
    })
}

async fn count_trace_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    account: &DurableRequestId,
    memo: &mut CountTraces,
) -> Result<HubRecordId, SafeError> {
    let op = stock::read::operation_on(tx, hub, account)
        .await?
        .ok_or_else(corrupt_storage)?;
    if op.kind != "count_post" {
        return Err(corrupt_storage());
    }
    let header = CountHeader {
        actor: *op.actor.as_bytes(),
        intent: op.hash,
        stock: op.revision.value(),
        catalog: op.catalog_revision.value(),
        result: *op.result.as_bytes(),
        recorded: op.recorded_at.as_seconds(),
    };
    if let Some((saved, trace)) = memo.get(account.as_str()) {
        return if *saved == header {
            Ok(*trace)
        } else {
            Err(corrupt_storage())
        };
    }
    let trace = Box::pin(crate::hub::counts::posting::original_on(tx, hub, op))
        .await?
        .original
        .before
        .trace;
    if memo.len() < HUB_CORRECTION_ROWS {
        memo.insert(account.as_str().to_owned(), (header, trace));
    }
    Ok(trace)
}

async fn event_traces_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    target: &DurableRequestId,
    cut: HubRevision,
    memo: &mut CountTraces,
) -> Result<BTreeSet<HubRecordId>, SafeError> {
    let op = stock::read::operation_on(tx, hub, target)
        .await?
        .ok_or_else(corrupt_storage)?;
    if op.revision.value() > cut.value() {
        return Err(corrupt_storage());
    }
    let traces = match op.kind.as_str() {
        "departure" => Box::pin(handling::departure::read::original_on(tx, hub, op))
            .await?
            .record
            .consumed
            .into_iter()
            .map(|line| line.before.trace)
            .collect(),
        "actual_review" => {
            BTreeSet::from([crate::hub::actuals::projection::known_on(tx, hub, &op)
                .await?
                .trace])
        }
        "correction_count_explain" => {
            let root = super::count_account::root_on(tx, hub, op).await?;
            BTreeSet::from([count_trace_on(tx, hub, &root.account, memo).await?])
        }
        _ => return Err(corrupt_storage()),
    };
    if traces.is_empty() || traces.len() > 64 {
        return Err(corrupt_storage());
    }
    Ok(traces)
}

async fn traces_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
    memo: &mut CountTraces,
) -> Result<BTreeSet<HubRecordId>, SafeError> {
    if matches!(
        op.kind.as_str(),
        "correction_count_explain" | "correction_count_classify"
    ) {
        let root = super::count_account::root_on(tx, hub, op).await?;
        crate::hub::submission::verify_recorded_input_identity_on(
            tx,
            hub,
            &root.op,
            root.command.request(),
        )
        .await?;
        return Ok(BTreeSet::from([count_trace_on(
            tx,
            hub,
            &root.account,
            memo,
        )
        .await?]));
    }
    if op.kind == "count_return_associate" {
        let root = super::count_return_ledger::read_on(tx, hub, op).await?;
        let explained = crate::hub::actuals::common::operation(root.command.explanation_operation)
            .map_err(|_| corrupt_storage())?;
        let explanation = stock::read::operation_on(tx, hub, &explained)
            .await?
            .ok_or_else(corrupt_storage)?;
        if explanation.kind != "correction_count_explain"
            || explanation.revision.value() >= root.op.revision.value()
        {
            return Err(corrupt_storage());
        }
        let explanation = super::count_account::root_on(tx, hub, explanation).await?;
        if explanation.account != root.count {
            return Err(corrupt_storage());
        }
        crate::hub::submission::verify_recorded_input_identity_on(
            tx,
            hub,
            &root.op,
            HubSubmittedRequest::CountReturnAssociation(&root.command),
        )
        .await?;
        return Ok(BTreeSet::from([count_trace_on(
            tx,
            hub,
            &explanation.account,
            memo,
        )
        .await?]));
    }
    if op.kind == "correction_additional" {
        let input = super::additional_record::input_on(tx, hub, &op).await?;
        crate::hub::submission::verify_recorded_input_identity_on(
            tx,
            hub,
            &op,
            HubSubmittedRequest::AdditionalQuantity(&input.command),
        )
        .await?;
        let target = crate::hub::actuals::common::operation(input.command.facts.target_operation)
            .map_err(|_| corrupt_storage())?;
        let line = super::quantity::original_on(
            tx,
            hub,
            &target,
            input.command.facts.target_group,
            input.command.facts.expected_stock_revision,
        )
        .await?;
        return Ok(BTreeSet::from([line.original.trace]));
    }
    let row = sqlx::query(concat!(
        "SELECT substr(correction_id,1,17) AS correction_id,substr(family,1,33) AS family,substr(subject_kind,1,33) AS subject_kind,",
        "substr(subject_id,1,17) AS subject_id,substr(target_operation_id,1,37) AS target,substr(observation_operation_id,1,37) AS observed,",
        "payload_schema,substr(payload,1,4097) AS payload,substr(result_hash,1,33) AS result_hash FROM hub_corrections WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<i64, _>("payload_schema")
        .map_err(|_| corrupt_storage())?
        != 1
        || stock::read::record_id(&row, "correction_id")? != op.result
        || row
            .try_get::<Vec<u8>, _>("result_hash")
            .map_err(|_| corrupt_storage())?
            .len()
            != 32
    {
        return Err(corrupt_storage());
    }
    let payload: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let target = DurableRequestId::parse(
        row.try_get::<String, _>("target")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let cut = HubRevision::from_value(
        op.revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let check = |family: &str,
                 subject_kind: &str,
                 subject: HubRecordId,
                 result: HubRecordId,
                 stock: HubRevision,
                 catalog: HubRevision,
                 hash: [u8; 32]|
     -> Result<(), SafeError> {
        if row
            .try_get::<String, _>("family")
            .map_err(|_| corrupt_storage())?
            != family
            || row
                .try_get::<String, _>("subject_kind")
                .map_err(|_| corrupt_storage())?
                != subject_kind
            || stock::read::record_id(&row, "subject_id")? != subject
            || op.result != result
            || stock != cut
            || catalog != op.catalog_revision
            || hash != op.hash
        {
            return Err(corrupt_storage());
        }
        Ok(())
    };
    match op.kind.as_str() {
        "correction_source" => {
            let command = super::source_payload::decode(&payload)?;
            check(
                "source",
                "population",
                command.trace,
                own,
                command.expected_stock_revision,
                command.expected_catalog_revision,
                command.fingerprint(hub, op.actor, own),
            )?;
            let birth: String = sqlx::query_scalar(
                "SELECT substr(operation_id,1,37) FROM hub_receipts WHERE trace_id=?",
            )
            .bind(command.trace.as_bytes().as_slice())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?
            .ok_or_else(corrupt_storage)?;
            if birth != target.as_str() {
                return Err(corrupt_storage());
            }
            crate::hub::submission::verify_recorded_input_identity_on(
                tx,
                hub,
                &op,
                HubSubmittedRequest::SourceAttribution(&command),
            )
            .await?;
            Ok(BTreeSet::from([command.trace]))
        }
        "correction_recipient" => {
            let command = super::recipient_payload::decode(&payload)?;
            check(
                "recipient",
                "event",
                command.departure_operation,
                own,
                command.expected_stock_revision,
                command.expected_catalog_revision,
                command.fingerprint(hub, op.actor, own),
            )?;
            if crate::hub::actuals::common::operation(command.departure_operation)
                .map_err(|_| corrupt_storage())?
                != target
            {
                return Err(corrupt_storage());
            }
            crate::hub::submission::verify_recorded_input_identity_on(
                tx,
                hub,
                &op,
                HubSubmittedRequest::RecipientAttribution(&command),
            )
            .await?;
            event_traces_on(tx, hub, &target, cut, memo).await
        }
        "correction_found" => {
            let observed = DurableRequestId::parse(
                row.try_get::<String, _>("observed")
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?;
            let observed_op = stock::read::operation_on(tx, hub, &observed)
                .await?
                .ok_or_else(corrupt_storage)?;
            if observed_op.revision.value() > cut.value() {
                return Err(corrupt_storage());
            }
            let observation = super::original_on(tx, hub, observed_op).await?.original;
            let command = FoundCorrectionCommand {
                observation_operation: observation.operation,
                expected_stock_revision: cut,
                expected_catalog_revision: op.catalog_revision,
            };
            let CorrectionObservedFacts::Found {
                target_operation,
                target_group,
                ..
            } = observation.observation.facts
            else {
                return Err(corrupt_storage());
            };
            check(
                "found",
                "line",
                target_group,
                harvestcircle_domain::hub::corrections::found_correction_id(own)
                    .map_err(|_| corrupt_storage())?,
                cut,
                op.catalog_revision,
                command.fingerprint(hub, op.actor, own),
            )?;
            if super::found_record::payload(&command)? != payload
                || crate::hub::actuals::common::operation(target_operation)
                    .map_err(|_| corrupt_storage())?
                    != target
            {
                return Err(corrupt_storage());
            }
            crate::hub::submission::verify_recorded_input_identity_on(
                tx,
                hub,
                &op,
                HubSubmittedRequest::FoundCorrection(&command),
            )
            .await?;
            let line = super::quantity::original_on(tx, hub, &target, target_group, cut).await?;
            Ok(BTreeSet::from([line.original.trace]))
        }
        _ => Err(corrupt_storage()),
    }
}
