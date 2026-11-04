//! Captured native review cuts; restoration validates immutable origins and seals,
//! never current balances or a saved execution grant.
use super::{common::*, reports};
use crate::hub::actuals::proofs::OriginalReader;
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{handling, reference, stock, submission},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    SafeError,
    hub::{
        HubId, HubRevision,
        actuals::{
            ActualKnownDeparture, ActualNewDepartureCommand, ActualNewDepartureContext,
            ActualPhysicalActivity, ActualPhysicalBirth, ActualReportRecord, ActualReviewContext,
            HistoricalActualSource,
        },
        movement::{PhysicalIdentification, PhysicalStockGroup, WorkAllocation, WorkProgress},
        portions::{PhysicalLineage, PhysicalLineageNode},
        reference::{HubLabel, ReferenceCatalog, SourceProvenance},
        stock::QuantityWork,
        treatment::TreatmentState,
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::{Row, sqlite::SqliteRow};
use stock::read::{optional_id, record_id, revision_field, whole_packs};

pub(in crate::hub) struct Context {
    pub state: stock::read::StockState,
    pub catalog: ReferenceCatalog,
    pub report: ActualReportRecord,
    pub group: PhysicalStockGroup,
    pub lineage: PhysicalLineage,
    pub births: Vec<ActualPhysicalBirth>,
    pub treatment: TreatmentState,
    pub source: HistoricalActualSource,
    pub activity: Vec<ActualPhysicalActivity>,
    pub known: Vec<ActualKnownDeparture>,
    pub uncertain_submissions: bool,
}
impl Context {
    pub fn domain(&self) -> ActualNewDepartureContext<'_> {
        ActualNewDepartureContext {
            review: ActualReviewContext {
                hub: self.report.hub,
                stock_revision: self.state.revision,
                catalog: &self.catalog,
                original: &self.report,
                group: &self.group,
                lineage: &self.lineage,
                treatment: &self.treatment,
                uncertain_submissions: self.uncertain_submissions,
            },
            historical_source: harvestcircle_domain::hub::actuals::ActualSourceContext {
                source: &self.source,
                lineage: &self.lineage,
                balancing_group: &self.group,
                subsequent_activity: &self.activity,
            },
            known_departures: &self.known,
            prior_review: None,
        }
    }
}

pub(in crate::hub) async fn current_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    command: &ActualNewDepartureCommand,
    except: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    Box::pin(crate::hub::corrections::require_recipient_actual_adapter_on(tx, hub, state.revision))
        .await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if state.revision != command.facts.expected_stock_revision
        || catalog.revision() != command.facts.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    let report = reports::by_id_on(tx, hub, command.facts.report)
        .await?
        .original;
    let prior:Option<String>=sqlx::query_scalar("SELECT substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=? LIMIT 1")
        .bind(report.id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if prior.is_some() {
        return Err(stock::conflict());
    }
    let group = handling::read::group_on(tx, hub, command.facts.group, &catalog).await?;
    let loaded = handling::lineage::load_on(tx, hub, group.trace, &catalog, state.revision).await?;
    let births = handling::lineage::actual_births_on(tx, hub, &loaded).await?;
    let source = handling::actuals::source_on(
        tx,
        hub,
        &loaded.graph,
        &loaded.trace,
        &operation(command.source_operation)?,
        command.source_group,
    )
    .await?;
    let activity = handling::actuals::activity_on(tx, hub, &loaded.graph, &source).await?;
    let known =
        handling::actuals::known_departures_on(tx, hub, group.trace, state.revision).await?;
    let treatment = handling::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    // Only the fresh executor may exclude its own already-admitted pending row.
    // Read/capture paths include every pending outcome; recorded report is no exception.
    let recorded_operation = operation(report.operation)?;
    let uncertain = submission::uncertainty_on(tx, except.unwrap_or(&recorded_operation))
        .await?
        .uncertain_intent;
    Ok(Context {
        state,
        catalog,
        report,
        group,
        lineage: loaded.graph,
        births,
        treatment,
        source,
        activity,
        known,
        uncertain_submissions: uncertain,
    })
}

