use super::super::super::{reference, stock};
use super::super::write as journal;
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::reference::ReferenceCatalog;
use harvestcircle_domain::hub::treatment::{
    AcceptanceStatus, TreatmentAction, TreatmentCommand, treatment_result_digest,
};
use stock::read::OperationRecord;

impl Database {
    pub async fn apply_hub_treatment(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: TreatmentCommand,
        now: UnixTimestamp,
    ) -> Result<HubHandlingReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move { apply_on(tx, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: TreatmentCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let id = handling_ids(&operation)?.treatment;
    let hash = command.fingerprint(hub.id(), actor, id);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.actor != actor || original.kind != command.kind() || original.hash != hash {
            return Err(conflict());
        }
        return history::original_on(tx, hub.id(), original).await;
    }
    let stock = stock::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let trace = command_trace_on(tx, &command).await?;
    let loaded =
        super::super::lineage::load_on(tx, hub.id(), trace, &catalog, stock.revision).await?;
    let state = read::state_on(tx, hub.id(), &loaded.graph).await?;
    let plan = command
        .plan(&catalog, &loaded.graph, &state, id, actor, now)
        .map_err(treatment_error)?;
    let reservation = match &command.action {
        TreatmentAction::SetAcceptance { .. } => capacity::Reservation::Ordinary(1),
        TreatmentAction::PlaceHold { .. } => capacity::Reservation::PlaceHold,
        TreatmentAction::ReleaseHold { .. } => capacity::Reservation::ReleaseHold,
    };
    capacity::ensure_on(tx, actor, &operation, reservation).await?;
    let recorded = OperationRecord {
        id: operation.clone(),
        kind: command.kind().to_owned(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision: plan.revision,
        result: id,
        recorded_at: now,
    };
    journal::insert_operation_on(tx, hub.id(), &recorded).await?;
    let (stamp, scope, acceptance, hold, expected) = match &plan.change {
        TreatmentChange::Acceptance(value) => (
            &value.stamp,
            Some(&value.scope),
            Some(match value.status {
                AcceptanceStatus::ReviewRequired => "review",
                AcceptanceStatus::Accepted => "accepted",
                AcceptanceStatus::Rejected => "rejected",
            }),
            None,
            None,
        ),
        TreatmentChange::HoldPlaced(value) => (&value.stamp, Some(&value.scope), None, None, None),
        TreatmentChange::HoldReleased { hold, release } => {
            (release.as_ref(), None, None, Some(*hold), Some(1_i64))
        }
    };
    let digest = treatment_result_digest(
        &hash,
        &plan.change,
        plan.summary,
        plan.affected_packs,
        plan.history_capacity_reached,
    );
    let scope_count = scope.map_or(0, |scope| scope.anchors().len());
    let summary = plan.summary;
    sqlx::query(concat!(
        "INSERT INTO hub_treatment_records (record_id,operation_id,trace_id,kind,acceptance,hold_id,expected_hold_revision,basis_id,basis_version,site_timezone,observation,scope_count,affected_packs,on_hand,accepted,review_required,rejected,restricted,departure_eligible,history_capacity_reached,result_hash) ",
        "VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(id.as_bytes().as_slice()).bind(operation.as_str()).bind(trace.as_bytes().as_slice()).bind(command.kind()).bind(acceptance)
        .bind(hold.as_ref().map(|hold|hold.as_bytes().as_slice())).bind(expected).bind(stamp.basis.id()).bind(i64::from(stamp.basis.version())).bind(stamp.basis.site_timezone())
        .bind(stamp.observation.as_str()).bind(i64::try_from(scope_count).map_err(|_|corrupt_storage())?).bind(i64::from(plan.affected_packs.value()))
        .bind(i64::from(summary.on_hand.value())).bind(i64::from(summary.accepted.value())).bind(i64::from(summary.review_required.value())).bind(i64::from(summary.rejected.value()))
        .bind(i64::from(summary.restricted.value())).bind(i64::from(summary.departure_eligible.value())).bind(i64::from(plan.history_capacity_reached)).bind(digest.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if let Some(scope) = scope {
        for anchor in scope.anchors() {
            sqlx::query("INSERT INTO hub_treatment_scope_anchors VALUES (?,?,?,?)")
                .bind(id.as_bytes().as_slice())
                .bind(anchor.group.as_bytes().as_slice())
                .bind(integer(anchor.created_revision)?)
                .bind(i64::from(anchor.captured_packs.value()))
                .execute(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
        }
    }
    let population = match &plan.change {
        TreatmentChange::Acceptance(value) => &value.scope,
        TreatmentChange::HoldPlaced(value) => &value.scope,
        TreatmentChange::HoldReleased { hold, .. } => {
            &state
                .holds()
                .iter()
                .find(|held| held.stamp.id == *hold)
                .ok_or_else(corrupt_storage)?
                .scope
        }
    };
    journal::event_on(
        tx,
        &operation,
        journal::EventEffect {
            kind: command.kind(),
            prior_events: stock.event_count,
            trace,
            group: population.anchors()[0].group,
            packs: plan.affected_packs,
            from: None,
            to: None,
            occurred: command.occurred,
        },
    )
    .await?;
    journal::advance_on(tx, hub.id(), &stock, plan.revision).await?;
    verify_on(tx, hub.id(), trace, &catalog, &plan).await?;
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::parse(command.kind())?,
        revision: plan.revision,
        catalog_revision: catalog.revision(),
        trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::Treatment(Box::new(HubTreatmentResult {
            change: plan.change,
            summary: plan.summary,
            affected_packs: plan.affected_packs,
            history_capacity_reached: plan.history_capacity_reached,
        })),
    })
}

async fn command_trace_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    command: &TreatmentCommand,
) -> Result<HubRecordId, SafeError> {
    let trace:Option<Vec<u8>>=match &command.action {
        TreatmentAction::SetAcceptance{selection,..}|TreatmentAction::PlaceHold{selection}=>sqlx::query_scalar("SELECT substr(trace_id,1,17) FROM hub_stock_groups WHERE group_id=? LIMIT 1")
            .bind(selection.groups()[0].as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?,
        TreatmentAction::ReleaseHold{hold,..}=>sqlx::query_scalar("SELECT substr(trace_id,1,17) FROM hub_treatment_records WHERE record_id=? AND kind='hold_place' LIMIT 1")
            .bind(hold.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?,
    };
    HubRecordId::from_bytes(
        trace
            .ok_or_else(invalid_input)?
            .try_into()
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())
}

async fn verify_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    catalog: &ReferenceCatalog,
    plan: &harvestcircle_domain::hub::treatment::TreatmentPlan,
) -> Result<(), SafeError> {
    let graph = super::super::lineage::load_on(tx, hub, trace, catalog, plan.revision)
        .await?
        .graph;
    let state = read::state_on(tx, hub, &graph).await?;
    let projected = state
        .project(&graph, catalog.handling_basis())
        .map_err(|_| corrupt_storage())?;
    if projected.summary != plan.summary
        || projected.history_capacity_reached != plan.history_capacity_reached
    {
        return Err(corrupt_storage());
    }
    Ok(())
}
