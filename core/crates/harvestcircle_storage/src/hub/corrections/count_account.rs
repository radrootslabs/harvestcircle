//! Shallow count classification history below full original reconstruction.
//! This layer authenticates native identity and projections, not complete prior
//! decision context. It never calls full count originals or known inventories.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        CorrectionObservedFacts, CountExplanationAmendment, CountExplanationReturns,
        CountLossExplanation, CountLossQuantityAccount, HUB_CORRECTION_ROWS,
        OriginalCountLossQuantity, count_classification_id, count_explanation_id,
    },
    counts::{CountEffect, CountPostPlan},
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) struct History {
    pub original: OriginalCountLossQuantity,
    pub explanations: Vec<CountLossExplanation>,
    pub amendments: Vec<CountExplanationAmendment>,
}
impl History {
    /// The caller supplies separately verified established-return totals. An
    /// empty history is not evidence that current linked returns are absent.
    pub fn with_returns(
        self,
        returns: Vec<CountExplanationReturns>,
    ) -> Result<CountLossQuantityAccount, SafeError> {
        CountLossQuantityAccount::restore_corrected(
            self.original,
            self.explanations,
            self.amendments,
            returns,
        )
        .map_err(|_| corrupt_storage())
    }
}

pub(super) struct Root {
    pub op: OperationRecord,
    pub command: super::count_payload::Command,
    pub target: DurableRequestId,
    pub account: DurableRequestId,
    pub previous: Option<HubRecordId>,
    pub account_previous: Option<HubRecordId>,
    pub report: Option<HubRecordId>,
    pub observation: Option<HubRecordId>,
    pub subject: HubRecordId,
    pub result_hash: [u8; 32],
    pub admission: [u8; 32],
}
fn optional_operation(
    row: &sqlx::sqlite::SqliteRow,
    field: &str,
) -> Result<Option<HubRecordId>, SafeError> {
    row.try_get::<Option<String>, _>(field)
        .map_err(|_| corrupt_storage())?
        .map(|s| {
            DurableRequestId::parse(s)
                .map_err(|_| corrupt_storage())
                .and_then(|id| {
                    crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())
                })
        })
        .transpose()
}
pub(super) async fn root_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<Root, SafeError> {
    use super::count_payload::Command;
    let row=sqlx::query(concat!(
        "SELECT substr(correction_id,1,17) AS correction_id,family,subject_kind,substr(subject_id,1,17) AS subject_id,",
        "substr(target_operation_id,1,37) AS target,substr(previous_operation_id,1,37) AS previous,",
        "substr(account_operation_id,1,37) AS account,substr(account_previous_operation_id,1,37) AS account_previous,",
        "substr(report_id,1,17) AS report,substr(observation_operation_id,1,37) AS observation,",
        "payload_schema,substr(payload,1,65537) AS payload,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_corrections WHERE operation_id=?"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let raw: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let command = super::count_payload::decode(&raw)?;
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let (cut, catalog) = command.cuts();
    let family = match command {
        Command::Explanation(_) => "count_explain",
        Command::Classification(_) => "count_classify",
    };
    let subject_kind = match command {
        Command::Explanation(_) => "count",
        Command::Classification(_) => "classification",
    };
    let result = match command {
        Command::Explanation(_) => count_explanation_id(own),
        Command::Classification(_) => count_classification_id(own),
    }
    .map_err(|_| corrupt_storage())?;
    if row
        .try_get::<i64, _>("payload_schema")
        .map_err(|_| corrupt_storage())?
        != 1
        || row
            .try_get::<String, _>("family")
            .map_err(|_| corrupt_storage())?
            != family
        || row
            .try_get::<String, _>("subject_kind")
            .map_err(|_| corrupt_storage())?
            != subject_kind
        || command.kind() != op.kind
        || cut.value().checked_add(1) != Some(op.revision.value())
        || catalog != op.catalog_revision
        || command.fingerprint(hub, op.actor, own) != op.hash
        || result != op.result
        || stock::read::record_id(&row, "correction_id")? != result
    {
        return Err(corrupt_storage());
    }
    let parse = |name| {
        DurableRequestId::parse(
            row.try_get::<String, _>(name)
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())
    };
    let target = parse("target")?;
    let account = parse("account")?;
    let original = stock::read::operation_on(tx, hub, &account)
        .await?
        .ok_or_else(corrupt_storage)?;
    if original.kind != "count_post"
        || original.revision.value() > cut.value()
        || original.catalog_revision.value() > catalog.value()
        || original.recorded_at > op.recorded_at
    {
        return Err(corrupt_storage());
    }
    let previous = optional_operation(&row, "previous")?;
    let account_previous = optional_operation(&row, "account_previous")?;
    let report = stock::read::optional_id(&row, "report")?;
    let observation = optional_operation(&row, "observation")?;
    let subject = stock::read::record_id(&row, "subject_id")?;
    match &command {
        Command::Explanation(command) => {
            if target != account
                || subject != original.result
                || command.facts.count_reconciliation != original.result
                || command.facts.expected_prior != account_previous
                || report != Some(command.facts.report)
                || observation.is_some()
            {
                return Err(corrupt_storage());
            }
        }
        Command::Classification(command) => {
            let target_op = stock::read::operation_on(tx, hub, &target)
                .await?
                .ok_or_else(corrupt_storage)?;
            let target_account:Option<String>=sqlx::query_scalar("SELECT account_operation_id FROM hub_corrections WHERE operation_id=? AND family='count_explain'")
                .bind(target.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.flatten();
            if target_op.kind != "correction_count_explain"
                || target_op.revision.value() > cut.value()
                || target_op.catalog_revision.value() > catalog.value()
                || target_op.recorded_at > op.recorded_at
                || target_account.as_deref() != Some(account.as_str())
                || subject
                    != crate::hub::actuals::common::operation_id(&target)
                        .map_err(|_| corrupt_storage())?
                || command.expected_prior != previous
                || command.expected_account_prior != account_previous
                || observation != Some(command.observation_operation)
            {
                return Err(corrupt_storage());
            }
            // The submitted classification names its immutable observation,
            // not the denormalized target index. Corroborate that target before
            // filtering by account so a coordinated target/account rewrite
            // cannot hide the last amendment from its original account.
            let observed_id = crate::hub::actuals::common::operation(command.observation_operation)
                .map_err(|_| corrupt_storage())?;
            let observed_op = stock::read::operation_on(tx, hub, &observed_id)
                .await?
                .ok_or_else(corrupt_storage)?;
            if observed_op.revision.value() > cut.value()
                || observed_op.revision.value() <= target_op.revision.value()
                || observed_op.catalog_revision.value() > catalog.value()
                || observed_op.recorded_at > op.recorded_at
            {
                return Err(corrupt_storage());
            }
            let observed = super::original_on(tx, hub, observed_op).await?.original;
            let CorrectionObservedFacts::CountClassification {
                explanation_operation,
                report: observed_report,
                ..
            } = observed.observation.facts
            else {
                return Err(corrupt_storage());
            };
            if explanation_operation != subject || observed_report != report {
                return Err(corrupt_storage());
            }
        }
    }
    if let Some(report_id) = report {
        let report = crate::hub::actuals::reports::by_id_on(tx, hub, report_id)
            .await?
            .original;
        let report_operation = crate::hub::actuals::common::operation(report.operation)
            .map_err(|_| corrupt_storage())?;
        let closed:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE report_id=? AND operation_id=? AND review_operation_id=?")
            .bind(report_id.as_bytes().as_slice()).bind(report_operation.as_str()).bind(op.id.as_str())
            .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if report.stock_revision.value() > cut.value()
            || report.catalog_revision.value() > catalog.value()
            || report.recorded_at > op.recorded_at
            || closed != 1
        {
            return Err(corrupt_storage());
        }
    }
    let admission =
        crate::hub::submission::count_projection_admission_hash_on(tx, hub, &op, command.request())
            .await?;
    let result_hash: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    Ok(Root {
        op,
        command,
        target,
        account,
        previous,
        account_previous,
        report,
        observation,
        subject,
        result_hash: result_hash.try_into().map_err(|_| corrupt_storage())?,
        admission,
    })
}

/// Fixed-purpose projection binding includes the root result and immutable
/// admission identity. A resealed shallow row is still not full original proof.
pub(super) fn projection_seal<T: serde::Serialize>(
    hub: HubId,
    root: &Root,
    count: &CountPostPlan,
    value: &T,
) -> Result<[u8; 32], SafeError> {
    let bytes = serde_json::to_vec(&(
        hub,
        root.op.id.as_str(),
        &root.op.kind,
        root.op.actor,
        root.op.hash,
        root.op.revision,
        root.op.catalog_revision,
        root.op.result,
        root.op.recorded_at,
        &root
            .command
            .request()
            .archive()
            .map_err(|_| corrupt_storage())?
            .digest(),
        root.result_hash,
        root.admission,
        count,
        value,
    ))
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.count.account.projection.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(super) async fn history_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    count: &CountPostPlan,
    cut: HubRevision,
) -> Result<History, SafeError> {
    use super::count_payload::Command;
    count.validate().map_err(|_| corrupt_storage())?;
    let CountEffect::Shortage { packs, .. } = count.effect else {
        return Err(corrupt_storage());
    };
    if count.count.scope.hub != hub || count.stock_revision.value() > cut.value() {
        return Err(corrupt_storage());
    }
    let account =
        crate::hub::actuals::common::operation(count.operation).map_err(|_| corrupt_storage())?;
    let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    // Validate root completeness before selecting the denormalized account.
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.family IN ('count_explain','count_classify') AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) ",
        "AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>CASE c.family WHEN 'count_explain' THEN 'correction_count_explain' ELSE 'correction_count_classify' END)) + ",
        "(SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_corrections c ON c.operation_id=o.operation_id ",
        "WHERE o.kind IN ('correction_count_explain','correction_count_classify') AND (o.hub_id<>?1 OR o.stock_revision<=?2) ",
        "AND (o.hub_id<>?1 OR c.operation_id IS NULL OR c.family<>CASE o.kind WHEN 'correction_count_explain' THEN 'count_explain' ELSE 'count_classify' END))"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(c.operation_id,1,37) FROM hub_corrections c JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.family IN ('count_explain','count_classify') AND o.hub_id=? AND o.stock_revision<=? ",
        "ORDER BY o.stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    let mut history = History {
        original: OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: count.before.trace,
            variant: count.before.variant,
            packs,
        },
        explanations: Vec::new(),
        amendments: Vec::new(),
    };
    let mut account_head = None;
    let mut explanation_head = None;
    let mut targets = BTreeMap::new();
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let root = root_on(tx, hub, op).await?;
        // Canonical ownership is checked for every included root before using
        // its account index. A changed final row cannot disappear from its
        // original account merely because no later predecessor references it.
        if root.account != account {
            continue;
        }
        let own = crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())?;
        if root.account != account
            || root.account_previous != account_head
            || root.op.revision.value() > cut.value()
        {
            return Err(corrupt_storage());
        }
        match &root.command {
            Command::Explanation(command) => {
                if root.target != account
                    || root.subject != count.id
                    || root.previous != explanation_head
                    || command.facts.count_reconciliation != count.id
                    || command.facts.expected_prior != account_head
                    || root.report != Some(command.facts.report)
                    || root.observation.is_some()
                {
                    return Err(corrupt_storage());
                }
                let row=sqlx::query("SELECT substr(count_operation_id,1,37) AS count_operation,substr(reconciliation_id,1,17) AS reconciliation,packs,substr(projection_hash,1,33) AS projection_hash FROM hub_correction_count_explanations WHERE operation_id=?")
                    .bind(id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
                let item = CountLossExplanation {
                    operation: own,
                    stock_revision: root.op.revision,
                    packs: command.facts.packs,
                };
                let seal: Vec<u8> = row
                    .try_get("projection_hash")
                    .map_err(|_| corrupt_storage())?;
                if row
                    .try_get::<String, _>("count_operation")
                    .map_err(|_| corrupt_storage())?
                    != account.as_str()
                    || stock::read::record_id(&row, "reconciliation")? != count.id
                    || stock::read::whole_packs(
                        row.try_get("packs").map_err(|_| corrupt_storage())?,
                    )? != item.packs
                    || projection_seal(hub, &root, count, &item)?.as_slice() != seal.as_slice()
                {
                    return Err(corrupt_storage());
                }
                history.explanations.push(item);
                targets.insert(own, (item.packs, None));
                explanation_head = Some(own);
            }
            Command::Classification(command) => {
                let target = crate::hub::actuals::common::operation_id(&root.target)
                    .map_err(|_| corrupt_storage())?;
                let (before, previous) =
                    targets.get(&target).copied().ok_or_else(corrupt_storage)?;
                if root.subject != target
                    || root.previous != previous
                    || command.expected_prior != previous
                    || command.expected_account_prior != account_head
                    || root.observation != Some(command.observation_operation)
                {
                    return Err(corrupt_storage());
                }
                let observed =
                    crate::hub::actuals::common::operation(command.observation_operation)
                        .map_err(|_| corrupt_storage())?;
                let observed_op = stock::read::operation_on(tx, hub, &observed)
                    .await?
                    .ok_or_else(corrupt_storage)?;
                if observed_op.revision.value() >= root.op.revision.value()
                    || observed_op.recorded_at > root.op.recorded_at
                {
                    return Err(corrupt_storage());
                }
                let observed = super::original_on(tx, hub, observed_op).await?.original;
                let CorrectionObservedFacts::CountClassification {
                    explanation_operation,
                    corrected_packs,
                    report,
                    ..
                } = observed.observation.facts
                else {
                    return Err(corrupt_storage());
                };
                if explanation_operation != target
                    || report != root.report
                    || before == corrected_packs
                {
                    return Err(corrupt_storage());
                }
                let row=sqlx::query("SELECT substr(target_operation_id,1,37) AS target,substr(group_id,1,17) AS group_id,before_packs,after_packs,delta_kind,delta_packs,substr(projection_hash,1,33) AS projection_hash FROM hub_correction_quantity_changes WHERE operation_id=?")
                    .bind(id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
                let item = CountExplanationAmendment {
                    operation: own,
                    stock_revision: root.op.revision,
                    explanation: target,
                    before,
                    after: corrected_packs,
                };
                let seal: Vec<u8> = row
                    .try_get("projection_hash")
                    .map_err(|_| corrupt_storage())?;
                if row
                    .try_get::<String, _>("target")
                    .map_err(|_| corrupt_storage())?
                    != root.target.as_str()
                    || stock::read::record_id(&row, "group_id")? != count.before.id
                    || stock::read::whole_packs(
                        row.try_get("before_packs").map_err(|_| corrupt_storage())?,
                    )? != before
                    || stock::read::whole_packs(
                        row.try_get("after_packs").map_err(|_| corrupt_storage())?,
                    )? != corrected_packs
                    || row
                        .try_get::<String, _>("delta_kind")
                        .map_err(|_| corrupt_storage())?
                        != "classification"
                    || row
                        .try_get::<i64, _>("delta_packs")
                        .map_err(|_| corrupt_storage())?
                        != i64::from(before.value().abs_diff(corrected_packs.value()))
                    || projection_seal(hub, &root, count, &item)?.as_slice() != seal.as_slice()
                {
                    return Err(corrupt_storage());
                }
                history.amendments.push(item);
                targets.insert(target, (corrected_packs, Some(own)));
            }
        }
        account_head = Some(own);
    }
    // Check all interleaved transitions, including retained zero quantities,
    // without claiming that current linked-return floors have been inspected.
    CountLossQuantityAccount::restore_corrected(
        history.original,
        history.explanations.clone(),
        history.amendments.clone(),
        Vec::new(),
    )
    .map_err(|_| corrupt_storage())?;
    Ok(history)
}
