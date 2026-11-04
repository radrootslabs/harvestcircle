use super::*;
use harvestcircle_domain::{
    SafeErrorCode,
    hub::reference::{HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue},
};

#[tokio::test]
async fn correction_created_source_reuse_later_issuer_version_and_new_history_preserve_every_original()
 {
    let (fixture, db, scope, input) = world().await;
    let crate::HubMutationResult::SourceAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let catalog = db.hub_references(owner()).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(1800),
        HubMutation::Reference(Box::new(ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(3),
                expected_record_revision: catalog.get(id(3)).unwrap().revision,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Renamed current issuer, immutable original retained")
                        .unwrap(),
                },
            },
        })),
        at(210),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let mut new_receipt = receiving(&catalog);
    new_receipt.expected_stock_revision =
        db.hub_stock_page(owner(), query()).await.unwrap().revision;
    new_receipt.source = source();
    let crate::HubMutationResult::Received(later) = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::Receive(Box::new(new_receipt)),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("receipt")
    };
    assert_eq!(
        later
            .original
            .trace
            .source_issuer
            .as_ref()
            .unwrap()
            .revision
            .value(),
        2
    );
    assert_eq!(
        first
            .original
            .after
            .issuer
            .as_ref()
            .unwrap()
            .revision
            .value(),
        1
    );
    let next = command(
        &db,
        input.trace,
        SourceAttribution::Unknown,
        Some(first.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::SourceAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::SourceAttribution(Box::new(next)),
            at(230),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let crate::HubMutationResult::SourceAttributed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input)),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    assert_eq!(replay, first);
    let mut invalid = receiving(&catalog);
    invalid.expected_stock_revision = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    invalid.source = source();
    if let SourceAttribution::Referenced { source, .. } = &mut invalid.source {
        source.provenance =
            SourceProvenance::new("Conflicting content cannot reuse a correction-created identity")
                .unwrap();
    }
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::Receive(Box::new(invalid)),
            at(240)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    let after = crate::hub::custody::tests::material(&fixture).await;
    let effects = |rows: Vec<(String, Vec<String>)>| {
        rows.into_iter()
            .filter(|(name, _)| {
                !matches!(
                    name.as_str(),
                    "hub_submissions" | "hub_submission_rejections"
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(effects(after.clone()), effects(before));
    assert_eq!(
        db.hub_submission(owner(), operation(3))
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::HubSubmissionStatus::Rejected(SafeErrorCode::HubConflict)
    );
    let before = after;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1901))
            .await
            .unwrap(),
        Some(*second)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(*later)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn source_meaning_cannot_clear_a_current_hold_or_change_any_physical_work_or_issued_table() {
    use harvestcircle_domain::hub::{
        movement::HandlingCommand,
        stock::Occurrence,
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let (fixture, db, scope, input) = world().await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = page.rows[0].position.group;
    db.submit_hub_mutation(
        scope,
        operation(1850),
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred: Occurrence::Known(at(180)),
            observation: SourceProvenance::new(
                "Independent current physical hold cannot be cleared by source attribution",
            )
            .unwrap(),
        }))),
        at(180),
    )
    .await
    .unwrap();
    let treatment_query = crate::HubTreatmentScope {
        trace: input.trace,
        expected_revision: None,
        expected_catalog_revision: None,
    };
    let held = db
        .hub_treatment(owner(), treatment_query.clone())
        .await
        .unwrap();
    assert_eq!(held.holds.len(), 1);
    assert_eq!(held.holds[0].current_packs.value(), 20);
    let input = command(&db, input.trace, source(), None).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.submit_hub_mutation(
        scope,
        operation(1900),
        HubMutation::SourceAttribution(Box::new(input)),
        at(200),
    )
    .await
    .unwrap();
    let after = crate::hub::custody::tests::material(&fixture).await;
    let physical = |rows: Vec<(String, Vec<String>)>| {
        rows.into_iter()
            .filter(|(name, _)| {
                !matches!(
                    name.as_str(),
                    "hub_stock_operations"
                        | "hub_stock_state"
                        | "hub_submissions"
                        | "hub_corrections"
                        | "hub_correction_scope_links"
                        | "hub_source_references"
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(physical(before), physical(after));
    let current = db
        .hub_treatment(owner(), treatment_query.clone())
        .await
        .unwrap();
    assert_eq!(current.holds, held.holds);
    let mut expected_projection = held.projection.clone();
    expected_projection.revision = current.revision;
    assert_eq!(current.projection, expected_projection);
    assert_eq!(current.trace, held.trace);
    assert_eq!(
        current.normal_operations_available,
        held.normal_operations_available
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_treatment(owner(), treatment_query).await.unwrap(),
        current
    );
    db.close().await.unwrap();
}
