//! Owned canonical facts at one count comparison cut. This data is neither
//! admission nor an execution grant; count persistence remains fenced.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{
        ActualPhysicalActivity, ActualSourceContext, CountedShortageSourceContext,
        HistoricalActualSource,
    },
    corrections::{CountExplanationCommand, CountExplanationContext, CountLossQuantityAccount},
    counts::{CountEffect, CountPostPlan},
    portions::PhysicalLineage,
    reference::ReferenceCatalog,
    treatment::TreatmentState,
};

pub(in crate::hub) struct Facts {
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
}
// Immutable facts do not assert the absence of pending work. Current entry
// reads that state separately; a future frozen reader must authenticate its
// original captured uncertainty before constructing a planning context.
pub(super) struct Context {
    pub facts: Facts,
    pub uncertain_submissions: bool,
}
impl Context {
    pub fn domain(&self) -> CountExplanationContext<'_> {
        CountExplanationContext {
            hub: self.facts.hub,
            stock_revision: self.facts.stock_revision,
            latest_recorded_at: self.facts.latest_recorded_at,
            catalog: &self.facts.catalog,
            account: &self.facts.account,
            report: &self.facts.report,
            prior_report_review: self.facts.prior_report_review,
            source: CountedShortageSourceContext {
                source: ActualSourceContext {
                    source: &self.facts.source,
                    lineage: &self.facts.graph,
                    balancing_group: &self.facts.count.before,
                    subsequent_activity: &self.facts.activity,
                },
                count: &self.facts.count,
            },
            treatment_at_count: &self.facts.treatment,
            known_departures: self
                .facts
                .known
                .iter()
                .map(|event| event.as_domain())
                .collect(),
            uncertain_submissions: self.uncertain_submissions,
        }
    }
}

pub(super) async fn current_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    query: &CountExplanationCommand,
) -> Result<Context, SafeError> {
    current_with_pending_on(tx, hub, query, None).await
}

// Only a fresh admitted writer may exclude its exact own pending operation.
pub(super) async fn current_with_pending_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    query: &CountExplanationCommand,
    except: Option<&DurableRequestId>,
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
    let facts = Box::pin(at_on(tx, hub, query, Positions::Current)).await?;
    let uncertain_submissions = match except {
        Some(own) => crate::hub::submission::uncertainty_on(tx, own).await?,
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
    }
    .uncertain_intent;
    Ok(Context {
        facts,
        uncertain_submissions,
    })
}

/// Canonical immutable facts at an explicit paired cut. This loader does not
/// inspect current positions or assert that historical pending work was absent.
/// The native original caller must authenticate its complete captured material.
pub(in crate::hub) async fn facts_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    query: &CountExplanationCommand,
) -> Result<Facts, SafeError> {
    Box::pin(at_on(tx, hub, query, Positions::Historical)).await
}

