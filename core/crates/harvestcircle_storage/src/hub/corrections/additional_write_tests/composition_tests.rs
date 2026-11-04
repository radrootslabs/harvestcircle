use super::*;

#[tokio::test]
async fn successive_additional_debits_share_group_but_not_operation_funding_after_found() {
    let (fixture, db, mut command) = super::super::additional_review::tests::actual_world().await;
    let scope = scope(&db).await;
    let target = crate::hub::actuals::common::operation(command.facts.target_operation).unwrap();
    let group = command.facts.target_group;
    assert_eq!(group, command.balancing_group);
    let crate::HubMutationResult::AdditionalCorrected(first) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::AdditionalQuantity(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("first")
    };
    let crate::HubMutationResult::ActualReported(raw) = db
        .submit_hub_mutation(
            scope,
            operation(1950),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(first.original.original_line.variant),
                recipient: command.facts.recipient,
                packs: Some(WholePacks::from_value(6).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note("Independent new corrected total six, not six more"),
            })),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("new raw")
    };
    let current = db.hub_group(owner(), group).await.unwrap();
    command.facts.report = raw.original.id;
    command.facts.expected_stock_revision = current.revision;
    command.facts.expected_catalog_revision = current.catalog_revision;
    command.facts.expected_prior = Some(first.original.operation);
    command.facts.corrected_line_packs = WholePacks::from_value(6).unwrap();
    command.expected_balancing_revision = current.group.revision;
    let crate::HubMutationResult::AdditionalCorrected(second) = db
        .submit_hub_mutation(
            scope,
            operation(1951),
            HubMutation::AdditionalQuantity(Box::new(command)),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("second")
    };
    let query = crate::HubCorrectionFundingQuery {
        target_operation: target.clone(),
        original_group: group,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    let initial = db
        .hub_correction_target_funding(owner(), query.clone())
        .await
        .unwrap();
    assert_eq!(initial.quantity.effective().value(), 6);
    assert_eq!(initial.debits.len(), 3);
    assert!(
        initial
            .debits
            .iter()
            .all(|d| d.original().key.group == group)
    );
    assert_eq!(
        initial
            .debits
            .iter()
            .map(|d| d.available().unwrap().value())
            .collect::<Vec<_>>(),
        vec![2, 2, 2]
    );
    assert_ne!(
        initial.debits[0].original().key.operation,
        initial.debits[1].original().key.operation
    );
    assert_ne!(
        initial.debits[1].original().key.operation,
        initial.debits[2].original().key.operation
    );
    let recovered = found(
        &db,
        scope,
        &target,
        group,
        initial.debits[1].original().key,
        2000,
        230,
    )
    .await;
    let current = db
        .hub_correction_target_funding(owner(), query.clone())
        .await
        .unwrap();
    assert_eq!(current.quantity.effective().value(), 4);
    assert_eq!(
        current
            .debits
            .iter()
            .map(|d| d.available().unwrap().value())
            .collect::<Vec<_>>(),
        vec![2, 0, 2]
    );
    let trace = first.original.original_line.trace;
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace,
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
            balance.actual_departed.value()
        ),
        (20, 16, 4)
    );
    assert_eq!(balance.departed.value(), 0);
    assert_eq!(
        db.hub_group(owner(), group)
            .await
            .unwrap()
            .group
            .packs
            .value(),
        14
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_original(owner(), first.operation.clone())
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_additional_quantity_original(owner(), second.operation.clone())
            .await
            .unwrap(),
        Some(*second)
    );
    assert_eq!(
        db.hub_found_correction_original(owner(), recovered.operation.clone())
            .await
            .unwrap(),
        Some(*recovered)
    );
    assert_eq!(
        db.hub_correction_target_funding(owner(), query)
            .await
            .unwrap(),
        current
    );
    db.close().await.unwrap();
}
