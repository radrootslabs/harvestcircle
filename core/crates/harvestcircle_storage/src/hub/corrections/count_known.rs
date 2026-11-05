//! Immutable positive event projection after full count-original verification.
//! Effective classification/returns are separate account facts; zero never
//! removes this original or turns the counted shortage into a physical debit.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{
        ActualKnownDeparture, ActualKnownDepartureLine, ActualSourceContext,
        CountedShortageSourceContext,
    },
    corrections::CountLossExplanation,
    reference::ReferenceKind,
};

pub(super) async fn original_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<ActualKnownDeparture, SafeError> {
    let root = super::count_account::root_on(tx, hub, op).await?;
    let counted = stock::read::operation_on(tx, hub, &root.account)
        .await?
        .ok_or_else(corrupt_storage)?;
    let count = Box::pin(crate::hub::counts::posting::original_on(tx, hub, counted))
        .await?
        .original;
    from_root_on(tx, hub, &root, &count).await
}

async fn from_root_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    root: &super::count_account::Root,
    count: &harvestcircle_domain::hub::counts::CountPostPlan,
) -> Result<ActualKnownDeparture, SafeError> {
    Box::pin(tx.verify_count_explanation(hub, &root.op)).await?;
    let super::count_payload::Command::Explanation(command) = &root.command else {
        return Err(corrupt_storage());
    };
    if crate::hub::actuals::common::operation(count.operation).map_err(|_| corrupt_storage())?
        != root.account
        || count.id != command.facts.count_reconciliation
    {
        return Err(corrupt_storage());
    }
    let catalog = reference::load_catalog_through_on(tx, hub, root.op.catalog_revision).await?;
    let graph = Box::pin(handling::lineage::historical_quantities_on(
        tx,
        hub,
        count.before.trace,
        count.count.scope.stock_revision,
    ))
    .await?;
    let population = handling::read::trace_on(tx, hub, graph.trace()).await?;
    let source = Box::pin(handling::actuals::source_on(
        tx,
        hub,
        &graph,
        &population,
        &crate::hub::actuals::common::operation(command.source_operation)
            .map_err(|_| corrupt_storage())?,
        command.source_group,
    ))
    .await?;
    let activity = Box::pin(handling::actuals::activity_on(tx, hub, &graph, &source)).await?;
    let resolved = CountedShortageSourceContext {
        source: ActualSourceContext {
            source: &source,
            lineage: &graph,
            balancing_group: &count.before,
            subsequent_activity: &activity,
        },
        count,
    }
    .resolve(
        &crate::hub::actuals::reports::by_id_on(tx, hub, command.facts.report)
            .await?
            .original,
        command.facts.occurred,
        root.op.recorded_at,
    )
    .map_err(|_| corrupt_storage())?;
    let own =
        crate::hub::actuals::common::operation_id(&root.op.id).map_err(|_| corrupt_storage())?;
    let item = CountLossExplanation {
        operation: own,
        stock_revision: root.op.revision,
        packs: command.facts.packs,
    };
    let projection:Vec<u8>=sqlx::query_scalar("SELECT substr(projection_hash,1,33) FROM hub_correction_count_explanations WHERE operation_id=?")
        .bind(root.op.id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if super::count_account::projection_seal(hub, root, count, &item)?.as_slice()
        != projection.as_slice()
    {
        return Err(corrupt_storage());
    }
    let known = ActualKnownDeparture {
        operation: own,
        actor: root.op.actor,
        hub,
        stock_revision: root.op.revision,
        catalog_revision: root.op.catalog_revision,
        trace: count.before.trace,
        variant: catalog
            .require_kind(count.before.variant, ReferenceKind::Pack)
            .map_err(|_| corrupt_storage())?
            .clone(),
        recipient: catalog
            .require_kind(command.facts.recipient, ReferenceKind::Party)
            .map_err(|_| corrupt_storage())?
            .clone(),
        occurred: command.facts.occurred,
        recorded_at: root.op.recorded_at,
        lines: vec![ActualKnownDepartureLine {
            group: count.before.id,
            location: resolved.historical().location.clone(),
            packs: item.packs,
        }],
    };
    known.validate().map_err(|_| corrupt_storage())?;
    Ok(known)
}

/// Complete callers validate all included originals before trace filtering.
/// Selected callers ask for one exact event; its account still contains every
/// explanation and classification, including zero, through the supplied cut.
pub(super) async fn components_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    trace: HubRecordId,
    cut: harvestcircle_domain::hub::HubRevision,
    recipients: &[harvestcircle_domain::hub::corrections::RecipientAttributionCorrection],
    selected: Option<&DurableRequestId>,
) -> Result<Vec<super::known::HubCorrectionKnownDeparture>, SafeError> {
    use harvestcircle_domain::hub::{
        actuals::ActualKnownCountExplanation,
        corrections::{
            CorrectedDepartureFamily, OriginalRecipientAttribution, current_recipient_attribution,
        },
    };
    use std::{collections::BTreeMap, sync::Arc};
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE hub_id=? AND kind='correction_count_explain' ",
        "AND stock_revision<=? AND (? IS NULL OR operation_id=?) ORDER BY stock_revision LIMIT ?"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?)
        .bind(selected.map(DurableRequestId::as_str)).bind(selected.map(DurableRequestId::as_str))
        .bind(i64::try_from(harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY + 1).map_err(|_|corrupt_storage())?)
        .fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY {
        return Err(stock::capacity());
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let (catalog,latest):(i64,i64)=sqlx::query_as("SELECT COALESCE(max(catalog_revision),1),COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?)
        .fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    let catalog = harvestcircle_domain::hub::HubRevision::from_value(
        u64::try_from(catalog).map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let latest =
        harvestcircle_domain::UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?;
    let mut accounts: BTreeMap<String, (Arc<ActualKnownCountExplanation>, bool)> = BTreeMap::new();
    let mut result = Vec::new();
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let root = super::count_account::root_on(tx, hub, op).await?;
        if !accounts.contains_key(root.account.as_str()) {
            let counted = stock::read::operation_on(tx, hub, &root.account)
                .await?
                .ok_or_else(corrupt_storage)?;
            let count = Box::pin(crate::hub::counts::posting::original_on(tx, hub, counted))
                .await?
                .original;
            let history = super::count_account::history_on(tx, hub, &count, cut).await?;
            // Scalar projections are not original decision proofs. Every
            // amendment used by this effective account needs its full original
            // certificate in this same decreasing-cut read phase.
            for explanation in &history.explanations {
                let operation = crate::hub::actuals::common::operation(explanation.operation)
                    .map_err(|_| corrupt_storage())?;
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .ok_or_else(corrupt_storage)?;
                Box::pin(tx.verify_count_explanation(hub, &op)).await?;
            }
            for amendment in &history.amendments {
                let operation = crate::hub::actuals::common::operation(amendment.operation)
                    .map_err(|_| corrupt_storage())?;
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .ok_or_else(corrupt_storage)?;
                Box::pin(tx.verify_count_classification(hub, &op)).await?;
            }
            let returns = super::count_returns::for_history_on(tx, hub, &history, cut).await?;
            let value = Arc::new(ActualKnownCountExplanation {
                count,
                account: history.with_returns(returns.totals)?,
            });
            accounts.insert(
                root.account.as_str().to_owned(),
                (value, returns.unresolved),
            );
        }
        let (account, unresolved) = accounts
            .get(root.account.as_str())
            .ok_or_else(corrupt_storage)?;
        let known = from_root_on(tx, hub, &root, &account.count).await?;
        if known.trace != trace {
            continue;
        }
        let original = OriginalRecipientAttribution {
            hub,
            family: CorrectedDepartureFamily::ExplainedCountLoss,
            operation: known.operation,
            stock_revision: known.stock_revision,
            catalog_revision: known.catalog_revision,
            recorded_at: known.recorded_at,
            actor: known.actor,
            recipient: known.recipient.clone(),
            populations: vec![known.trace],
            issued_versions: Vec::new(),
        };
        let history = recipients
            .iter()
            .filter(|r| r.original.operation == known.operation)
            .cloned()
            .collect::<Vec<_>>();
        current_recipient_attribution(&original, &history).map_err(|_| corrupt_storage())?;
        let value = super::known::HubCorrectionKnownDeparture {
            original: known,
            recipient_original: original,
            recipient_history: history,
            quantities: Vec::new(),
            funding: Vec::new(),
            count_explanation: Some(Arc::clone(account)),
            count_return_uncertainty: *unresolved,
        };
        value
            .as_domain()
            .associations_through(hub, cut, catalog, latest)
            .map_err(|_| corrupt_storage())?;
        result.push(value);
    }
    Ok(result)
}
