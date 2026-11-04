use super::*;
use harvestcircle_domain::hub::{
    counts::{CountObserveCommand, CountPostCommand},
    reference::SourceAttribution,
    returns::{ReturnCommand, ReturnLinkResolution, UnresolvedReturnLink},
};

async fn observe_current(
    db: &Database,
    scope: HubSubmissionScope,
    group: HubRecordId,
    number: u32,
    time: i64,
) -> Box<crate::HubCountObservationReceipt> {
    let current = db.hub_group(owner(), group).await.unwrap();
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(number),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group,
                expected_group_revision: current.group.revision,
                counted_packs: current.group.packs,
                cutoff: Occurrence::Known(at(time)),
                observation: note("Independent current count preserves its exact physical cutoff"),
            })),
            at(time),
        )
        .await
        .unwrap()
    else {
        panic!("observed current")
    };
    observed
}

#[tokio::test]
async fn count_cutoff_keeps_reviewed_actual_return_and_found_birth_recording_times() {
    let (fixture, db, command) = super::super::additional_review::tests::actual_world().await;
    let scope = scope(&db).await;
    let target = crate::hub::actuals::common::operation(command.facts.target_operation).unwrap();
    let original = db
        .hub_actual_review_operation(owner(), target)
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = original.original else {
        panic!("actual debit")
    };
    let observed = observe_current(&db, scope, command.balancing_group, 2300, 190).await;
    assert_eq!(
        observed.original.scope.latest_physical_recorded_at,
        plan.recorded_at
    );
    db.close().await.unwrap();
    drop(fixture);

    let (fixture, db, _, scope, _) = normal_world(true, false).await;
    let returned = db
        .hub_return_operation(owner(), operation(100))
        .await
        .unwrap()
        .unwrap();
    let observed = observe_current(&db, scope, returned.original.group.id, 2300, 190).await;
    assert_eq!(observed.original.scope.latest_physical_recorded_at, at(120));
    db.close().await.unwrap();
    drop(fixture);

    let (fixture, db, command, scope, departure) = normal_world(false, false).await;
    let crate::HubMutationResult::AdditionalCorrected(additional) = db
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
    let recovered = found(
        &db,
        scope,
        &departure.operation,
        departure.record.consumed[0].before.id,
        PhysicalDebitKey {
            operation: additional.original.operation,
            group: command.balancing_group,
        },
        2200,
        210,
    )
    .await;
    let observed = observe_current(
        &db,
        scope,
        recovered.original.allocations[0].group.id,
        2300,
        220,
    )
    .await;
    assert_eq!(observed.original.scope.latest_physical_recorded_at, at(211));
    db.close().await.unwrap();
    drop(fixture);
}

#[tokio::test]
async fn later_count_uses_additional_physical_cutoff_and_preserves_its_original_after_post_and_reopen()
 {
    let (fixture, db, command, scope, departure) = normal_world(false, false).await;
    let crate::HubMutationResult::AdditionalCorrected(additional) = db
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
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(2300),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: current.group.id,
                expected_group_revision: current.group.revision,
                counted_packs: WholePacks::from_value(11).unwrap(),
                cutoff: Occurrence::Known(at(210)),
                observation: note(
                    "Later independently observed eleven after correction left twelve",
                ),
            })),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("later count")
    };
    assert_eq!(observed.original.scope.recorded_packs.value(), 12);
    assert_eq!(observed.original.scope.latest_physical_recorded_at, at(200));
    let review = db
        .hub_count_review(owner(), observed.original.id)
        .await
        .unwrap();
    db.submit_hub_mutation(
        scope,
        operation(2301),
        HubMutation::CountPosting(Box::new(CountPostCommand {
            expected_stock_revision: review.stock_revision,
            expected_catalog_revision: review.catalog_revision,
            count: observed.original.id,
            reviewed_activity: true,
            surplus_identification: None,
            reason: note(
                "This later independent count is not reclassification of the known departure",
            ),
        })),
        at(220),
    )
    .await
    .unwrap();
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: additional.original.original_line.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.departed.value(),
            balance.counted_loss.value()
        ),
        (20, 11, 8, 1)
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
        Some(*additional.clone())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*additional)
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(2300))
            .await
            .unwrap(),
        Some(*observed)
    );
    db.hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn later_linked_returns_exhaust_original_six_and_never_borrow_additional_two() {
    let (fixture, db, command, scope, departure) = normal_world(true, false).await;
    let crate::HubMutationResult::AdditionalCorrected(additional) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::AdditionalQuantity(Box::new(command)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("additional")
    };
    let mut returns = Vec::new();
    for (n, time) in [(2400, 210), (2401, 220), (2402, 230)] {
        let state = db
            .hub_stock_page(owner(), crate::hub::stock::tests::query())
            .await
            .unwrap();
        let crate::HubMutationResult::Returned(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(n),
                HubMutation::Return(Box::new(ReturnCommand {
                    expected_stock_revision: state.revision,
                    expected_catalog_revision: state.catalog_revision,
                    variant: id(2),
                    location: id(6),
                    returned_by: None,
                    source: SourceAttribution::Unknown,
                    packs: WholePacks::from_value(2).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new(format!("Independent later return {n}")).unwrap(),
                        observation: note("Separate actual returned custody"),
                    },
                    planned_location: None,
                    departure: Some(ReturnDepartureClaim {
                        departure: departure.record.id,
                        group: Some(departure.record.consumed[0].before.id),
                    }),
                    occurred: Occurrence::Unknown,
                    observation: note("Return claimed only against original A; no B allocation"),
                })),
                at(time),
            )
            .await
            .unwrap()
        else {
            panic!("returned")
        };
        assert_eq!(
            receipt.original.linkage.resolution,
            if n == 2402 {
                ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
            } else {
                ReturnLinkResolution::EstablishedScope
            }
        );
        let current = funding(&db, &departure).await;
        assert_eq!(current.quantity.original().packs.value(), 6);
        assert_eq!(current.quantity.effective().value(), 8);
        assert_eq!(
            current.quantity.established_returns().value(),
            if n == 2400 { 4 } else { 6 }
        );
        assert_eq!(current.debits[1].original().packs.value(), 2);
        assert_eq!(current.debits[1].returned().value(), 0);
        if n != 2402 {
            assert_eq!(
                current.debits[0].available().unwrap().value(),
                if n == 2400 { 2 } else { 0 }
            );
            assert_eq!(current.debits[1].available().unwrap().value(), 2);
        } else {
            assert!(current.debits[0].available().is_err());
            assert!(current.debits[1].available().is_err());
        }
        assert_eq!(
            db.hub_additional_quantity_original(owner(), operation(1900))
                .await
                .unwrap(),
            Some(*additional.clone())
        );
        returns.push(*receipt);
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    for receipt in returns {
        assert_eq!(
            db.hub_return_operation(owner(), receipt.operation.clone())
                .await
                .unwrap(),
            Some(receipt)
        );
    }
    assert_eq!(
        db.hub_additional_quantity_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*additional)
    );
    db.close().await.unwrap();
}