enum Positions {
    Current,
    Historical,
}
async fn at_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    query: &CountExplanationCommand,
    positions: Positions,
) -> Result<Facts, SafeError> {
    // Audit the current journal envelope without replaying current originals
    // or substituting current materialized positions for the requested cut.
    stock::read::journal_state_on(tx, hub).await?;
    let cut = query.facts.expected_stock_revision;
    let cut_sql = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    let catalog_sql = i64::try_from(query.facts.expected_catalog_revision.value())
        .map_err(|_| corrupt_storage())?;
    let (operations, maximum, dependency, latest): (i64,i64,i64,Option<i64>) = sqlx::query_as(concat!(
        "SELECT count(*),COALESCE(max(stock_revision),1),COALESCE(max(catalog_revision),1),max(recorded_at_unix_s) ",
        "FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?"
    )).bind(hub.as_bytes().as_slice()).bind(cut_sql).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if maximum != cut_sql || dependency > catalog_sql {
        return Err(stock::invalid_input());
    }
    if operations.checked_add(1) != Some(cut_sql) {
        return Err(corrupt_storage());
    }
    let catalog =
        reference::load_catalog_through_on(tx, hub, query.facts.expected_catalog_revision).await?;
    let unsupported: i64 = sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.family IN ('count_explain','count_classify') AND (o.operation_id IS NULL OR (o.hub_id=? AND o.stock_revision<=?)))+",
        "(SELECT count(*) FROM hub_correction_count_explanations c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE o.operation_id IS NULL OR (o.hub_id=? AND o.stock_revision<=?))"
    )).bind(hub.as_bytes().as_slice()).bind(cut_sql).bind(hub.as_bytes().as_slice()).bind(cut_sql)
        .fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    let target: Option<String> = sqlx::query_scalar(
        concat!("SELECT substr(c.operation_id,1,37) FROM hub_count_reconciliations c ",
            "JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
            "WHERE c.reconciliation_id=? AND o.hub_id=? AND o.stock_revision<=? AND o.catalog_revision<=?"),
    )
    .bind(query.facts.count_reconciliation.as_bytes().as_slice())
    .bind(hub.as_bytes().as_slice()).bind(cut_sql).bind(catalog_sql)
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
    let CountEffect::Shortage { .. } = count.effect else {
        return Err(stock::invalid_input());
    };
    if matches!(positions, Positions::Current) {
        // Current materialized positions are independently audited before
        // offering a current comparison. A balanced sibling transfer cannot
        // hide behind population-wide conservation or the old count cut.
        let current = Box::pin(handling::lineage::load_on(
            tx,
            hub,
            count.before.trace,
            &catalog,
            cut,
        ))
        .await?;
        let reconstructed_current = Box::pin(handling::lineage::historical_quantities_on(
            tx,
            hub,
            count.before.trace,
            cut,
        ))
        .await?;
        if current.graph != reconstructed_current {
            return Err(corrupt_storage());
        }
    }
    let history = super::count_account::history_on(tx, hub, &count, cut).await?;
    let returns = super::count_returns::for_history_on(tx, hub, &history, cut).await?;
    let account = history.with_returns(returns.totals)?;
    let included: i64 = sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_actual_reports r JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "WHERE r.report_id=? AND o.hub_id=? AND o.stock_revision<=? AND o.catalog_revision<=?"
    )).bind(query.facts.report.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(cut_sql).bind(catalog_sql).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if included != 1 {
        return Err(stock::invalid_input());
    }
    let report = crate::hub::actuals::reports::by_id_on(tx, hub, query.facts.report)
        .await?
        .original;
    let prior_report_review = report_marker_through_on(tx, hub, report.id, cut).await?;
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
        cut,
    ))
    .await?;
    Ok(Facts {
        hub,
        stock_revision: cut,
        catalog,
        latest_recorded_at: UnixTimestamp::from_seconds(latest.ok_or_else(corrupt_storage)?)
            .ok_or_else(corrupt_storage)?,
        count,
        account,
        report,
        prior_report_review,
        graph,
        source,
        activity,
        treatment,
        known,
    })
}

