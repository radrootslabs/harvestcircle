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
            ActualKnownDeparture, ActualKnownInterpretations, ActualNewDepartureCommand,
            ActualNewDepartureContext, ActualPhysicalActivity, ActualPhysicalBirth,
            ActualReportRecord, ActualReviewContext, HistoricalActualSource,
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
    pub interpretations: Option<ActualKnownInterpretations>,
    pub(super) capture: super::mode::Capture,
}
impl Context {
    pub fn submitted_material(
        &self,
        id: harvestcircle_domain::hub::reference::HubRecordId,
    ) -> Result<harvestcircle_application::hub_submission::HubSubmittedMaterial, SafeError> {
        use harvestcircle_application::hub_submission::{
            HubSubmittedCountAwareActualReview, HubSubmittedMaterial,
        };
        let mut review = self.material(id);
        match self.capture {
            super::mode::Capture::Count21 => {
                review.interpretations = None;
                let inventory = harvestcircle_domain::hub::actuals::ActualKnownCountInventory::new(
                    self.interpretations.clone().ok_or_else(corrupt_storage)?,
                )
                .map_err(error)?;
                Ok(HubSubmittedMaterial::CountAwareActualReview(Box::new(
                    HubSubmittedCountAwareActualReview { review, inventory },
                )))
            }
            _ => Ok(HubSubmittedMaterial::ActualReview(Box::new(review))),
        }
    }
    pub fn material(
        &self,
        id: harvestcircle_domain::hub::reference::HubRecordId,
    ) -> harvestcircle_application::hub_submission::HubSubmittedActualReview {
        harvestcircle_application::hub_submission::HubSubmittedActualReview {
            id,
            report: self.report.clone(),
            group: self.group.clone(),
            lineage: self.lineage.clone(),
            births: self.births.clone(),
            treatment: self.treatment.clone(),
            source: self.source.clone(),
            activity: self.activity.clone(),
            known_departures: self.known.clone(),
            uncertain_submissions: self.uncertain_submissions,
            interpretations: self.interpretations.clone(),
        }
    }

    pub fn plan(
        &self,
        command: &ActualNewDepartureCommand,
        id: harvestcircle_domain::hub::reference::HubRecordId,
        actor: harvestcircle_domain::PublicKey,
        now: harvestcircle_domain::UnixTimestamp,
    ) -> Result<harvestcircle_domain::hub::actuals::ActualNewDeparturePlan, SafeError> {
        match &self.interpretations {
            Some(set) => command.plan_with_interpretations(&self.domain(), set, id, actor, now),
            None => command.plan(&self.domain(), id, actor, now),
        }
        .map_err(error)
    }

    pub fn digest(
        &self,
        input: &[u8; 32],
        command: &ActualNewDepartureCommand,
        findings: &[harvestcircle_domain::hub::actuals::HistoricalHoldScopeAssessment],
        plan: &harvestcircle_domain::hub::actuals::ActualNewDeparturePlan,
    ) -> Result<[u8; 32], SafeError> {
        let base=match &self.interpretations {
            Some(set) => harvestcircle_domain::hub::actuals::actual_departure_result_digest_with_interpretations(input, command, &self.domain(), set, &self.births, findings, plan),
            None => harvestcircle_domain::hub::actuals::actual_departure_result_digest(input, command, &self.domain(), &self.births, findings, plan),
        }.map_err(error)?;
        Ok(self.capture.departure_result(base))
    }

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
    let capture = super::mode::current_capture_on(tx, hub).await?;
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
    let physical_known =
        handling::actuals::known_departures_on(tx, hub, group.trace, state.revision).await?;
    let treatment = handling::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    let interpretations = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(
            crate::hub::corrections::actual_interpretations_at_with_proofs_on(
                &mut reader,
                hub,
                group.trace,
                state.revision,
                catalog.revision(),
            ),
        )
        .await?
    };
    let known = known_baseline(capture, physical_known, Some(&interpretations))?;
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
        interpretations: Some(interpretations),
        capture,
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
    let node = lineage
        .node(record_id(row, "group_id")?)
        .ok_or_else(corrupt_storage)?;
    let group = group_on(
        tx,
        hub,
        op,
        row,
        &GroupCut {
            trace: lineage.trace(),
            variant: lineage.variant(),
            stock: lineage.revision(),
            parent: node.parent,
            packs: node.packs,
        },
    )
    .await?;
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
    let physical_known = handling::actuals::known_departures_on(tx, hub, trace, cut).await?;
    let treatment = handling::treatment::read::historical_state_on(tx, hub, &lineage).await?;
    let event_count:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_events e JOIN hub_stock_operations o ON o.operation_id=e.operation_id WHERE o.hub_id=? AND o.stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(integer(cut)?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    let event_count = u32::try_from(event_count).map_err(|_| corrupt_storage())?;
    if event_count as usize > harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(corrupt_storage());
    }
    let mode = super::mode::original_on(tx, hub, op).await?;
    let interpretations = if mode.qualified() {
        Some(
            Box::pin(
                crate::hub::corrections::actual_interpretations_at_with_proofs_on(
                    tx,
                    hub,
                    trace,
                    cut,
                    op.catalog_revision,
                ),
            )
            .await?,
        )
    } else {
        None
    };
    let known = known_baseline(
        mode.capture_kind(),
        physical_known,
        interpretations.as_ref(),
    )?;
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
        interpretations,
        capture: mode.capture_kind(),
    })
}

