use super::*;
use harvestcircle_domain::hub::{
    disposition::{DispositionCommand, DispositionKind},
    movement::{HandlingCommand, MovementCommand, NewWorkCommand},
    treatment::{AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection},
};

#[tokio::test]
async fn additional_delta_retires_only_pending_work_and_later_new_work_movement_depletion_preserve_original()
 {
    for completed in [false, true] {
        let (fixture, db, command, scope, departure) = normal_world(false, completed).await;
        let before = db
            .hub_group(owner(), command.balancing_group)
            .await
            .unwrap()
            .group;
        let old_work = before.work[0].requirement.clone();
        let crate::HubMutationResult::AdditionalCorrected(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::AdditionalQuantity(Box::new(command.clone())),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("additional")
        };
        let current = db
            .hub_group(owner(), command.balancing_group)
            .await
            .unwrap();
        let work = &current.group.work[0].requirement;
        assert_eq!(work.quantity.required(), old_work.quantity.required());
        assert_eq!(work.quantity.completed(), old_work.quantity.completed());
        assert_eq!(
            work.quantity.remaining().value(),
            if completed { 0 } else { 12 }
        );
        assert_eq!(
            work.quantity.no_longer_required().value(),
            if completed { 6 } else { 8 }
        );
        assert_eq!(
            receipt.original.reduction.work_changes.len(),
            usize::from(!completed)
        );
        if completed {
            assert_eq!(*work, old_work);
        }
        // Moving completes the remaining old requirement. Moving back never
        // restarts it, and an explicit new requirement is independently owned.
        for (n, time, to) in [(2100, 210, id(7)), (2101, 220, id(6))] {
            let current = db
                .hub_group(owner(), command.balancing_group)
                .await
                .unwrap();
            db.submit_hub_mutation(
                scope,
                operation(n),
                HubMutation::Handling(Box::new(HandlingCommand::Move(MovementCommand {
                    expected_stock_revision: current.revision,
                    expected_catalog_revision: current.catalog_revision,
                    group: current.group.id,
                    expected_group_revision: current.group.revision,
                    to,
                    packs: current.group.packs,
                    separated_portion: None,
                    occurred: Occurrence::Known(at(time)),
                    observation: note("Later explicit complete movement"),
                }))),
                at(time),
            )
            .await
            .unwrap();
        }
        let current = db
            .hub_group(owner(), command.balancing_group)
            .await
            .unwrap();
        db.submit_hub_mutation(
            scope,
            operation(2102),
            HubMutation::Handling(Box::new(HandlingCommand::NewWork(NewWorkCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                groups: vec![current.group.id],
                target: id(7),
                observation: note("Explicit new handling after completion, no old work reset"),
            }))),
            at(230),
        )
        .await
        .unwrap();
        let current = db
            .hub_group(owner(), command.balancing_group)
            .await
            .unwrap();
        assert_eq!(current.group.work.len(), 2);
        assert_eq!(
            current.group.work[0]
                .requirement
                .quantity
                .remaining()
                .value()
                + current.group.work[1]
                    .requirement
                    .quantity
                    .remaining()
                    .value(),
            12
        );
        db.submit_hub_mutation(
            scope,
            operation(2103),
            HubMutation::Disposition(Box::new(DispositionCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: current.group.id,
                expected_group_revision: current.group.revision,
                packs: current.group.packs,
                kind: DispositionKind::OtherObservedRemoval,
                occurred: Occurrence::Known(at(240)),
                observation: note("Later separately observed complete removal"),
            })),
            at(240),
        )
        .await
        .unwrap();
        let trace = receipt.original.original_line.trace;
        let snapshot = db
            .hub_trace_snapshot(
                owner(),
                crate::HubTraceSnapshotQuery {
                    trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        let balance = snapshot.balance().unwrap();
        assert_eq!(
            (
                balance.on_hand.value(),
                balance.departed.value(),
                balance.disposed.value()
            ),
            (0, 8, 12)
        );
        assert_eq!(
            db.hub_departure_operation(owner(), departure.operation.clone())
                .await
                .unwrap(),
            Some(departure)
        );
        assert_eq!(
            db.hub_additional_quantity_original(owner(), operation(1900))
                .await
                .unwrap(),
            Some(*receipt.clone())
        );
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_additional_quantity_original(owner(), operation(1900))
                .await
                .unwrap(),
            Some(*receipt)
        );
        db.hub_work_page(owner(), crate::HubHandlingQuery::default())
            .await
            .unwrap();
        db.close().await.unwrap();
    }
}

async fn hold(db: &Database, scope: HubSubmissionScope, group: HubRecordId, time: i64) {
    let current = db.hub_group(owner(), group).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(1850),
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred: Occurrence::Known(at(time)),
            observation: note("Exact current physical cohort held independently"),
        }))),
        at(time),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn found_additional_cohort_inherits_only_hold_captured_before_its_exact_debit_revision() {
    for pre_debit in [true, false] {
        let (fixture, db, mut command) =
            super::super::additional_review::tests::actual_world().await;
        let scope = scope(&db).await;
        if pre_debit {
            hold(&db, scope, command.balancing_group, 190).await;
            let current = db
                .hub_group(owner(), command.balancing_group)
                .await
                .unwrap();
            command.facts.expected_stock_revision = current.revision;
            command.facts.expected_catalog_revision = current.catalog_revision;
            command.expected_balancing_revision = current.group.revision;
        }
        let crate::HubMutationResult::AdditionalCorrected(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::AdditionalQuantity(Box::new(command.clone())),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("held additional")
        };
        if !pre_debit {
            hold(&db, scope, command.balancing_group, 210).await;
        }
        let target =
            crate::hub::actuals::common::operation(command.facts.target_operation).unwrap();
        let found = found(
            &db,
            scope,
            &target,
            command.facts.target_group,
            PhysicalDebitKey {
                operation: receipt.original.operation,
                group: command.balancing_group,
            },
            2200,
            220,
        )
        .await;
        let child = found.original.allocations[0].group.id;
        let treatment = db
            .hub_treatment(
                owner(),
                crate::HubTreatmentScope {
                    trace: receipt.original.original_line.trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        let selected = treatment
            .projection
            .groups
            .iter()
            .find(|g| g.group == child)
            .unwrap();
        assert_eq!(selected.acceptance, AcceptanceStatus::ReviewRequired);
        assert_eq!(selected.active_holds.len(), usize::from(pre_debit));
        assert_eq!(
            treatment
                .projection
                .groups
                .iter()
                .find(|g| g.group == command.balancing_group)
                .unwrap()
                .active_holds
                .len(),
            1
        );
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_additional_quantity_original(owner(), receipt.operation.clone())
                .await
                .unwrap(),
            Some(*receipt)
        );
        assert_eq!(
            db.hub_found_correction_original(owner(), found.operation.clone())
                .await
                .unwrap(),
            Some(*found)
        );
        db.close().await.unwrap();
    }
}
