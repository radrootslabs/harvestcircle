//! Synthetic schema-7-shaped originals in actual prefix-schema hosts.
//! This tests upgrade data compatibility, not a retained historical executable.
//! Never call current-schema readers or add missing future tables to a prefix.
use super::*;
use crate::hub::stock;
use crate::hub::stock::tests::{now, operation};
use harvestcircle_domain::hub::movement::{
    DiscrepancyAction, DiscrepancyCommand, MovementCommand, WorkAllocation, WorkProgress,
};
use harvestcircle_domain::hub::reference::{ReferenceKind, SourceProvenance};

pub(crate) async fn seed_schema7_handling(
    host: &radroots_service_sqlite::ServiceSqliteHost,
    hub: harvestcircle_domain::hub::HubOwnership,
    receipt: &crate::HubStockReceipt,
) -> Vec<HubHandlingReceipt> {
    let intake = receipt.original.clone();
    host.transaction(|tx| Box::pin(async move { seed_handling_on(tx, hub, intake).await }))
        .await
        .unwrap()
}

async fn seed_handling_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubOwnership,
    intake: ReceiptPlan,
) -> Result<Vec<HubHandlingReceipt>, SafeError> {
    let catalog = super::super::reference::load_catalog_on(tx, hub.id()).await?;
    let source = PhysicalStockGroup {
        id: intake.position.group,
        trace: intake.trace.id,
        variant: intake.position.variant,
        parent: None,
        identification: None,
        location: intake.position.location,
        packs: intake.position.packs,
        revision: HubRevision::INITIAL,
        causal_floor: None,
        work: vec![WorkAllocation {
            requirement: WorkRequirement {
                id: intake.trace.id,
                trace: intake.trace.id,
                variant: intake.position.variant,
                target: intake.trace.planned_location.clone(),
                quantity: intake.put_away,
                revision: HubRevision::INITIAL,
            },
            progress: WorkProgress::Pending,
        }],
    };
    let movement = MovementCommand {
        expected_stock_revision: intake.stock_revision,
        expected_catalog_revision: catalog.revision(),
        group: source.id,
        expected_group_revision: source.revision,
        to: intake.trace.planned_location.as_ref().unwrap().id,
        packs: WholePacks::from_value(8).unwrap(),
        separated_portion: Some(PhysicalIdentification {
            label: HubLabel::new("Historical blue eight").unwrap(),
            observation: SourceProvenance::new("Eight original tagged crates").unwrap(),
        }),
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Actual historical move to A3").unwrap(),
    };
    let op = operation(300);
    let ids = handling_ids(&op)?;
    let plan = movement
        .plan(&catalog, intake.stock_revision, &source, ids.group, now())
        .unwrap();
    write::insert_operation_on(
        tx,
        hub.id(),
        &stock::read::OperationRecord {
            id: op.clone(),
            kind: "move".into(),
            actor: hub.owner(),
            hash: movement.fingerprint(hub.id(), hub.owner(), ids.group),
            catalog_revision: catalog.revision(),
            revision: plan.stock_revision,
            result: plan.moved.id,
            recorded_at: now(),
        },
    )
    .await?;
    write::write_groups_on(
        tx,
        &catalog,
        &op,
        &source,
        write::GroupChange {
            remaining: plan.remaining.as_ref(),
            selected: &plan.moved,
            destination: &plan.destination,
        },
    )
    .await?;
    let mut changes = Vec::new();
    for after in &plan.work_updates {
        let before = &source
            .work
            .iter()
            .find(|scope| scope.requirement.id == after.id)
            .unwrap()
            .requirement;
        write::update_work_on(tx, before, after).await?;
        write::work_change_on(tx, &op, Some(before), after).await?;
        changes.push(HubWorkChange {
            before: Some(before.clone()),
            after: after.clone(),
        });
    }
    sqlx::query("INSERT INTO hub_movement_details (operation_id,selected_group_id,expected_group_revision,source_before_packs,moved_group_id,work_change_count,observation) VALUES (?,?,1,20,?,1,?)")
        .bind(op.as_str()).bind(source.id.as_bytes().as_slice()).bind(plan.moved.id.as_bytes().as_slice()).bind(movement.observation.as_str())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let from = catalog
        .require_kind(source.location, ReferenceKind::Location)
        .unwrap()
        .clone();
    write::event_on(
        tx,
        &op,
        write::EventEffect {
            kind: "move",
            prior_events: 1,
            trace: source.trace,
            group: plan.moved.id,
            packs: movement.packs,
            from: Some(&from),
            to: Some(&plan.destination),
            occurred: movement.occurred,
        },
    )
    .await?;
    write::advance_on(
        tx,
        hub.id(),
        &stock::read::StockState {
            revision: intake.stock_revision,
            event_count: 1,
        },
        plan.stock_revision,
    )
    .await?;
    let moved = HubHandlingReceipt {
        operation: op,
        kind: HubHandlingKind::Move,
        revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: source.trace,
        actor: hub.owner(),
        recorded_at: now(),
        result: HubHandlingResult::Moved(HubMovementResult {
            selected_group: source.id,
            expected_group_revision: source.revision,
            source_before_packs: source.packs,
            from,
            to: plan.destination,
            packs: movement.packs,
            moved_group: plan.moved.id,
            remaining_packs: WholePacks::from_value(12).unwrap(),
            separated_portion: movement.separated_portion,
            occurred: movement.occurred,
            observation: movement.observation,
            work_changes: changes,
        }),
    };

    let op = operation(301);
    let ids = handling_ids(&op)?;
    let observation = SourceProvenance::new("Historical label follow-up remains open").unwrap();
    let command = DiscrepancyCommand {
        expected_stock_revision: moved.revision,
        expected_catalog_revision: catalog.revision(),
        action: DiscrepancyAction::Open {
            trace: source.trace,
            group: Some(source.id),
            observation: observation.clone(),
        },
    };
    let value = Discrepancy::open(ids.discrepancy, source.trace, Some(source.id), observation);
    let revision = HubRevision::from_value(moved.revision.value() + 1).unwrap();
    write::insert_operation_on(
        tx,
        hub.id(),
        &stock::read::OperationRecord {
            id: op.clone(),
            kind: "discrepancy_open".into(),
            actor: hub.owner(),
            hash: command.fingerprint(hub.id(), hub.owner(), ids.discrepancy),
            catalog_revision: catalog.revision(),
            revision,
            result: value.id,
            recorded_at: now(),
        },
    )
    .await?;
    sqlx::query("INSERT INTO hub_discrepancies (discrepancy_id,trace_id,group_id,observation,revision,resolution,created_operation_id,resolved_operation_id) VALUES (?,?,?,?,1,NULL,?,NULL)")
        .bind(value.id.as_bytes().as_slice()).bind(value.trace.as_bytes().as_slice()).bind(source.id.as_bytes().as_slice()).bind(value.observation.as_str()).bind(op.as_str())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    write::event_on(
        tx,
        &op,
        write::EventEffect {
            kind: "discrepancy_open",
            prior_events: 2,
            trace: source.trace,
            group: source.id,
            packs: WholePacks::from_value(0).unwrap(),
            from: None,
            to: None,
            occurred: Occurrence::Unknown,
        },
    )
    .await?;
    write::advance_on(
        tx,
        hub.id(),
        &stock::read::StockState {
            revision: moved.revision,
            event_count: 2,
        },
        revision,
    )
    .await?;
    let issue = HubHandlingReceipt {
        operation: op,
        kind: HubHandlingKind::OpenDiscrepancy,
        revision,
        catalog_revision: catalog.revision(),
        trace: source.trace,
        actor: hub.owner(),
        recorded_at: now(),
        result: HubHandlingResult::DiscrepancyChanged(value),
    };
    Ok(vec![moved, issue])
}

pub(crate) async fn seed_schema7_receipt(
    tx: &mut radroots_service_sqlite::ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &ReceiptPlan,
) -> Result<(), SafeError> {
    seed_receipt_on(tx, operation, plan).await
}
