use super::*;
use harvestcircle_domain::hub::{
    corrections::CorrectedDepartureFamily,
    departure::DepartureCommand,
    dispatch::{DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection},
    movement::HandlingCommand,
    returns::{ReturnDepartureClaim, ReturnLinkResolution},
    treatment::{AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection},
};

// Keep each native command future out of the larger scenario's poll frame.
// The same ordinary admission/execution API and default thread stack remain.
fn submit<'a>(
    db: &'a Database,
    scope: HubSubmissionScope,
    operation: harvestcircle_application::DurableRequestId,
    mutation: HubMutation,
    now: UnixTimestamp,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<crate::HubMutationResult, SafeError>> + Send + 'a>,
> {
    Box::pin(db.submit_hub_mutation(scope, operation, mutation, now))
}

#[tokio::test]
async fn ordinary_later_departure_and_claimed_return_keep_count_association_disjoint_from_funding()
{
    let world = association_world().await;
    qualify_normal_funding(world).await;
}

fn qualify_normal_funding(
    world: Box<AssociationWorld>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        classification(
            &world.db,
            world.scope,
            own(905),
            (own(905), None),
            1,
            907,
            270,
        )
        .await
        .unwrap();
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let catalog = world.db.hub_references(owner()).await.unwrap();
        let mut receipt = crate::hub::stock::tests::command(&catalog);
        receipt.expected_stock_revision = page.revision;
        receipt.packs = WholePacks::from_value(2).unwrap();
        receipt.planned_location = None;
        receipt.occurred = Occurrence::Known(at(290));
        let crate::HubMutationResult::Received(received) = submit(
            &world.db,
            world.scope,
            operation(1000),
            HubMutation::Receive(Box::new(receipt)),
            at(300),
        )
        .await
        .unwrap() else {
            panic!("ordinary receipt")
        };
        let group = received.original.position.group;
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        submit(
            &world.db,
            world.scope,
            operation(1001),
            HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: TreatmentAction::SetAcceptance {
                    selection: TreatmentSelection::new(vec![group]).unwrap(),
                    status: AcceptanceStatus::Accepted,
                },
                occurred: Occurrence::Known(at(310)),
                observation: evidence(),
            }))),
            at(310),
        )
        .await
        .unwrap();
        let current = world.db.hub_group(owner(), group).await.unwrap();
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Dispatch(prepared) = submit(
            &world.db,
            world.scope,
            operation(1002),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: id(5),
                    lines: DispatchSelection::new(vec![DispatchLineInput {
                        group,
                        expected_group_revision: current.group.revision,
                        packs: WholePacks::from_value(2).unwrap(),
                    }])
                    .unwrap(),
                },
                observation: evidence(),
            })),
            at(320),
        )
        .await
        .unwrap() else {
            panic!("ordinary prepare")
        };
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Dispatch(issued) = submit(
            &world.db,
            world.scope,
            operation(1003),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: DispatchAction::Issue {
                    preparation: prepared.record.id,
                    expected_revision: prepared.record.revision,
                },
                observation: evidence(),
            })),
            at(330),
        )
        .await
        .unwrap() else {
            panic!("ordinary issue")
        };
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Departure(departed) = submit(
            &world.db,
            world.scope,
            operation(1004),
            HubMutation::Departure(Box::new(DepartureCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                preparation: issued.record.id,
                expected_preparation_revision: issued.record.revision,
                identification: issued.record.versions.last().unwrap().id,
                occurred: Occurrence::Known(at(340)),
                observation: evidence(),
            })),
            at(340),
        )
        .await
        .unwrap() else {
            panic!("ordinary departure")
        };
        assert_funding(&world.db, group, 0, 2).await;
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Returned(returned) = submit(
            &world.db,
            world.scope,
            operation(1005),
            HubMutation::Return(Box::new(ReturnCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                variant: id(2),
                location: id(6),
                returned_by: Some(id(5)),
                source: SourceAttribution::Unknown,
                packs: WholePacks::from_value(1).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Separate ordinary claimed return").unwrap(),
                    observation: evidence(),
                },
                planned_location: None,
                departure: Some(ReturnDepartureClaim {
                    departure: departed.record.id,
                    group: Some(group),
                }),
                occurred: Occurrence::Known(at(350)),
                observation: evidence(),
            })),
            at(350),
        )
        .await
        .unwrap() else {
            panic!("ordinary claimed return")
        };
        assert_eq!(
            returned.original.linkage.resolution,
            ReturnLinkResolution::EstablishedScope
        );
        assert_funding(&world.db, group, 1, 1).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(900))
                .await
                .unwrap()
                .as_ref(),
            Some(world.returned.as_ref())
        );
        assert_eq!(
            world
                .db
                .hub_count_return_association_operation(owner(), operation(906))
                .await
                .unwrap()
                .as_ref(),
            Some(world.associated.as_ref())
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_funding(&db, group, 1, 1).await;
        assert_eq!(
            db.hub_return_operation(owner(), operation(1005))
                .await
                .unwrap()
                .as_ref(),
            Some(returned.as_ref())
        );
        assert_eq!(
            db.hub_return_operation(owner(), operation(900))
                .await
                .unwrap()
                .as_ref(),
            Some(world.returned.as_ref())
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        db.close().await.unwrap();
    })
}

async fn assert_funding(db: &Database, group: HubRecordId, returned: u32, available: u32) {
    let evidence = db
        .hub_correction_target_funding(
            owner(),
            crate::HubCorrectionFundingQuery {
                target_operation: operation(1004),
                original_group: group,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(evidence.family, CorrectedDepartureFamily::Normal);
    assert!(!evidence.uncertain_intent && !evidence.unresolved_actual);
    assert_eq!(evidence.debits.len(), 1);
    let debit = &evidence.debits[0];
    assert_eq!(debit.original().packs.value(), 2);
    assert_eq!(debit.returned().value(), returned);
    assert_eq!(debit.available().unwrap().value(), available);
    assert_eq!(debit.returns().len(), returned as usize);
    if returned != 0 {
        assert_eq!(debit.returns()[0].operation, own(1005));
    }
}