/// Later closure is not prior review. Read only the immutable owner's header
/// before deciding whether it belongs to the requested stock cut; never replan
/// a future correction while reconstructing an earlier fact set.
pub(in crate::hub) async fn report_marker_through_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    report: HubRecordId,
    cut: HubRevision,
) -> Result<Option<HubRecordId>, SafeError> {
    let marker: Option<String> = sqlx::query_scalar(
        "SELECT substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=?",
    )
    .bind(report.as_bytes().as_slice())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let cut_sql = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    // The mutable marker is corroborated against immutable closure owners at
    // this cut. Clearing it must not turn an already reviewed report into new
    // allowance; a later owner's payload is never reconstructed here.
    let owners: Vec<(String,String,Option<String>)> = sqlx::query_as(concat!(
        "SELECT substr(r.operation_id,1,37),'actual_review',o.kind FROM hub_actual_reviews r ",
        "LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id WHERE r.report_id=? ",
        "AND (o.operation_id IS NULL OR o.hub_id<>? OR o.stock_revision<=?) UNION ALL ",
        "SELECT substr(c.operation_id,1,37),CASE c.family WHEN 'additional' THEN 'correction_additional' ",
        "WHEN 'count_explain' THEN 'correction_count_explain' ELSE 'correction_count_classify' END,o.kind ",
        "FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.report_id=? AND c.family IN ('additional','count_explain','count_classify') ",
        "AND (o.operation_id IS NULL OR o.hub_id<>? OR o.stock_revision<=?) LIMIT 3"
    )).bind(report.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(cut_sql)
        .bind(report.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(cut_sql)
        .fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if owners.len() > 1
        || owners
            .iter()
            .any(|(_, expected, actual)| actual.as_deref() != Some(expected.as_str()))
    {
        return Err(corrupt_storage());
    }
    let Some(marker) = marker else {
        return if owners.is_empty() {
            Ok(None)
        } else {
            Err(corrupt_storage())
        };
    };
    let operation = DurableRequestId::parse(marker).map_err(|_| corrupt_storage())?;
    let original = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    if original.revision.value() > cut.value() {
        return if owners.is_empty() {
            Ok(None)
        } else {
            Err(corrupt_storage())
        };
    }
    if owners.len() != 1 || owners[0].0 != operation.as_str() {
        return Err(corrupt_storage());
    }
    crate::hub::actuals::common::operation_id(&operation)
        .map(Some)
        .map_err(|_| corrupt_storage())
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
    reference::validate_basis_on(tx, hub).await?;
    let mut reader = OriginalReader::new(tx);
    let context = Box::pin(current_on(&mut reader, hub, command)).await?;
    super::quantity::ensure_capacity_on(&mut reader, hub, context.facts.count.before.trace).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(
            &context.domain(),
            own,
            actor,
            context.facts.latest_recorded_at,
        )
        .map_err(super::additional_review::error)?;
    material(&context, command, own)
}

/// Exact descriptive material shared by fresh capture and authenticated original
/// reconstruction. The caller supplies its separately established uncertainty.
pub(super) fn material(
    context: &Context,
    command: &CountExplanationCommand,
    own: HubRecordId,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedCountExplanation, SafeError> {
    use harvestcircle_application::hub_submission::HubSubmittedCountExplanation;
    use harvestcircle_domain::hub::actuals::{
        ActualKnownCountInventory, ActualKnownInterpretation, ActualKnownInterpretations,
        ActualReportError,
    };
    // Planning checks current reference kinds as well as the count's immutable
    // labels. Preserve the selected current catalog content separately.
    use harvestcircle_domain::hub::reference::ReferenceValue;
    let variant = context
        .facts
        .catalog
        .get(context.facts.count.before.variant)
        .map_err(|_| corrupt_storage())?;
    let ReferenceValue::Pack { product, .. } = &variant.value else {
        return Err(corrupt_storage());
    };
    let mut references = [command.facts.recipient, variant.id, *product]
        .into_iter()
        .map(|id| {
            context
                .facts
                .catalog
                .get(id)
                .cloned()
                .map_err(|_| corrupt_storage())
        })
        .collect::<Result<Vec<_>, _>>()?;
    references.sort_by_key(|record| record.id);
    references.dedup_by_key(|record| record.id);
    let known = ActualKnownCountInventory::new(ActualKnownInterpretations {
        hub: context.facts.hub,
        stock_revision: context.facts.stock_revision,
        catalog_revision: context.facts.catalog.revision(),
        latest_recorded_at: context.facts.latest_recorded_at,
        records: context
            .facts
            .known
            .iter()
            .map(|known| ActualKnownInterpretation {
                stock_revision: context.facts.stock_revision,
                catalog_revision: context.facts.catalog.revision(),
                latest_recorded_at: context.facts.latest_recorded_at,
                original: known.original.clone(),
                recipient_original: known.recipient_original.clone(),
                recipient_history: known.recipient_history.clone(),
                quantities: known.quantities.clone(),
                count_explanation: known.count_explanation.clone(),
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
        count: context.facts.count.clone(),
        account: context.facts.account.clone(),
        report: context.facts.report.clone(),
        source: context.facts.source.clone(),
        lineage: context.facts.graph.clone(),
        activity: context.facts.activity.clone(),
        treatment: context.facts.treatment.clone(),
        references,
        known,
        prior_report_review: context.facts.prior_report_review,
        latest_recorded_at: context.facts.latest_recorded_at,
        uncertain_submissions: context.uncertain_submissions,
    })
}
