//! A genuine fresh admission followed by explicitly synthetic lower inventory.
//! This tests the shared writer guard, not 4096 authenticated native originals.
use super::*;
use crate::hub::corrections::count_return_write::tests::native22::association_world;
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        movement::PhysicalIdentification,
        reference::{HubLabel, SourceAttribution},
        returns::ReturnCommand,
        stock::Occurrence,
    },
};

fn at(value: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(value).unwrap()
}
fn own(value: u32) -> HubRecordId {
    crate::hub::actuals::common::operation_id(&operation(value)).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Fictional shared-capacity guard evidence").unwrap()
}

#[tokio::test]
async fn freshly_admitted_association_rejects_combined_hub_capacity_without_an_effect_or_recapture()
{
    let world = association_world().await;
    let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
    let crate::HubMutationResult::Returned(_) = Box::pin(world.db.submit_hub_mutation(
        world.scope,
        operation(1000),
        HubMutation::Return(Box::new(ReturnCommand {
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            variant: crate::hub::stock::tests::id(2),
            location: crate::hub::stock::tests::id(6),
            returned_by: None,
            source: SourceAttribution::Unknown,
            packs: WholePacks::from_value(1).unwrap(),
            identification: PhysicalIdentification {
                label: HubLabel::new("Independent second restricted returned intake").unwrap(),
                observation: note(),
            },
            planned_location: None,
            departure: None,
            occurred: Occurrence::Unknown,
            observation: note(),
        })),
        at(270),
    ))
    .await
    .unwrap() else {
        panic!("ordinary independent Return")
    };
    let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
    let mut command = world.command.clone();
    command.return_operation = own(1000);
    command.expected_returns_prior = Some(own(906));
    command.expected_stock_revision = page.revision;
    command.expected_catalog_revision = page.catalog_revision;
    // Capture/plan validation is genuine and completes before any synthetic row.
    let input = command.clone();
    let scope = world.scope;
    let decision = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1001),
                    HubMutation::CountReturnAssociation(Box::new(input)),
                    at(280),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh ordinary admission")
    };
    let pending = world
        .db
        .hub_submission(owner(), operation(1001))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);

    crate::hub::corrections::count_return_write::tests::native22::capacity::fill_hub_with_lower_source_inventory(&world).await;

    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        Box::pin(execute::execute_fresh(&world.db, grant, at(300)))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    let rejected = world
        .db
        .hub_submission(owner(), operation(1001))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rejected.status,
        crate::HubSubmissionStatus::Rejected(SafeErrorCode::HubCapacity)
    );
    assert_eq!(rejected.captured_semantics(), pending.captured_semantics());
    assert_eq!(rejected.capture_hash(), pending.capture_hash());
    let after = crate::hub::custody::tests::material(&world.fixture).await;
    let effects = |rows: Vec<(String, Vec<String>)>| {
        rows.into_iter()
            .filter(|(table, _)| table != "hub_submission_rejections")
            .collect::<Vec<_>>()
    };
    assert_eq!(effects(after), effects(before));
    assert!(
        world
            .db
            .hub_count_return_association_operation(owner(), operation(1001))
            .await
            .unwrap()
            .is_none()
    );
    let before_replay = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        Box::pin(world.db.submit_hub_mutation(
            world.scope,
            operation(1001),
            HubMutation::CountReturnAssociation(Box::new(command.clone())),
            at(310)
        ))
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubCapacity
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before_replay
    );
    let crate::HubMutationResult::CountReturnAssociated(recorded) =
        Box::pin(world.db.submit_hub_mutation(
            world.scope,
            operation(906),
            HubMutation::CountReturnAssociation(Box::new(world.command.clone())),
            at(320),
        ))
        .await
        .unwrap()
    else {
        panic!("recorded original remains recoverable at shared capacity")
    };
    assert_eq!(recorded, world.associated);
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before_replay
    );
    world.db.close().await.unwrap();
    let db = world.fixture.open().await;
    assert_eq!(
        db.hub_submission(owner(), operation(1001))
            .await
            .unwrap()
            .unwrap(),
        rejected
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before_replay
    );
    db.close().await.unwrap();
}