// Count explanations are positive original events even when their effective
// quantity is now zero. The native mode selects this complete baseline; the
// preserved physical20/legacy baseline retains its original ordering and bytes.
fn known_baseline(
    capture: super::mode::Capture,
    physical: Vec<ActualKnownDeparture>,
    interpretations: Option<&ActualKnownInterpretations>,
) -> Result<Vec<ActualKnownDeparture>, SafeError> {
    if capture != super::mode::Capture::Count21 {
        return Ok(physical);
    }
    let set = interpretations.ok_or_else(corrupt_storage)?;
    let complete = set
        .records
        .iter()
        .map(|entry| entry.original.clone())
        .collect::<Vec<_>>();
    let physical_in_complete = set
        .records
        .iter()
        .filter(|entry| entry.count_explanation.is_none())
        .map(|entry| entry.original.clone())
        .collect::<Vec<_>>();
    if physical_in_complete != physical {
        return Err(corrupt_storage());
    }
    Ok(complete)
}

struct GroupCut {
    trace: harvestcircle_domain::hub::reference::HubRecordId,
    variant: harvestcircle_domain::hub::reference::HubRecordId,
    stock: HubRevision,
    parent: Option<harvestcircle_domain::hub::reference::HubRecordId>,
    packs: harvestcircle_domain::hub::reference::WholePacks,
}

/// Called only by OriginalReader after its complete original certificate check.
/// Reconstructs every original work allocation without replanning its dependencies.
pub(super) async fn verified_before_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
) -> Result<PhysicalStockGroup, SafeError> {
    let row = sqlx::query(concat!(
        "SELECT substr(r.group_id,1,17) AS group_id,substr(r.trace_id,1,17) AS trace_id,substr(r.variant_id,1,17) AS variant_id,",
        "substr(r.current_location_id,1,17) AS current_location_id,r.before_packs,r.before_group_revision,r.before_causal_floor_unix_s,r.scope_count,",
        "substr(l.parent_group_id,1,17) AS frozen_parent,l.packs AS frozen_packs ",
        "FROM hub_actual_reviews r JOIN hub_actual_review_lineage l ON l.operation_id=r.operation_id AND l.group_id=r.group_id ",
        "WHERE r.operation_id=? AND r.review_kind='departure' LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let cut = GroupCut {
        trace: record_id(&row, "trace_id")?,
        variant: record_id(&row, "variant_id")?,
        stock: HubRevision::from_value(
            op.revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt_storage)?,
        )
        .map_err(|_| corrupt_storage())?,
        parent: optional_id(&row, "frozen_parent")?,
        packs: whole_packs(row.try_get("frozen_packs").map_err(|_| corrupt_storage())?)?,
    };
    group_on(tx, hub, op, &row, &cut).await
}

async fn group_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
    row: &SqliteRow,
    cut: &GroupCut,
) -> Result<PhysicalStockGroup, SafeError> {
    let id = record_id(row, "group_id")?;
    let identity=sqlx::query("SELECT substr(parent_group_id,1,17) AS parent_id,substr(CAST(physical_label AS BLOB),1,121) AS label,substr(CAST(observation AS BLOB),1,513) AS observation FROM hub_group_identification WHERE group_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|corrupt_storage())?;
    let parent = optional_id(&identity, "parent_id")?;
    if parent != cut.parent {
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
        if definition.trace != cut.trace || definition.variant != cut.variant {
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
    if handling::read::causal_floor_through_on(tx, id, cut.stock).await? != floor {
        return Err(corrupt_storage());
    }
    let group = PhysicalStockGroup {
        id,
        trace: cut.trace,
        variant: cut.variant,
        parent,
        identification,
        location: record_id(row, "current_location_id")?,
        packs: whole_packs(row.try_get("before_packs").map_err(|_| corrupt_storage())?)?,
        revision: revision_field(row, "before_group_revision")?,
        causal_floor: floor,
        work,
    };
    if group.packs != cut.packs {
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
