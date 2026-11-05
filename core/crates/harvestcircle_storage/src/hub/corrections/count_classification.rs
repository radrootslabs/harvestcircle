//! Exact observed classification at paired native cuts. Facts carry no current
//! uncertainty assertion; only fresh entry or authenticated capture supplies it.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedCountClassification;
use harvestcircle_domain::hub::{
    actuals::{
        ActualKnownCountExplanation, ActualKnownCountInventory, ActualKnownInterpretation,
        ActualKnownInterpretations, ActualReportRecord,
    },
    corrections::{
        CorrectionObservedFacts, CountClassificationCommand, CountClassificationContext,
    },
    reference::{ReferenceCatalog, ReferenceValue},
};
use std::sync::Arc;

pub(super) struct Facts {
    pub hub: HubId,
    pub stock: HubRevision,
    pub catalog: ReferenceCatalog,
    pub latest: UnixTimestamp,
    pub original: Arc<super::count_record::HubCountExplanationReceipt>,
    pub shared: Arc<ActualKnownCountExplanation>,
    pub known: Vec<super::known::HubCorrectionKnownDeparture>,
    pub selected: usize,
    pub observation: CorrectionObservationRecord,
    pub successor: Option<HubRecordId>,
    pub used_by: Option<HubRecordId>,
    pub report: Option<ActualReportRecord>,
    pub prior_report_review: Option<HubRecordId>,
    pub unresolved_returns: bool,
}
pub(super) struct Context {
    pub facts: Facts,
    pub uncertain_submissions: bool,
}
impl Context {
    pub fn domain(&self) -> CountClassificationContext<'_> {
        let f = &self.facts;
        let selected = &f.known[f.selected];
        CountClassificationContext {
            hub: f.hub,
            stock_revision: f.stock,
            catalog: &f.catalog,
            latest_recorded_at: f.latest,
            original: &f.original.original,
            account: &f.shared.account,
            recipient_original: &selected.recipient_original,
            recipient_history: &selected.recipient_history,
            observation: &f.observation,
            observation_successor: f.successor,
            observation_used_by: f.used_by,
            report: f.report.as_ref(),
            prior_report_review: f.prior_report_review,
            known_departures: f.known.iter().map(|known| known.as_domain()).collect(),
            uncertain_submissions: self.uncertain_submissions,
            unresolved_return_association: f.unresolved_returns,
        }
    }
    pub fn material(&self, id: HubRecordId) -> Result<HubSubmittedCountClassification, SafeError> {
        let f = &self.facts;
        let selected = &f.known[f.selected];
        let variant = f
            .catalog
            .get(f.original.original.known_event.variant.id)
            .map_err(|_| corrupt_storage())?;
        let ReferenceValue::Pack { product, .. } = &variant.value else {
            return Err(corrupt_storage());
        };
        let parties = selected
            .as_domain()
            .associations_through(f.hub, f.stock, f.catalog.revision(), f.latest)
            .map_err(|_| corrupt_storage())?;
        let ids = [
            *product,
            variant.id,
            selected.recipient_original.recipient.id,
        ]
        .into_iter()
        .chain(parties.parties())
        .chain(f.report.as_ref().and_then(|report| report.report.recipient));
        let mut references = ids
            .map(|id| f.catalog.get(id).cloned().map_err(|_| corrupt_storage()))
            .collect::<Result<Vec<_>, _>>()?;
        references.sort_by_key(|record| record.id);
        references.dedup_by_key(|record| record.id);
        let known = ActualKnownCountInventory::new(ActualKnownInterpretations {
            hub: f.hub,
            stock_revision: f.stock,
            catalog_revision: f.catalog.revision(),
            latest_recorded_at: f.latest,
            records: f
                .known
                .iter()
                .map(|known| ActualKnownInterpretation {
                    stock_revision: f.stock,
                    catalog_revision: f.catalog.revision(),
                    latest_recorded_at: f.latest,
                    original: known.original.clone(),
                    recipient_original: known.recipient_original.clone(),
                    recipient_history: known.recipient_history.clone(),
                    quantities: known.quantities.clone(),
                    count_explanation: known.count_explanation.clone(),
                })
                .collect(),
        })
        .map_err(crate::hub::actuals::common::error)?;
        Ok(HubSubmittedCountClassification {
            id,
            observation: f.observation.clone(),
            original: f.original.original.clone(),
            account: f.shared.account.clone(),
            recipient_original: selected.recipient_original.clone(),
            recipient_history: selected.recipient_history.clone(),
            report: f.report.clone(),
            references,
            known,
            prior_report_review: f.prior_report_review,
            observation_successor: f.successor,
            observation_used_by: f.used_by,
            latest_recorded_at: f.latest,
            uncertain_submissions: self.uncertain_submissions,
            unresolved_return_association: f.unresolved_returns,
        })
    }
}

