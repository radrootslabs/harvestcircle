//! A second canonical native event must reach the unchanged duplicate policy.
use super::*;
use harvestcircle_domain::hub::{
    actuals::ActualReportError,
    departure::DepartureCommand,
    dispatch::{DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection},
    movement::{HandlingCommand, PhysicalIdentification},
    portions::SplitCommand,
    reference::HubLabel,
    treatment::{AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection},
};

#[tokio::test]
async fn overlapping_normal_event_remains_a_native_duplicate_candidate_for_actual_correction() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) = super::super::super::projection_native_tests::actual_debit(&db).await;
    let runtime = db.begin_hub_runtime(at(170)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(170),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let current = db.hub_group(owner(), before.id).await.unwrap();
    let crate::HubMutationResult::Handling(split) = db
        .submit_hub_mutation(
            scope,
            operation(1860),
            HubMutation::Handling(Box::new(HandlingCommand::Split(SplitCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: before.id,
                expected_group_revision: current.group.revision,
                packs: WholePacks::from_value(6).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Independent normal six for same recipient").unwrap(),
                    observation: note("Physically identified separate six"),
                },
                occurred: Occurrence::Known(at(110)),
                observation: note("Actual independently recorded split"),
            }))),
            at(170),
        )
        .await
        .unwrap()
    else {
        panic!("split")
    };
    let crate::HubHandlingResult::Separated(split) = split.result else {
        panic!("separation")
    };
    let group = split.portion_group;
    let current = db.hub_group(owner(), group).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(1861),
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::SetAcceptance {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
                status: AcceptanceStatus::Accepted,
            },
            occurred: Occurrence::Known(at(110)),
            observation: note("Explicit acceptance of the identified normal group"),
        }))),
        at(171),
    )
    .await
    .unwrap();
    let current = db.hub_group(owner(), group).await.unwrap();
    let crate::HubMutationResult::Dispatch(prepared) = db
        .submit_hub_mutation(
            scope,
            operation(1862),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: id(3),
                    lines: DispatchSelection::new(vec![DispatchLineInput {
                        group,
                        expected_group_revision: current.group.revision,
                        packs: current.group.packs,
                    }])
                    .unwrap(),
                },
                observation: note("Independent normal preparation for the same party"),
            })),
            at(172),
        )
        .await
        .unwrap()
    else {
        panic!("preparation")
    };
    let crate::HubMutationResult::Dispatch(issued) = db
        .submit_hub_mutation(
            scope,
            operation(1863),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: prepared.record.updated.stock_revision,
                expected_catalog_revision: prepared.catalog_revision,
                action: DispatchAction::Issue {
                    preparation: prepared.record.id,
                    expected_revision: prepared.record.revision,
                },
                observation: note("Explicit independent issued identification"),
            })),
            at(173),
        )
        .await
        .unwrap()
    else {
        panic!("issued")
    };
    let current = db
        .hub_dispatch_record(owner(), issued.record.id)
        .await
        .unwrap();
    let crate::HubMutationResult::Departure(departure) = db
        .submit_hub_mutation(
            scope,
            operation(1864),
            HubMutation::Departure(Box::new(DepartureCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                preparation: issued.record.id,
                expected_preparation_revision: issued.record.revision,
                identification: issued.record.versions.last().unwrap().id,
                occurred: Occurrence::Known(at(110)),
                observation: note("Known overlapping normal departure"),
            })),
            at(174),
        )
        .await
        .unwrap()
    else {
        panic!("normal departure")
    };
    let command = actual_command(&db, before, target, Some(4)).await;
    let current = db
        .hub_group(owner(), command.balancing_group)
        .await
        .unwrap();
    assert_eq!(current.group.packs.value(), 12);
    let known = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace: current.group.trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(known.departures.len(), 2);
    let material = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
    assert_eq!(
        db.hub_departure_operation(owner(), departure.operation.clone())
            .await
            .unwrap(),
        Some(*departure)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::Actual(
            ActualReportError::DuplicateReviewRequired
        ))
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
}
