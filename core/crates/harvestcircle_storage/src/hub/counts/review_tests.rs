use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receive, now, operation, other, owner, query, setup,
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        counts::{CountBlockReason, CountDifference},
        movement::MovementCommand,
        reference::{LocationPurpose, ReferenceValue, WholePacks},
        stock::Occurrence,
    },
};

async fn observe(db: &Database, quantity: u32, identity: u32) -> HubCountObservationReceipt {
    let catalog = db.hub_references(owner()).await.unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = db
        .hub_group(owner(), page.rows[0].position.group)
        .await
        .unwrap();
    db.observe_hub_count(
        owner(),
        operation(identity),
        CountObserveCommand {
            expected_stock_revision: page.revision,
            expected_catalog_revision: catalog.revision(),
            group: group.group.id,
            expected_group_revision: group.group.revision,
            counted_packs: WholePacks::from_value(quantity).unwrap(),
            cutoff: Occurrence::Known(now()),
            observation: SourceProvenance::new("A fictional observed count for native review")
                .unwrap(),
        },
        now(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn native_review_keeps_observed_eighteen_recorded_twenty_and_work_twenty_without_effects() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let original = db
        .apply_hub_receipt(owner(), operation(1), receive(&catalog), now())
        .await
        .unwrap();
    let count = observe(&db, 18, 2).await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let review = db
        .hub_count_review(owner(), count.original.id)
        .await
        .unwrap();
    assert_eq!(review.observation, count.original);
    assert_eq!(review.current.packs.value(), 20);
    assert_eq!(
        review.current.work[0]
            .requirement
            .quantity
            .remaining()
            .value(),
        20
    );
    assert_eq!(
        review.assessment.difference,
        CountDifference::Shortage(WholePacks::from_value(2).unwrap())
    );
    assert!(review.assessment.ready());
    assert_eq!(
        db.hub_count_review(owner(), count.original.id)
            .await
            .unwrap(),
        review
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_count_review(other(), count.original.id)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_count_review(owner(), count.original.id)
            .await
            .unwrap(),
        review
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
}

#[tokio::test]
async fn newer_native_observation_blocks_the_older_comparison_without_overwriting_it() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receive(&catalog), now())
        .await
        .unwrap();
    let old = observe(&db, 18, 2).await;
    let new = observe(&db, 19, 3).await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let review = db.hub_count_review(owner(), old.original.id).await.unwrap();
    assert_eq!(review.observation, old.original);
    assert_eq!(
        review.assessment.blockers,
        vec![CountBlockReason::NotLatestObservation]
    );
    let latest = db.hub_count_review(owner(), new.original.id).await.unwrap();
    assert!(latest.assessment.ready());
    assert_eq!(
        latest.assessment.difference,
        CountDifference::Shortage(WholePacks::from_value(1).unwrap())
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_movement_after_the_observation_blocks_scope_and_cutoff_review() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receive(&catalog), now())
        .await
        .unwrap();
    let count = observe(&db, 18, 2).await;
    let group = db
        .hub_group(owner(), count.original.scope.group)
        .await
        .unwrap();
    let destination = catalog
        .records()
        .find(|r| {
            matches!(
                r.value,
                ReferenceValue::Location {
                    purpose: LocationPurpose::Storage,
                    ..
                }
            )
        })
        .unwrap()
        .id;
    let later = UnixTimestamp::from_seconds(now().as_seconds() + 1).unwrap();
    db.apply_hub_movement(
        owner(),
        operation(3),
        MovementCommand {
            expected_stock_revision: group.revision,
            expected_catalog_revision: catalog.revision(),
            group: group.group.id,
            expected_group_revision: group.group.revision,
            to: destination,
            packs: group.group.packs,
            separated_portion: None,
            occurred: Occurrence::Known(later),
            observation: SourceProvenance::new("Observed later movement of all twenty crates")
                .unwrap(),
        },
        later,
    )
    .await
    .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let review = db
        .hub_count_review(owner(), count.original.id)
        .await
        .unwrap();
    assert_eq!(review.current.location, destination);
    assert_eq!(review.observation, count.original);
    assert_eq!(
        review.assessment.blockers,
        vec![
            CountBlockReason::ScopeChanged,
            CountBlockReason::CutoffPrecedesRecordedActivity
        ]
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    db.close().await.unwrap();
}