pub(super) async fn facts_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    command: &CountClassificationCommand,
) -> Result<Facts, SafeError> {
    let cut = command.expected_stock_revision;
    let cat = command.expected_catalog_revision;
    let sql_cut = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    stock::read::journal_state_on(tx, hub).await?;
    let catalog = reference::load_catalog_through_on(tx, hub, cat).await?;
    let observed_id = crate::hub::actuals::common::operation(command.observation_operation)
        .map_err(|_| stock::invalid_input())?;
    let observed_op = stock::read::operation_on(tx, hub, &observed_id)
        .await?
        .ok_or_else(stock::invalid_input)?;
    if observed_op.kind != "correction_observe"
        || observed_op.revision.value() > cut.value()
        || observed_op.catalog_revision.value() > cat.value()
    {
        return Err(stock::invalid_input());
    }
    let observation = super::original_on(tx, hub, observed_op).await?.original;
    let CorrectionObservedFacts::CountClassification {
        explanation_operation,
        report,
        ..
    } = &observation.observation.facts
    else {
        return Err(stock::invalid_input());
    };
    let target = crate::hub::actuals::common::operation(*explanation_operation)
        .map_err(|_| corrupt_storage())?;
    let op = stock::read::operation_on(tx, hub, &target)
        .await?
        .ok_or_else(stock::invalid_input)?;
    if op.kind != "correction_count_explain"
        || op.revision.value() >= observation.stock_revision.value()
        || op.revision.value() > cut.value()
        || op.catalog_revision.value() > cat.value()
    {
        return Err(stock::invalid_input());
    }
    let original = Box::pin(tx.count_explanation(hub, op)).await?;
    let trace = original.original.known_event.trace;
    let latest = super::known::interpretation_clock_on(tx, hub, trace, cut, cat).await?;
    let known = Box::pin(super::known::load_with_proofs_on(tx, hub, trace, cut)).await?;
    let selected = known
        .iter()
        .position(|known| known.original.operation == *explanation_operation)
        .ok_or_else(corrupt_storage)?;
    let shared = Arc::clone(
        known[selected]
            .count_explanation
            .as_ref()
            .ok_or_else(corrupt_storage)?,
    );
    if known[selected].original != original.original.known_event
        || shared.count != original.original.count
    {
        return Err(corrupt_storage());
    }
    let unresolved_returns = known[selected].count_return_uncertainty;
    let successors:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(child.operation_id,1,37) FROM hub_correction_observations child ",
        "LEFT JOIN hub_stock_operations o ON o.operation_id=child.operation_id WHERE child.previous_operation_id=? ",
        "AND (o.operation_id IS NULL OR o.hub_id<>? OR o.stock_revision<=?) LIMIT 2"
    )).bind(observed_id.as_str()).bind(hub.as_bytes().as_slice()).bind(sql_cut).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    let users:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(c.operation_id,1,37) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.observation_operation_id=? AND (o.operation_id IS NULL OR o.hub_id<>? OR o.stock_revision<=?) LIMIT 2"
    )).bind(observed_id.as_str()).bind(hub.as_bytes().as_slice()).bind(sql_cut).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if successors.len() > 1 || users.len() > 1 {
        return Err(corrupt_storage());
    }
    let successor = if let Some(id) = successors.first() {
        let id = DurableRequestId::parse(id.clone()).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "correction_observe" {
            return Err(corrupt_storage());
        }
        Some(crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())?)
    } else {
        None
    };
    let used_by = if let Some(id) = users.first() {
        let id = DurableRequestId::parse(id.clone()).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "correction_count_classify" {
            return Err(corrupt_storage());
        }
        Some(crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())?)
    } else {
        None
    };
    let (report, prior_report_review) = if let Some(report) = report {
        let included:i64=sqlx::query_scalar(concat!(
            "SELECT count(*) FROM hub_actual_reports r JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
            "WHERE r.report_id=? AND o.hub_id=? AND o.stock_revision<=? AND o.catalog_revision<=?"
        )).bind(report.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(sql_cut)
            .bind(i64::try_from(cat.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
        if included != 1 {
            return Err(stock::invalid_input());
        }
        let report = crate::hub::actuals::reports::by_id_on(tx, hub, *report)
            .await?
            .original;
        let marker =
            super::count_context::report_marker_through_on(tx, hub, report.id, cut).await?;
        (Some(report), marker)
    } else {
        (None, None)
    };
    Ok(Facts {
        hub,
        stock: cut,
        catalog,
        latest,
        original,
        shared,
        known,
        selected,
        observation,
        successor,
        used_by,
        report,
        prior_report_review,
        unresolved_returns,
    })
}

pub(super) async fn current_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    command: &CountClassificationCommand,
    except: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if state.revision != command.expected_stock_revision
        || catalog.revision() != command.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    let facts = Box::pin(facts_on(tx, hub, command)).await?;
    let current = Box::pin(handling::lineage::load_on(
        tx,
        hub,
        facts.shared.count.before.trace,
        &facts.catalog,
        state.revision,
    ))
    .await?;
    let reconstructed = Box::pin(handling::lineage::historical_quantities_on(
        tx,
        hub,
        facts.shared.count.before.trace,
        state.revision,
    ))
    .await?;
    if current.graph != reconstructed {
        return Err(corrupt_storage());
    }
    let uncertain = match except {
        Some(id) => crate::hub::submission::uncertainty_on(tx, id).await?,
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
    };
    Ok(Context {
        facts,
        uncertain_submissions: uncertain.uncertain_intent,
    })
}
pub(in crate::hub) async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &CountClassificationCommand,
) -> Result<HubSubmittedCountClassification, SafeError> {
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 21 {
        return Err(stock::invalid_input());
    }
    reference::validate_basis_on(tx, hub).await?;
    let mut reader = OriginalReader::new(tx);
    let context = Box::pin(current_on(&mut reader, hub, command, None)).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(&context.domain(), own, actor, context.facts.latest)
        .map_err(super::additional_review::error)?;
    context.material(own)
}
