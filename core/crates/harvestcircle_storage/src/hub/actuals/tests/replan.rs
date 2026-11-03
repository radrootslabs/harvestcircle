use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::HubMutation;
use harvestcircle_domain::hub::{
    movement::{HandlingCommand, ReplanCommand},
    reference::{LocationPurpose, ReferenceCommand, ReferenceMutation, ReferenceValue},
};

#[tokio::test]
async fn fully_depleted_actual_child_remains_historical_while_positive_sibling_replans() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let child = separated(&db, intake.original.position.group).await;
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(108),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Create {
                id: id(8),
                value: ReferenceValue::Location {
                    label: HubLabel::new("Separate storage shelf").unwrap(),
                    purpose: LocationPurpose::Storage,
                },
            },
        },
        at(120),
    )
    .await
    .unwrap();
    let scope = scope(&db).await;
    let mut input = report();
    input.packs = Some(packs(6));
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(input)),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let mut command = review(&db, saved.original.id, child).await;
    command.facts.packs = packs(6);
    command.facts.occurred = Occurrence::Known(at(120));
    command.source_operation =
        HubRecordId::from_hex(&operation(2).as_str().replace('-', "")).unwrap();
    let crate::HubMutationResult::ActualReviewed(original) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("review")
    };
    let root = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(root.group.packs, packs(14));
    assert_eq!(
        root.group.work[0].requirement.quantity.remaining(),
        packs(14)
    );
    let work = &root.group.work[0].requirement;
    let command = ReplanCommand {
        expected_stock_revision: root.revision,
        expected_catalog_revision: root.catalog_revision,
        requirement: work.id,
        expected_work_revision: work.revision,
        target: id(8),
        observation: note("Replan only the fourteen packs still physically on hand"),
    };
    db.submit_hub_mutation(
        scope,
        operation(82),
        HubMutation::Handling(Box::new(HandlingCommand::Replan(command))),
        at(170),
    )
    .await
    .unwrap();
    let after = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(after.group.packs, packs(14));
    assert!(after.group.work.iter().any(|allocation| {
        allocation
            .requirement
            .target
            .as_ref()
            .is_some_and(|target| target.id == id(8))
            && allocation.requirement.quantity.remaining() == packs(14)
    }));
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap(),
        Some(*original.clone())
    );
    let evidence = db
        .hub_actual_lineage(owner(), query(intake.original.trace.id))
        .await
        .unwrap();
    assert_eq!(evidence.lineage.node(child).unwrap().packs, packs(0));
    assert_eq!(
        evidence
            .births
            .iter()
            .find(|birth| birth.group == child)
            .unwrap()
            .packs,
        packs(6)
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
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap(),
        after
    );
    db.close().await.unwrap();
}
