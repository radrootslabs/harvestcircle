//! Owned canonical facts at one count comparison cut. This data is neither
//! admission nor an execution grant; count persistence remains fenced.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{
        ActualPhysicalActivity, ActualSourceContext, CountedShortageSourceContext,
        HistoricalActualSource,
    },
    corrections::{
        CountExplanationCommand, CountExplanationContext, CountLossQuantityAccount,
        OriginalCountLossQuantity,
    },
    counts::{CountEffect, CountPostPlan},
    portions::PhysicalLineage,
    reference::ReferenceCatalog,
    treatment::TreatmentState,
};

pub(super) struct Context {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: ReferenceCatalog,
    pub latest_recorded_at: UnixTimestamp,
    pub count: CountPostPlan,
    pub account: CountLossQuantityAccount,
    pub report: harvestcircle_domain::hub::actuals::ActualReportRecord,
    pub prior_report_review: Option<HubRecordId>,
    pub graph: PhysicalLineage,
    pub source: HistoricalActualSource,
    pub activity: Vec<ActualPhysicalActivity>,
    pub treatment: TreatmentState,
    pub known: Vec<super::known::HubCorrectionKnownDeparture>,
    pub uncertain_submissions: bool,
}
impl Context {
    pub fn domain(&self) -> CountExplanationContext<'_> {
        CountExplanationContext {
            hub: self.hub,
            stock_revision: self.stock_revision,
            latest_recorded_at: self.latest_recorded_at,
            catalog: &self.catalog,
            account: &self.account,
            report: &self.report,
            prior_report_review: self.prior_report_review,
            source: CountedShortageSourceContext {
                source: ActualSourceContext {
                    source: &self.source,
                    lineage: &self.graph,
                    balancing_group: &self.count.before,
                    subsequent_activity: &self.activity,
                },
                count: &self.count,
            },
            treatment_at_count: &self.treatment,
            known_departures: self.known.iter().map(|event| event.as_domain()).collect(),
            uncertain_submissions: self.uncertain_submissions,
        }
    }
}

pub(super) async fn current_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    query: &CountExplanationCommand,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if state.revision != query.facts.expected_stock_revision
        || catalog.revision() != query.facts.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    // Writers and their original/correction ledgers are not activated
    // yet. An absent adapter must never fabricate an empty history.
    let unsupported: i64 = sqlx::query_scalar(concat!(
                "SELECT (SELECT count(*) FROM hub_corrections WHERE family IN ('count_explain','count_classify'))+",
                "(SELECT count(*) FROM hub_correction_count_explanations)"
            )).fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    let target: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_count_reconciliations WHERE reconciliation_id=?",
    )
    .bind(query.facts.count_reconciliation.as_bytes().as_slice())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let target = DurableRequestId::parse(target.ok_or_else(stock::invalid_input)?)
        .map_err(|_| corrupt_storage())?;
    let original = stock::read::operation_on(tx, hub, &target)
        .await?
        .ok_or_else(corrupt_storage)?;
    let count = Box::pin(crate::hub::counts::posting::original_on(tx, hub, original))
        .await?
        .original;
    if count.id != query.facts.count_reconciliation {
        return Err(corrupt_storage());
    }
    let CountEffect::Shortage { packs, .. } = count.effect else {
        return Err(stock::invalid_input());
    };
    // Current materialized positions are independently audited before
    // offering a current comparison. A balanced sibling transfer cannot
    // hide behind population-wide conservation or the old count cut.
    let current = Box::pin(handling::lineage::load_on(
        tx,
        hub,
        count.before.trace,
        &catalog,
        state.revision,
    ))
    .await?;
    let reconstructed_current = Box::pin(handling::lineage::historical_quantities_on(
        tx,
        hub,
        count.before.trace,
        state.revision,
    ))
    .await?;
    if current.graph != reconstructed_current {
        return Err(corrupt_storage());
    }
    let account = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: count.before.trace,
            variant: count.before.variant,
            packs,
        },
        Vec::new(),
    )
    .map_err(|_| corrupt_storage())?;
    let report = crate::hub::actuals::reports::by_id_on(tx, hub, query.facts.report)
        .await?
        .original;
    let prior: Option<String> = sqlx::query_scalar(
        "SELECT substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=?",
    )
    .bind(report.id.as_bytes().as_slice())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let prior_report_review = prior
        .map(|id| {
            DurableRequestId::parse(id)
                .map_err(|_| corrupt_storage())
                .and_then(|id| {
                    crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())
                })
        })
        .transpose()?;
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
        &crate::hub::actuals::common::operation(query.source_operation)?,
        query.source_group,
    ))
    .await?;
    let activity = Box::pin(handling::actuals::activity_on(tx, hub, &graph, &source)).await?;
    let treatment = Box::pin(handling::treatment::read::historical_state_on(
        tx, hub, &graph,
    ))
    .await?;
    let known = Box::pin(super::known::load_with_proofs_on(
        tx,
        hub,
        graph.trace(),
        state.revision,
    ))
    .await?;
    let latest: i64 = sqlx::query_scalar(
        "SELECT max(recorded_at_unix_s) FROM hub_stock_operations WHERE hub_id=?",
    )
    .bind(hub.as_bytes().as_slice())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let uncertain = crate::hub::submission::uncertainty_all_on(tx)
        .await?
        .uncertain_intent;
    Ok(Context {
        hub,
        stock_revision: state.revision,
        catalog,
        latest_recorded_at: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        count,
        account,
        report,
        prior_report_review,
        graph,
        source,
        activity,
        treatment,
        known,
        uncertain_submissions: uncertain,
    })
}