pub(in crate::hub) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    context: &Context,
) -> Result<(), SafeError> {
    for node in context.lineage.nodes() {
        sqlx::query("INSERT INTO hub_actual_review_lineage (operation_id,group_id,parent_group_id,created_revision,packs) VALUES (?,?,?,?,?)")
            .bind(operation.as_str()).bind(node.id.as_bytes().as_slice()).bind(node.parent.as_ref().map(|id|id.as_bytes().as_slice()))
            .bind(integer(node.created_revision)?).bind(i64::from(node.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    for allocation in &context.group.work {
        let work = &allocation.requirement;
        sqlx::query("INSERT INTO hub_actual_review_work_scope (operation_id,work_id,before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining) VALUES (?,?,?,?,?,?,?)")
            .bind(operation.as_str()).bind(work.id.as_bytes().as_slice()).bind(progress(allocation.progress)).bind(integer(work.revision)?)
            .bind(i64::from(work.quantity.completed().value())).bind(i64::from(work.quantity.no_longer_required().value())).bind(i64::from(work.quantity.remaining().value()))
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}

pub(in crate::hub) async fn original_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
    row: &SqliteRow,
    command: &ActualNewDepartureCommand,
) -> Result<Context, SafeError> {
    let cut = HubRevision::from_value(
        op.revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let catalog = reference::load_catalog_through_on(tx, hub, op.catalog_revision).await?;
    let report = reports::by_id_on(tx, hub, record_id(row, "report_id")?)
        .await?
        .original;
    let trace = record_id(row, "trace_id")?;
    let population = handling::read::trace_on(tx, hub, trace).await?;
    let expected = usize::try_from(
        row.try_get::<i64, _>("lineage_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if !(1..=harvestcircle_domain::hub::portions::HUB_LINEAGE_GROUPS).contains(&expected) {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query("SELECT substr(group_id,1,17) AS group_id,substr(parent_group_id,1,17) AS parent_group_id,created_revision,packs FROM hub_actual_review_lineage WHERE operation_id=? ORDER BY group_id LIMIT 257")
        .bind(op.id.as_str()).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != expected {
        return Err(corrupt_storage());
    }
    let mut nodes = Vec::with_capacity(rows.len());
    for node in &rows {
        let id = record_id(node, "group_id")?;
        nodes.push(PhysicalLineageNode {
            recovery: crate::hub::corrections::birth::recovery_at_on(tx, hub, id, cut).await?,
            id,
            parent: optional_id(node, "parent_group_id")?,
            trace,
            variant: population.variant.id,
            created_revision: revision_field(node, "created_revision")?,
            packs: whole_packs(node.try_get("packs").map_err(|_| corrupt_storage())?)?,
        });
    }
    let lineage = PhysicalLineage::new(hub, trace, population.variant.id, cut, nodes)
        .map_err(|_| corrupt_storage())?;
    let births = handling::lineage::historical_births_on(tx, hub, &lineage).await?;
    let group = group_on(tx, hub, op, row, &lineage).await?;
    let source = handling::actuals::source_on(
        tx,
        hub,
        &lineage,
        &population,
        &operation(command.source_operation).map_err(|_| corrupt_storage())?,
        command.source_group,
    )
    .await?;
    let activity = handling::actuals::activity_on(tx, hub, &lineage, &source).await?;
    let known = handling::actuals::known_departures_on(tx, hub, trace, cut).await?;
    let treatment = handling::treatment::read::historical_state_on(tx, hub, &lineage).await?;
    let event_count:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_events e JOIN hub_stock_operations o ON o.operation_id=e.operation_id WHERE o.hub_id=? AND o.stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(integer(cut)?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    let event_count = u32::try_from(event_count).map_err(|_| corrupt_storage())?;
    if event_count as usize > harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(corrupt_storage());
    }
    Ok(Context {
        state: stock::read::StockState {
            revision: cut,
            event_count,
        },
        catalog,
        report,
        group,
        lineage,
        births,
        treatment,
        source,
        activity,
        known,
        uncertain_submissions: false,
    })
}

async fn group_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
    row: &SqliteRow,
    graph: &PhysicalLineage,
) -> Result<PhysicalStockGroup, SafeError> {
    let id = record_id(row, "group_id")?;
    let node = graph.node(id).ok_or_else(corrupt_storage)?;
    let identity=sqlx::query("SELECT substr(parent_group_id,1,17) AS parent_id,substr(CAST(physical_label AS BLOB),1,121) AS label,substr(CAST(observation AS BLOB),1,513) AS observation FROM hub_group_identification WHERE group_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|corrupt_storage())?;
    let parent = optional_id(&identity, "parent_id")?;
    if parent != node.parent {
        return Err(corrupt_storage());
    }
    let label: Option<Vec<u8>> = identity.try_get("label").map_err(|_| corrupt_storage())?;
    let observation: Option<Vec<u8>> = identity
        .try_get("observation")
        .map_err(|_| corrupt_storage())?;
    let identification = match (label, observation) {
        (None, None) => None,
        (Some(label), Some(observation)) => Some(PhysicalIdentification {
            label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(
                String::from_utf8(observation).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
        }),
        _ => return Err(corrupt_storage()),
    };
    let scope_count = usize::try_from(
        row.try_get::<i64, _>("scope_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if scope_count > harvestcircle_domain::hub::movement::HUB_GROUP_REQUIREMENTS {
        return Err(corrupt_storage());
    }
    let scopes=sqlx::query("SELECT substr(work_id,1,17) AS work_id,substr(before_progress,1,33) AS before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining FROM hub_actual_review_work_scope WHERE operation_id=? ORDER BY work_id LIMIT 65")
        .bind(op.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if scopes.len() != scope_count {
        return Err(corrupt_storage());
    }
    let mut work = Vec::with_capacity(scope_count);
    for scope in scopes {
        let definition =
            handling::read::definition_on(tx, hub, record_id(&scope, "work_id")?).await?;
        if definition.trace != graph.trace() || definition.variant != graph.variant() {
            return Err(corrupt_storage());
        }
        let quantity = QuantityWork::restore(
            definition.required,
            whole_packs(
                scope
                    .try_get("before_completed")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                scope
                    .try_get("before_no_longer_required")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                scope
                    .try_get("before_remaining")
                    .map_err(|_| corrupt_storage())?,
            )?,
        )
        .map_err(|_| corrupt_storage())?;
        let requirement =
            definition.at(quantity, revision_field(&scope, "before_work_revision")?)?;
        let progress = match scope
            .try_get::<String, _>("before_progress")
            .map_err(|_| corrupt_storage())?
            .as_str()
        {
            "pending" => WorkProgress::Pending,
            "completed" => WorkProgress::Completed,
            "no_longer_required" => WorkProgress::NoLongerRequired,
            _ => return Err(corrupt_storage()),
        };
        work.push(WorkAllocation {
            requirement,
            progress,
        });
    }
    let floor = row
        .try_get::<Option<i64>, _>("before_causal_floor_unix_s")
        .map_err(|_| corrupt_storage())?
        .map(stock::read::timestamp)
        .transpose()?;
    if handling::read::causal_floor_through_on(tx, id, graph.revision()).await? != floor {
        return Err(corrupt_storage());
    }
    let group = PhysicalStockGroup {
        id,
        trace: graph.trace(),
        variant: graph.variant(),
        parent,
        identification,
        location: record_id(row, "current_location_id")?,
        packs: whole_packs(row.try_get("before_packs").map_err(|_| corrupt_storage())?)?,
        revision: revision_field(row, "before_group_revision")?,
        causal_floor: floor,
        work,
    };
    if group.packs != node.packs {
        return Err(corrupt_storage());
    }
    group.validate().map_err(|_| corrupt_storage())?;
    Ok(group)
}

pub(super) const fn progress(value: WorkProgress) -> &'static str {
    match value {
        WorkProgress::Pending => "pending",
        WorkProgress::Completed => "completed",
        WorkProgress::NoLongerRequired => "no_longer_required",
    }
}
