//! Real later consumers of a legally posted found correction, with no synthetic
//! terminal rows or reconstructed execution grants.
use super::*;

#[tokio::test]
async fn later_linked_returns_use_corrected_bound_and_keep_excess_intake_unresolved_after_reopen() {
    use harvestcircle_domain::hub::returns::{
        ReturnDepartureClaim, ReturnLinkResolution, UnresolvedReturnLink,
    };
    let world = world(2, PriorReturn::Established).await;
    let found = post_world(&world).await;
    let work = work_rows(crate::hub::custody::tests::material(&world.fixture).await);
    let mut returns = Vec::new();
    for (op, time) in [(1900, 210), (1901, 220)] {
        let mut input = super::super::super::return_funding_tests::return_command(
            &world.db,
            Some(ReturnDepartureClaim {
                departure: world.departure.record.id,
                group: Some(world.departure.record.consumed[0].before.id),
            }),
            id(2),
        )
        .await;
        input.identification.label =
            HubLabel::new(format!("Later independently returned two {op}")).unwrap();
        let crate::HubMutationResult::Returned(receipt) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(op),
                HubMutation::Return(Box::new(input)),
                at(time),
            )
            .await
            .unwrap()
        else {
            panic!("actual returned custody")
        };
        assert!(receipt.original.current_allowances.is_some());
        assert_eq!(receipt.original.intake.packs.value(), 2);
        assert_eq!(
            receipt.original.acceptance,
            AcceptanceStatus::ReviewRequired
        );
        assert!(receipt.original.group.work.is_empty());
        if op == 1900 {
            assert_eq!(
                receipt.original.linkage.resolution,
                ReturnLinkResolution::EstablishedScope
            );
            let funding = world
                .db
                .hub_correction_target_funding(
                    owner(),
                    crate::HubCorrectionFundingQuery {
                        target_operation: world.departure.operation.clone(),
                        original_group: world.departure.record.consumed[0].before.id,
                        expected_stock_revision: None,
                        expected_catalog_revision: None,
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                (
                    funding.quantity.original().packs.value(),
                    funding.quantity.effective().value(),
                    funding.quantity.established_returns().value()
                ),
                (6, 4, 4)
            );
            assert_eq!(funding.debits[0].available().unwrap().value(), 0);
        } else {
            assert_eq!(
                receipt.original.linkage.resolution,
                ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
            );
        }
        assert_eq!(
            world
                .db
                .hub_found_correction_original(owner(), found.operation.clone())
                .await
                .unwrap(),
            Some(*found.clone())
        );
        returns.push(*receipt);
    }
    assert_eq!(
        work_rows(crate::hub::custody::tests::material(&world.fixture).await),
        work
    );
    let page = world
        .db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        22
    );
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    for receipt in returns {
        assert_eq!(
            reopened
                .hub_return_operation(owner(), receipt.operation.clone())
                .await
                .unwrap(),
            Some(receipt)
        );
    }
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap(),
        Some(*found)
    );
    assert_eq!(
        reopened
            .hub_return_operation(owner(), world.returned.as_ref().unwrap().operation.clone())
            .await
            .unwrap(),
        world.returned
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn explicit_new_work_can_complete_on_found_child_without_rewriting_old_work_or_birth_original()
 {
    use harvestcircle_domain::hub::movement::{
        HandlingCommand, MovementCommand, NewWorkCommand, WorkProgress,
    };
    let world = world(2, PriorReturn::None).await;
    let found = post_world(&world).await;
    let child = found.original.allocations[0].group.id;
    let old = work_rows(crate::hub::custody::tests::material(&world.fixture).await);
    let current = world.db.hub_group(owner(), child).await.unwrap();
    let target = if current.group.location == id(7) {
        id(6)
    } else {
        id(7)
    };
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1900),
            HubMutation::Handling(Box::new(HandlingCommand::NewWork(NewWorkCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                groups: vec![child],
                target,
                observation: note(
                    "Explicit new two-pack requirement, independent of retired original handling",
                ),
            }))),
            at(210),
        )
        .await
        .unwrap();
    let current = world.db.hub_group(owner(), child).await.unwrap();
    assert_eq!(current.group.work.len(), 1);
    let fresh = current.group.work[0].requirement.id;
    assert!(
        world.departure.record.consumed[0]
            .before
            .work
            .iter()
            .all(|w| w.requirement.id != fresh)
    );
    assert_eq!(
        (
            current.group.work[0]
                .requirement
                .quantity
                .required()
                .value(),
            current.group.work[0]
                .requirement
                .quantity
                .completed()
                .value(),
            current.group.work[0]
                .requirement
                .quantity
                .remaining()
                .value()
        ),
        (2, 0, 2)
    );
    assert_eq!(current.group.work[0].progress, WorkProgress::Pending);
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1901),
            HubMutation::Handling(Box::new(HandlingCommand::Move(MovementCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: child,
                expected_group_revision: current.group.revision,
                to: target,
                packs: current.group.packs,
                separated_portion: None,
                occurred: Occurrence::Known(at(220)),
                observation: note("Actual movement completes only this explicit new work"),
            }))),
            at(220),
        )
        .await
        .unwrap();
    let current = world.db.hub_group(owner(), child).await.unwrap();
    assert_eq!(current.group.work[0].requirement.id, fresh);
    assert_eq!(
        (
            current.group.work[0]
                .requirement
                .quantity
                .completed()
                .value(),
            current.group.work[0]
                .requirement
                .quantity
                .no_longer_required()
                .value(),
            current.group.work[0]
                .requirement
                .quantity
                .remaining()
                .value()
        ),
        (2, 0, 0)
    );
    let after = work_rows(crate::hub::custody::tests::material(&world.fixture).await);
    for (table, rows) in old {
        let current = &after.iter().find(|(t, _)| *t == table).unwrap().1;
        assert!(
            rows.iter().all(|row| current.contains(row)),
            "unchanged old {table}"
        );
    }
    assert!(found.original.allocations[0].group.work.is_empty());
    assert_eq!(
        world
            .db
            .hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap(),
        Some(*found.clone())
    );
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap(),
        Some(*found)
    );
    assert_eq!(
        reopened.hub_group(owner(), child).await.unwrap().group,
        current.group
    );
    reopened.close().await.unwrap();
}
