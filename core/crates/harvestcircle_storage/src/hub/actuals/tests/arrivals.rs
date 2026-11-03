use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::HubMutation;
use harvestcircle_domain::hub::{
    movement::MovementCommand,
    reference::{LocationPurpose, ReferenceCommand, ReferenceMutation, ReferenceValue},
};

#[tokio::test]
async fn historical_a1_adjusts_current_b2_without_lowering_later_floor_or_rewriting_completed_work()
{
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let group = intake.original.position.group;
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        MovementCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group,
            expected_group_revision: current.group.revision,
            to: id(7),
            packs: packs(20),
            separated_portion: None,
            occurred: Occurrence::Known(at(155)),
            observation: note("Later recorded move to B completes its original twenty-pack work"),
        },
        at(170),
    )
    .await
    .unwrap();
    for (n, label, purpose) in [
        (6, "Receiving A2", LocationPurpose::Receiving),
        (7, "Storage B2", LocationPurpose::Storage),
    ] {
        let catalog = db.hub_references(owner()).await.unwrap();
        let prior = catalog.get(id(n)).unwrap();
        db.apply_hub_reference(
            owner(),
            operation(1000 + u32::from(n)),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: id(n),
                    expected_record_revision: prior.revision,
                    value: ReferenceValue::Location {
                        label: HubLabel::new(label).unwrap(),
                        purpose,
                    },
                },
            },
            at(175),
        )
        .await
        .unwrap();
    }
    let scope = scope(&db).await;
    let mut input = report();
    input.occurred = Occurrence::Interval {
        earliest: at(140),
        latest: at(190),
    };
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(input)),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let mut command = review(&db, saved.original.id, group).await;
    command.facts.occurred = Occurrence::Known(at(145));
    let crate::HubMutationResult::ActualReviewed(original) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("review")
    };
    let HubActualReviewOriginal::Departure(plan) = &original.original else {
        panic!("physical")
    };
    assert_eq!(plan.historical_source.historical().location.id, id(6));
    assert_eq!(
        plan.historical_source
            .historical()
            .location
            .revision
            .value(),
        1
    );
    assert_eq!(
        plan.historical_source
            .historical()
            .location
            .value
            .label()
            .as_str(),
        "Receiving"
    );
    assert_eq!(plan.comparison.scope.current_location.id, id(7));
    assert_eq!(plan.comparison.scope.current_location.revision.value(), 2);
    assert_eq!(
        plan.comparison
            .scope
            .current_location
            .value
            .label()
            .as_str(),
        "Storage B2"
    );
    assert_eq!(plan.after_causal_floor, Some(at(155)));
    assert!(plan.reduction.work_changes.is_empty());
    let after = db.hub_group(owner(), group).await.unwrap();
    assert_eq!(after.group.location, id(7));
    assert_eq!(after.group.packs, packs(18));
    assert_eq!(after.group.causal_floor, Some(at(155)));
    assert_eq!(
        after.group.work[0].requirement.quantity.completed(),
        packs(20)
    );
    assert_eq!(
        after.group.work[0]
            .requirement
            .quantity
            .no_longer_required(),
        packs(0)
    );
    let catalog = db.hub_references(owner()).await.unwrap();
    let prior = catalog.get(id(7)).unwrap();
    db.submit_hub_mutation(
        scope,
        operation(82),
        HubMutation::Reference(Box::new(ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(7),
                expected_record_revision: prior.revision,
                value: ReferenceValue::Location {
                    label: HubLabel::new("Later B3").unwrap(),
                    purpose: LocationPurpose::Storage,
                },
            },
        })),
        at(210),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap(),
        Some(*original.clone())
    );
    let source = db
        .hub_actual_source(
            owner(),
            HubActualSourceQuery {
                context: query(intake.original.trace.id),
                operation: operation(1),
                group,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        source.known_departures[0].lines[0]
            .location
            .value
            .label()
            .as_str(),
        "Receiving"
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap(),
        Some(*original)
    );
    assert_eq!(
        db.hub_group(owner(), group).await.unwrap().group,
        after.group
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn chained_actuals_restore_each_original_cut_and_current_work_without_recursive_history() {
    let started = std::time::Instant::now();
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let mut originals = Vec::new();
    for n in 0..3 {
        let mut input = report();
        input.packs = Some(packs(1));
        let crate::HubMutationResult::ActualReported(saved) = db
            .submit_hub_mutation(
                scope,
                operation(80 + 2 * n),
                HubMutation::ActualReport(Box::new(input)),
                at(150 + 10 * i64::from(n)),
            )
            .await
            .unwrap()
        else {
            panic!("report")
        };
        let mut command = review(&db, saved.original.id, intake.original.position.group).await;
        command.facts.packs = packs(1);
        command.facts.occurred = Occurrence::Known(at(110 + 10 * i64::from(n)));
        let crate::HubMutationResult::ActualReviewed(original) = db
            .submit_hub_mutation(
                scope,
                operation(81 + 2 * n),
                HubMutation::ActualReview(Box::new(command)),
                at(155 + 10 * i64::from(n)),
            )
            .await
            .unwrap()
        else {
            panic!("review")
        };
        originals.push(original);
    }
    let after = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(after.group.packs, packs(17));
    assert_eq!(
        after.group.work[0].requirement.quantity.remaining(),
        packs(17)
    );
    assert_eq!(
        after.group.work[0]
            .requirement
            .quantity
            .no_longer_required(),
        packs(3)
    );
    for (n, original) in originals.iter().enumerate() {
        assert_eq!(
            db.hub_actual_review_operation(owner(), operation(81 + 2 * n as u32))
                .await
                .unwrap()
                .as_ref(),
            Some(original.as_ref())
        );
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap(),
        after
    );
    for (n, original) in originals.into_iter().enumerate() {
        assert_eq!(
            db.hub_actual_review_operation(owner(), operation(81 + 2 * n as u32))
                .await
                .unwrap(),
            Some(*original)
        );
    }
    db.close().await.unwrap();
    println!(
        "Three-review current-work/original/reopen world elapsed: {:?}",
        started.elapsed()
    );
}