/// Descriptive capture only. No owned mutation route selects count kinds until
/// their canonical originals, fresh writer and closure pipeline are qualified.
pub(in crate::hub) async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &CountExplanationCommand,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedCountExplanation, SafeError> {
    use harvestcircle_application::hub_submission::HubSubmittedCountExplanation;
    use harvestcircle_domain::hub::actuals::{
        ActualKnownCountInventory, ActualKnownInterpretation, ActualKnownInterpretations,
        ActualReportError,
    };
    reference::validate_basis_on(tx, hub).await?;
    let mut reader = OriginalReader::new(tx);
    let context = Box::pin(current_on(&mut reader, hub, command)).await?;
    super::quantity::ensure_capacity_on(&mut reader, hub, context.count.before.trace).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(&context.domain(), own, actor, context.latest_recorded_at)
        .map_err(super::additional_review::error)?;
    // Planning checks current reference kinds as well as the count's immutable
    // labels. Preserve the selected current catalog content separately.
    use harvestcircle_domain::hub::reference::ReferenceValue;
    let variant = context
        .catalog
        .get(context.count.before.variant)
        .map_err(|_| corrupt_storage())?;
    let ReferenceValue::Pack { product, .. } = &variant.value else {
        return Err(corrupt_storage());
    };
    let mut references = [command.facts.recipient, variant.id, *product]
        .into_iter()
        .map(|id| {
            context
                .catalog
                .get(id)
                .cloned()
                .map_err(|_| corrupt_storage())
        })
        .collect::<Result<Vec<_>, _>>()?;
    references.sort_by_key(|record| record.id);
    references.dedup_by_key(|record| record.id);
    let known = ActualKnownCountInventory::new(ActualKnownInterpretations {
        hub,
        stock_revision: context.stock_revision,
        catalog_revision: context.catalog.revision(),
        latest_recorded_at: context.latest_recorded_at,
        records: context
            .known
            .iter()
            .map(|known| ActualKnownInterpretation {
                stock_revision: context.stock_revision,
                catalog_revision: context.catalog.revision(),
                latest_recorded_at: context.latest_recorded_at,
                original: known.original.clone(),
                recipient_original: known.recipient_original.clone(),
                recipient_history: known.recipient_history.clone(),
                quantities: known.quantities.clone(),
                // The explicit unsupported-count fence above still rejects count
                // roots; never infer an empty count account from absent adapters.
                count_explanation: None,
            })
            .collect(),
    })
    .map_err(|error| {
        if error == ActualReportError::Capacity {
            stock::capacity()
        } else {
            corrupt_storage()
        }
    })?;
    Ok(HubSubmittedCountExplanation {
        id: own,
        count: context.count,
        account: context.account,
        report: context.report,
        source: context.source,
        lineage: context.graph,
        activity: context.activity,
        treatment: context.treatment,
        references,
        known,
        prior_report_review: context.prior_report_review,
        latest_recorded_at: context.latest_recorded_at,
        uncertain_submissions: context.uncertain_submissions,
    })
}
