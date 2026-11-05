//! Cross-family original recovery uses fresh native fixtures, never old witnesses.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualNewDepartureCommand, ActualReportCommand, ActualReportSources},
    corrections::{PhysicalDebitKey, SourceAttributionCommand},
    reference::{
        SourceAttribution, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
        WholePacks,
    },
};
async fn source(
    db: &Database,
    trace: HubRecordId,
    n: u8,
    prior: Option<HubRecordId>,
) -> SourceAttributionCommand {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    SourceAttributionCommand {
        trace,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: prior,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(n),
                issuer: Some(id(3)),
                kind: SourceReferenceKind::Other,
                code: SourceCode::new(format!("Independent fictional source {n}")).unwrap(),
                provenance: note(),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
        basis: note(),
    }
}
#[tokio::test]
async fn native20_direct_found_source_then_source_recipient_source_preserves_both_actual_originals()
{
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) =
        crate::hub::corrections::projection_native_tests::actual_debit(&db).await;
    let original_target = crate::hub::actuals::common::operation_id(&target).unwrap();
    let old_actual = db
        .hub_actual_review_operation(owner(), target.clone())
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(old_plan) = &old_actual.original else {
        panic!("old actual")
    };
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let found = crate::hub::corrections::additional_write_tests::found(
        &db,
        scope,
        &target,
        before.id,
        PhysicalDebitKey {
            operation: original_target,
            group: before.id,
        },
        2200,
        210,
    )
    .await;
    let child = found.original.allocations[0].group.id;
    let current = db.hub_group(owner(), child).await.unwrap();
    let trace = current.group.trace;
    let crate::HubMutationResult::ActualReported(raw) = db
        .submit_hub_mutation(
            scope,
            operation(2300),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(current.group.variant),
                recipient: Some(id(4)),
                packs: Some(WholePacks::from_value(1).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note(),
            })),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("raw found-child event")
    };
    let current = db.hub_group(owner(), child).await.unwrap();
    let mut facts = old_plan.comparison.facts.clone();
    facts.report = raw.original.id;
    facts.group = child;
    facts.expected_group_revision = current.group.revision;
    facts.expected_stock_revision = current.revision;
    facts.expected_catalog_revision = current.catalog_revision;
    facts.recipient = id(4);
    facts.packs = WholePacks::from_value(1).unwrap();
    facts.occurred = Occurrence::Known(at(235));
    let evidence = SourceProvenance::new(
        "Independently reviewed fictional actual departure at235 after found birth211",
    )
    .unwrap();
    facts.source_evidence = evidence.clone();
    facts.quantity_evidence = evidence.clone();
    facts.recipient_evidence = evidence.clone();
    facts.timing_evidence = evidence.clone();
    let reviewed = ActualNewDepartureCommand {
        facts,
        source_operation: found.original.operation,
        source_group: child,
        duplicate_review_basis: evidence,
    };
    // Unknown raw time is retained. Unknown reviewed time cannot prove that this
    // physical source existed; only the independently evidenced235 permits posting.
    let mut unknown = reviewed.clone();
    unknown.facts.occurred = Occurrence::Unknown;
    let before_unknown = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(2301), unknown, at(250))
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::HubConflict
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        before_unknown
    );
    // Forward-qualified direct found source retains the exact original birth,
    // parent/debit and historical location; it is not a generic kind whitelist.
    let preview = db
        .hub_actual_departure_preview(owner(), operation(2301), reviewed.clone(), at(250))
        .await
        .unwrap();
    assert_eq!(
        preview.plan.historical_source.historical().operation,
        found.original.operation
    );
    let parent_before = db.hub_group(owner(), before.id).await.unwrap().group;
    let crate::HubMutationResult::ActualReviewed(actual) = db
        .submit_hub_mutation(
            scope,
            operation(2301),
            HubMutation::ActualReview(Box::new(reviewed)),
            at(250),
        )
        .await
        .unwrap()
    else {
        panic!("qualified direct found source")
    };
    assert_eq!(
        db.hub_group(owner(), before.id).await.unwrap().group,
        parent_before
    );
    assert_eq!(
        db.hub_group(owner(), child)
            .await
            .unwrap()
            .group
            .packs
            .value(),
        1
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(2300))
            .await
            .unwrap()
            .as_ref(),
        Some(raw.as_ref())
    );
    let first_input = source(&db, trace, 90, None).await;
    let crate::HubMutationResult::SourceAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(2400),
            HubMutation::SourceAttribution(Box::new(first_input)),
            at(300),
        )
        .await
        .unwrap()
    else {
        panic!("source before recipient")
    };
    let event = original_target;
    let recipient_input = command(&db, event, None, None).await;
    let crate::HubMutationResult::RecipientAttributed(recipient) = db
        .submit_hub_mutation(
            scope,
            operation(2500),
            HubMutation::RecipientAttribution(Box::new(recipient_input)),
            at(320),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let second_input = source(&db, trace, 91, Some(first.original.stamp.operation)).await;
    let crate::HubMutationResult::SourceAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(2600),
            HubMutation::SourceAttribution(Box::new(second_input)),
            at(340),
        )
        .await
        .unwrap()
    else {
        panic!("source after recipient")
    };
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(3)))
    );
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(event))
    );
    let known = db
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
    assert_eq!(known.departures.len(), 2);
    assert!(known.departures.iter().any(
        |r| r.original.operation == original_target && r.quantities[0].effective().value() == 0
    ));
    assert_eq!(
        db.hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(found.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2400))
            .await
            .unwrap()
            .as_ref(),
        Some(first.as_ref())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), target.clone())
            .await
            .unwrap(),
        Some(old_actual.clone())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(2301))
            .await
            .unwrap()
            .as_ref(),
        Some(actual.as_ref())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(found.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2400))
            .await
            .unwrap()
            .as_ref(),
        Some(first.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2600))
            .await
            .unwrap()
            .as_ref(),
        Some(second.as_ref())
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(2500))
            .await
            .unwrap()
            .as_ref(),
        Some(recipient.as_ref())
    );
    assert_eq!(
        db.hub_submission(owner(), operation(2301))
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::HubSubmissionStatus::Recorded
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(2300))
            .await
            .unwrap()
            .as_ref(),
        Some(raw.as_ref())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), target)
            .await
            .unwrap(),
        Some(old_actual)
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(2301))
            .await
            .unwrap()
            .as_ref(),
        Some(actual.as_ref())
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn explicit_receipt_scope_found_child_actual_then_source_recipient_source_preserves_every_original()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) =
        crate::hub::corrections::projection_native_tests::actual_debit(&db).await;
    let original_target = crate::hub::actuals::common::operation_id(&target).unwrap();
    let old_actual = db
        .hub_actual_review_operation(owner(), target.clone())
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(old_plan) = &old_actual.original else {
        panic!("old actual")
    };
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let found = crate::hub::corrections::additional_write_tests::found(
        &db,
        scope,
        &target,
        before.id,
        PhysicalDebitKey {
            operation: original_target,
            group: before.id,
        },
        2200,
        210,
    )
    .await;
    let child = found.original.allocations[0].group.id;
    let current = db.hub_group(owner(), child).await.unwrap();
    let trace = current.group.trace;
    let crate::HubMutationResult::ActualReported(raw) = db
        .submit_hub_mutation(
            scope,
            operation(2300),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(current.group.variant),
                recipient: Some(id(4)),
                packs: Some(WholePacks::from_value(1).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note(),
            })),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("raw found-child event")
    };
    let current = db.hub_group(owner(), child).await.unwrap();
    let mut facts = old_plan.comparison.facts.clone();
    facts.report = raw.original.id;
    facts.group = child;
    facts.expected_group_revision = current.group.revision;
    facts.expected_stock_revision = current.revision;
    facts.expected_catalog_revision = current.catalog_revision;
    facts.recipient = id(4);
    facts.packs = WholePacks::from_value(1).unwrap();
    facts.occurred = Occurrence::Known(at(235));
    let evidence = SourceProvenance::new(
        "Independently reviewed fictional actual departure at235 after found birth211",
    )
    .unwrap();
    facts.source_evidence = evidence.clone();
    facts.quantity_evidence = evidence.clone();
    facts.recipient_evidence = evidence.clone();
    facts.timing_evidence = evidence.clone();
    let reviewed = ActualNewDepartureCommand {
        facts,
        source_operation: crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
        source_group: before.id,
        duplicate_review_basis: evidence,
    };
    // Unknown raw time is retained. Unknown reviewed time cannot prove that this
    // physical source existed; only the independently evidenced235 permits posting.
    let mut unknown = reviewed.clone();
    unknown.facts.occurred = Occurrence::Unknown;
    let before_unknown = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(2301), unknown, at(250))
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::HubConflict
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        before_unknown
    );
    // This is an explicit original-receipt choice for the raw Unknown source,
    // not a translation of a found-only claim. Native captured scope/birth proof
    // must independently include the exact found child at235.
    let preview = db
        .hub_actual_departure_preview(owner(), operation(2301), reviewed.clone(), at(250))
        .await
        .unwrap();
    assert_eq!(
        preview.plan.historical_source.historical().operation,
        crate::hub::actuals::common::operation_id(&operation(1)).unwrap()
    );
    assert_eq!(preview.plan.reduction.before.id, child);
    assert_eq!(preview.plan.reduction.before.packs.value(), 2);
    assert_eq!(preview.plan.reduction.on_hand_after.value(), 1);
    let before_parent = db.hub_group(owner(), before.id).await.unwrap().group;
    let crate::HubMutationResult::ActualReviewed(actual) = db
        .submit_hub_mutation(
            scope,
            operation(2301),
            HubMutation::ActualReview(Box::new(reviewed.clone())),
            at(250),
        )
        .await
        .unwrap()
    else {
        panic!("explicit qualified receipt source")
    };
    assert_eq!(
        db.hub_group(owner(), before.id).await.unwrap().group,
        before_parent
    );
    assert_eq!(
        db.hub_group(owner(), child)
            .await
            .unwrap()
            .group
            .packs
            .value(),
        1
    );
    let first_input = source(&db, trace, 90, None).await;
    let crate::HubMutationResult::SourceAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(2400),
            HubMutation::SourceAttribution(Box::new(first_input)),
            at(300),
        )
        .await
        .unwrap()
    else {
        panic!("source before recipient")
    };
    let event = crate::hub::actuals::common::operation_id(&operation(2301)).unwrap();
    let recipient_input = command(&db, event, None, None).await;
    let crate::HubMutationResult::RecipientAttributed(recipient) = db
        .submit_hub_mutation(
            scope,
            operation(2500),
            HubMutation::RecipientAttribution(Box::new(recipient_input)),
            at(320),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let second_input = source(&db, trace, 91, Some(first.original.stamp.operation)).await;
    let crate::HubMutationResult::SourceAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(2600),
            HubMutation::SourceAttribution(Box::new(second_input)),
            at(340),
        )
        .await
        .unwrap()
    else {
        panic!("source after recipient")
    };
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(3)))
    );
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(4)))
    );
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(event))
    );
    let known = db
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
    assert_eq!(known.departures.len(), 2);
    assert!(known.departures.iter().any(
        |r| r.original.operation == original_target && r.quantities[0].effective().value() == 0
    ));
    let crate::HubMutationResult::ActualReviewed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(2301),
            HubMutation::ActualReview(Box::new(reviewed)),
            at(350),
        )
        .await
        .unwrap()
    else {
        panic!("old actual replay after recipient fence")
    };
    assert_eq!(replay, actual);
    assert_eq!(
        db.hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(found.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2400))
            .await
            .unwrap()
            .as_ref(),
        Some(first.as_ref())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), target.clone())
            .await
            .unwrap(),
        Some(old_actual.clone())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(2301))
            .await
            .unwrap()
            .as_ref(),
        Some(actual.as_ref())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(found.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2400))
            .await
            .unwrap()
            .as_ref(),
        Some(first.as_ref())
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(2600))
            .await
            .unwrap()
            .as_ref(),
        Some(second.as_ref())
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(2500))
            .await
            .unwrap()
            .as_ref(),
        Some(recipient.as_ref())
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(2300))
            .await
            .unwrap()
            .as_ref(),
        Some(raw.as_ref())
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), target)
            .await
            .unwrap(),
        Some(old_actual)
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(2301))
            .await
            .unwrap()
            .as_ref(),
        Some(actual.as_ref())
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn recipient_then_additional_and_found_keep_all_known_unknown_impacts_and_originals() {
    use harvestcircle_domain::hub::corrections::{CorrectionImpact, PhysicalDebitKey};
    let (fixture, db, mut additional) = crate::hub::corrections::additional_review_fixture().await;
    let target = crate::hub::actuals::common::operation(additional.facts.target_operation).unwrap();
    let balancing = additional.balancing_group;
    let trace = db.hub_group(owner(), balancing).await.unwrap().group.trace;
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let recipient_input = command(&db, additional.facts.target_operation, None, None).await;
    let crate::HubMutationResult::RecipientAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(recipient_input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let recipient_input = command(
        &db,
        additional.facts.target_operation,
        Some(id(4)),
        Some(first.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(1902),
            HubMutation::RecipientAttribution(Box::new(recipient_input)),
            at(201),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let target_id = additional.facts.target_operation;
    // The preserved raw corrected-total report explicitly says original party3.
    // It cannot silently become a report about current party4.
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    additional.facts.expected_stock_revision = page.revision;
    let before_invalid = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1899),
            HubMutation::AdditionalQuantity(Box::new(additional.clone())),
            at(202)
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_submission(owner(), operation(1899)).await.unwrap(),
        None
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        before_invalid
    );
    // An independent recipient review restores original party3; the intervening
    // party4 and Unknown history must still remain in all later impact sets.
    let input = command(
        &db,
        target_id,
        Some(id(3)),
        Some(second.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(third) = db
        .submit_hub_mutation(
            scope,
            operation(1903),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(203),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    additional.facts.expected_stock_revision = page.revision;
    let target_group = additional.facts.target_group;
    let crate::HubMutationResult::AdditionalCorrected(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::AdditionalQuantity(Box::new(additional)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("additional")
    };
    assert!(posted.impact.contains(CorrectionImpact::Recipient(id(3))));
    assert!(posted.impact.contains(CorrectionImpact::Recipient(id(4))));
    assert!(
        posted
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(target_id))
    );
    assert!(
        posted
            .impact
            .contains(CorrectionImpact::UnresolvedSource(trace))
    );
    let found = crate::hub::corrections::additional_write_tests::found(
        &db,
        scope,
        &target,
        target_group,
        PhysicalDebitKey {
            operation: posted.original.operation,
            group: balancing,
        },
        1950,
        220,
    )
    .await;
    assert!(found.impact.contains(CorrectionImpact::Recipient(id(3))));
    assert!(found.impact.contains(CorrectionImpact::Recipient(id(4))));
    assert!(
        found
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(target_id))
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*first.clone())
    );
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
    assert!(
        matches!(snapshot.current_recipient(target_id).unwrap(),AmendedRecipient::Known(ref p) if p.id==id(3))
    );
    assert_eq!(
        snapshot.recipient_corrections,
        vec![
            first.original.clone(),
            second.original.clone(),
            third.original.clone()
        ]
    );
    assert_eq!(
        db.hub_additional_quantity_original(owner(), operation(1901))
            .await
            .unwrap()
            .as_ref(),
        Some(posted.as_ref())
    );
    assert_eq!(
        db.hub_found_correction_original(owner(), found.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(found.as_ref())
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace,
                expected_revision: None,
                expected_catalog_revision: None
            }
        )
        .await
        .unwrap(),
        snapshot
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
