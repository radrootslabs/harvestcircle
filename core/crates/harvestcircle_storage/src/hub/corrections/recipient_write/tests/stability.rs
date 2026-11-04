use super::*;
use harvestcircle_domain::hub::{
    movement::PhysicalIdentification,
    reference::{
        HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceAttribution,
        WholePacks,
    },
    returns::{ReturnCommand, ReturnDepartureClaim, ReturnLinkResolution},
};

async fn known(db: &Database, trace: HubRecordId) -> crate::HubCorrectionKnownDeparture {
    let mut evidence = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(evidence.departures.len(), 1);
    evidence.departures.remove(0)
}

#[tokio::test]
async fn whole_multipopulation_event_survives_reference_changes_and_returns_keep_original_funding()
{
    let (fixture, db, scope, input) = world().await;
    let departure = db
        .hub_departure_operation(owner(), operation(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(departure.record.consumed.len(), 3);
    let paper = db
        .hub_dispatch_operation(owner(), operation(21))
        .await
        .unwrap();
    let crate::HubMutationResult::RecipientAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert_eq!(first.original.original.populations.len(), 2);
    assert_eq!(first.original.original.issued_versions.len(), 1);
    for (n, party, label) in [
        (1800, id(8), "Renamed original party"),
        (1801, id(9), "Renamed amended party"),
    ] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.submit_hub_mutation(
            scope,
            operation(n),
            HubMutation::Reference(Box::new(ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: party,
                    expected_record_revision: catalog.get(party).unwrap().revision,
                    value: ReferenceValue::Party {
                        label: HubLabel::new(label).unwrap(),
                    },
                },
            })),
            at(210 + i64::from(n - 1800)),
        )
        .await
        .unwrap();
    }
    let next = command(
        &db,
        input.departure_operation,
        Some(id(9)),
        Some(first.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::RecipientAttribution(Box::new(next)),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert_eq!(first.original.original, second.original.original);
    let AmendedRecipient::Known(current) = &second.original.after else {
        panic!("known")
    };
    assert_eq!(current.revision.value(), 2);
    assert!(
        matches!(&current.value,ReferenceValue::Party { label } if label.as_str()=="Renamed amended party")
    );
    assert_eq!(second.original.original.recipient.revision.value(), 1);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let selected = &departure.record.consumed[0].before;
    let return_input = ReturnCommand {
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        variant: selected.variant,
        location: id(6),
        returned_by: Some(id(9)),
        source: SourceAttribution::Unknown,
        packs: WholePacks::from_value(2).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new("Separate returned two after recipient review").unwrap(),
            observation: note(),
        },
        planned_location: None,
        departure: Some(ReturnDepartureClaim {
            departure: departure.record.id,
            group: Some(selected.id),
        }),
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    let crate::HubMutationResult::Returned(returned) = db
        .submit_hub_mutation(
            scope,
            operation(1950),
            HubMutation::Return(Box::new(return_input.clone())),
            at(230),
        )
        .await
        .unwrap()
    else {
        panic!("return")
    };
    assert_eq!(
        returned.original.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    assert_eq!(
        returned
            .original
            .linkage
            .departure
            .as_ref()
            .unwrap()
            .recipient
            .id,
        id(8)
    );
    assert_eq!(
        returned.original.intake.returned_by.as_ref().unwrap().id,
        id(9)
    );
    let before = known(&db, selected.trace).await;
    let next = command(
        &db,
        input.departure_operation,
        None,
        Some(second.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(third) = db
        .submit_hub_mutation(
            scope,
            operation(1902),
            HubMutation::RecipientAttribution(Box::new(next)),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let after = known(&db, selected.trace).await;
    assert_eq!(before.funding, after.funding);
    assert_eq!(before.quantities, after.quantities);
    assert_eq!(before.original, after.original);
    assert_eq!(
        after.recipient_history,
        vec![
            first.original.clone(),
            second.original.clone(),
            third.original.clone()
        ]
    );
    for trace in &first.original.original.populations {
        let snapshot = db
            .hub_trace_snapshot(
                owner(),
                crate::HubTraceSnapshotQuery {
                    trace: *trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(snapshot.recipient_corrections.len(), 3);
        assert!(matches!(
            snapshot
                .current_recipient(input.departure_operation)
                .unwrap(),
            AmendedRecipient::Unknown
        ));
        assert_eq!(snapshot.exits[0].departure.id, departure.record.id);
        assert_eq!(
            snapshot.exits[0].departure.operation,
            departure.record.operation
        );
        assert_eq!(
            snapshot.exits[0].departure.recipient,
            departure.record.paper.versions[0].material.recipient
        );
        assert_eq!(
            snapshot.exits[0].departure.stock_revision,
            departure.record.stock_revision
        );
    }
    let before = crate::hub::custody::tests::material(&fixture).await;
    let crate::HubMutationResult::RecipientAttributed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(260),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert_eq!(replay, first);
    let crate::HubMutationResult::Returned(replay) = db
        .submit_hub_mutation(
            scope,
            operation(1950),
            HubMutation::Return(Box::new(return_input)),
            at(260),
        )
        .await
        .unwrap()
    else {
        panic!("return")
    };
    assert_eq!(replay, returned);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1901))
            .await
            .unwrap(),
        Some(*second)
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1902))
            .await
            .unwrap(),
        Some(*third)
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(1950))
            .await
            .unwrap(),
        Some(*returned)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departure)
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), operation(21))
            .await
            .unwrap(),
        paper
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
