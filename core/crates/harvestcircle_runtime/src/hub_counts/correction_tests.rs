//! Actual actor admission and recovery, using the existing in-memory demo owner.
use super::super::super::hub_corrections::{HubCorrectionCommand, HubCorrectionWrite};
use super::*;
use harvestcircle_domain::hub::{
    corrections::SourceAttributionCommand,
    reference::{SourceCertainty, SourceCode, SourceReference, SourceReferenceKind},
};
use harvestcircle_storage::HubAttributionReviewOutcome;

#[tokio::test(flavor = "multi_thread")]
async fn recipient_additional_and_found_actor_preserve_original_event_and_separate_physical_effects()
 {
    qualify_physical_corrections().await;
}

fn qualify_physical_corrections() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
{
    Box::pin(async move {
        use harvestcircle_domain::hub::{
            actuals::{
                ActualNewDepartureCommand, ActualReportCommand, ActualReportSources,
                ActualReviewFacts,
            },
            corrections::{
                AdditionalQuantityCommand, CorrectedLineFacts, CorrectionObservationCommand,
                CorrectionObservedFacts, FoundCorrectionCommand, FoundStockInput, PhysicalDebitKey,
                RecipientAttributionCommand,
            },
        };
        use harvestcircle_storage::{
            HubAdditionalQuantityReviewOutcome, HubCorrectionFundingQuery, HubCorrectionKnownQuery,
            HubFoundCorrectionReviewOutcome, HubFoundCorrectionReviewQuery,
        };
        fn record(operation: &DurableRequestId) -> HubRecordId {
            HubRecordId::from_hex(&operation.as_str().replace('-', "")).unwrap()
        }
        let (actor, secrets, owner) = fixture().await;
        for n in [5, 6] {
            let catalog = actor.hub_references().await.unwrap();
            actor
                .apply_hub_reference(
                    DurableRequestId::new_v7(),
                    ReferenceCommand {
                        expected_catalog_revision: catalog.revision(),
                        mutation: ReferenceMutation::Create {
                            id: id(n),
                            value: ReferenceValue::Party {
                                label: HubLabel::new(format!("Fictional recipient {n}")).unwrap(),
                            },
                        },
                    },
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT,
                )
                .await
                .unwrap();
        }
        let catalog = actor.hub_references().await.unwrap();
        let intake_operation = DurableRequestId::new_v7();
        let intake = actor
            .receive_hub_stock(
                intake_operation.clone(),
                ReceiptCommand {
                    expected_stock_revision: HubRevision::INITIAL,
                    expected_catalog_revision: catalog.revision(),
                    variant: id(2),
                    supplier: None,
                    producer: None,
                    source: SourceAttribution::Unknown,
                    packs: packs(20),
                    receiving: id(3),
                    planned_location: Some(id(4)),
                    occurred: Occurrence::Known(now()),
                    observation: note("Twenty fictional crates"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let group = intake.original.position.group;
        let raw = actor
            .report_hub_actual(
                DurableRequestId::new_v7(),
                ActualReportCommand {
                    variant: Some(id(2)),
                    recipient: Some(id(5)),
                    packs: Some(packs(2)),
                    sources: ActualReportSources::Unknown,
                    occurred: Occurrence::Known(now()),
                    observation: note("Two already taken crates reported independently"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let current = actor.hub_group(group).await.unwrap();
        let target_operation = DurableRequestId::new_v7();
        let departed = actor
            .reconcile_hub_actual(
                target_operation.clone(),
                ActualNewDepartureCommand {
                    facts: ActualReviewFacts {
                        report: raw.original.id,
                        expected_stock_revision: current.revision,
                        expected_catalog_revision: current.catalog_revision,
                        group,
                        expected_group_revision: current.group.revision,
                        packs: packs(2),
                        recipient: id(5),
                        occurred: Occurrence::Known(now()),
                        source_evidence: note("Original intake cohort"),
                        quantity_evidence: note("Two whole crates"),
                        recipient_evidence: note("Explicit original recipient"),
                        timing_evidence: note("Current physical time"),
                    },
                    source_operation: record(&intake_operation),
                    source_group: group,
                    duplicate_review_basis: note("Complete native original departure comparison"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let before = actor.hub_stock(query()).await.unwrap();
        assert_eq!(before.rows[0].position.packs, packs(18));
        let recipient_operation = DurableRequestId::new_v7();
        let recipient_command = RecipientAttributionCommand {
            departure_operation: record(&target_operation),
            expected_stock_revision: before.revision,
            expected_catalog_revision: before.catalog_revision,
            expected_prior: None,
            recipient: Some(id(6)),
            basis: note("Whole event recipient independently corrected"),
        };
        let preview = actor
            .hub_recipient_attribution_review(
                recipient_operation.clone(),
                recipient_command.clone(),
            )
            .await
            .unwrap();
        let HubAttributionReviewOutcome::Candidate(recipient_plan) = preview.outcome else {
            panic!("native recipient comparison")
        };
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, recipient_operation.clone())
                .await
                .unwrap()
                .is_none()
        );
        let recipient = actor
            .post_hub_recipient_attribution(
                recipient_operation.clone(),
                recipient_command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        assert_eq!(recipient.original, *recipient_plan);
        assert_eq!(
            actor
                .post_hub_recipient_attribution(
                    recipient_operation.clone(),
                    recipient_command,
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap(),
            recipient
        );
        assert_eq!(actor.hub_stock(query()).await.unwrap().rows, before.rows);

        let corrected_report = actor
            .report_hub_actual(
                DurableRequestId::new_v7(),
                ActualReportCommand {
                    variant: Some(id(2)),
                    recipient: Some(id(6)),
                    packs: Some(packs(3)),
                    sources: ActualReportSources::Unknown,
                    occurred: Occurrence::Known(now()),
                    observation: note("Corrected total three, not three additional crates"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let current = actor.hub_group(group).await.unwrap();
        let additional_operation = DurableRequestId::new_v7();
        let additional_command = AdditionalQuantityCommand {
            facts: CorrectedLineFacts {
                report: corrected_report.original.id,
                target_operation: record(&target_operation),
                target_group: group,
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                expected_prior: None,
                corrected_line_packs: packs(3),
                recipient: Some(id(6)),
                occurred: Occurrence::Known(now()),
                source_evidence: note("Original physical cohort"),
                quantity_evidence: note("Original two plus one"),
                recipient_evidence: note("Current amended recipient"),
                timing_evidence: note("Original physical time"),
            },
            source_operation: record(&intake_operation),
            source_group: group,
            balancing_group: group,
            expected_balancing_revision: current.group.revision,
            duplicate_review_basis: note("Complete original event comparison"),
        };
        let credential = secrets.load(owner).await.unwrap();
        secrets
            .delete(&DurableRequestId::new_v7(), owner)
            .await
            .unwrap();
        let preview = actor
            .hub_additional_quantity_review(
                additional_operation.clone(),
                additional_command.clone(),
            )
            .await
            .unwrap();
        let HubAdditionalQuantityReviewOutcome::Candidate(additional_plan) = preview.outcome else {
            panic!("native additional comparison")
        };
        assert_eq!(
            actor
                .post_hub_additional_quantity(
                    additional_operation.clone(),
                    additional_command.clone(),
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::CredentialMissing
        );
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, additional_operation.clone())
                .await
                .unwrap()
                .is_none()
        );
        secrets
            .put(&DurableRequestId::new_v7(), owner, credential)
            .await
            .unwrap();
        let additional = actor
            .post_hub_additional_quantity(
                additional_operation.clone(),
                additional_command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        assert_eq!(additional.original, *additional_plan);
        assert_eq!(
            actor
                .post_hub_additional_quantity(
                    additional_operation.clone(),
                    additional_command,
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap(),
            additional
        );
        assert_eq!(
            actor.hub_stock(query()).await.unwrap().rows[0]
                .position
                .packs,
            packs(17)
        );

        let observed_operation = DurableRequestId::new_v7();
        let observed = actor.observe_hub_correction(observed_operation.clone(), CorrectionObservationCommand {
            previous: None, facts: CorrectionObservedFacts::Found {
                target_operation: record(&target_operation), target_group: group,
                allocations: vec![FoundStockInput { debit: PhysicalDebitKey { operation: record(&target_operation), group },
                    packs: packs(1), location: id(3), identification: PhysicalIdentification {
                        label: HubLabel::new("Independently found fictional crate").unwrap(), observation: note("Separate physical cohort"),
                    }, cohort_evidence: note("Found outside the counted remainder, never put back into its old group"),
                }], basis: note("One independently identified found crate"),
            },
        }, actor.snapshot().revision(), DEFAULT_COMMAND_TIMEOUT).await.unwrap();
        let before_found = actor.hub_stock(query()).await.unwrap();
        let found_operation = DurableRequestId::new_v7();
        let preview = actor
            .hub_found_correction_review(HubFoundCorrectionReviewQuery {
                observation_operation: observed_operation,
                proposed_operation: found_operation.clone(),
                expected_stock_revision: Some(before_found.revision),
                expected_catalog_revision: Some(before_found.catalog_revision),
            })
            .await
            .unwrap();
        let HubFoundCorrectionReviewOutcome::Candidate(found_plan) = preview.outcome else {
            panic!("native found comparison: {:?}", preview.outcome)
        };
        let found_command = FoundCorrectionCommand {
            observation_operation: observed.original.operation,
            expected_stock_revision: before_found.revision,
            expected_catalog_revision: before_found.catalog_revision,
        };
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, found_operation.clone())
                .await
                .unwrap()
                .is_none()
        );
        let found = actor
            .post_hub_found_correction(
                found_operation.clone(),
                found_command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        assert_eq!(found.original, *found_plan);
        assert_eq!(
            actor
                .post_hub_found_correction(
                    found_operation.clone(),
                    found_command,
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap(),
            found
        );
        let after = actor.hub_stock(query()).await.unwrap();
        assert_eq!(after.total, 2);
        assert_eq!(
            after
                .rows
                .iter()
                .find(|r| r.position.group == group)
                .unwrap()
                .position
                .packs,
            packs(17)
        );
        let found_row = after
            .rows
            .iter()
            .find(|r| r.position.group != group)
            .unwrap();
        assert_eq!(found_row.position.packs, packs(1));
        secrets
            .delete(&DurableRequestId::new_v7(), owner)
            .await
            .unwrap();
        assert_eq!(
            actor
                .hub_found_correction_original(found_operation.clone())
                .await
                .unwrap(),
            Some(found)
        );
        assert_eq!(
            actor
                .hub_additional_quantity_original(additional_operation.clone())
                .await
                .unwrap(),
            Some(additional)
        );
        assert_eq!(
            actor
                .hub_recipient_attribution_original(recipient_operation.clone())
                .await
                .unwrap(),
            Some(recipient)
        );
        assert_eq!(
            actor
                .hub_actual_review_operation(target_operation.clone())
                .await
                .unwrap(),
            Some(departed)
        );
        assert_eq!(
            actor.hub_receipt_operation(intake_operation).await.unwrap(),
            Some(intake)
        );
        let funding = actor
            .hub_correction_target_funding(HubCorrectionFundingQuery {
                target_operation: target_operation.clone(),
                original_group: group,
                expected_stock_revision: Some(after.revision),
                expected_catalog_revision: Some(after.catalog_revision),
            })
            .await
            .unwrap();
        assert_eq!(funding.quantity.effective(), packs(2));
        let known = actor
            .hub_correction_known_departures(HubCorrectionKnownQuery {
                trace: after
                    .rows
                    .iter()
                    .find(|r| r.position.group == group)
                    .unwrap()
                    .position
                    .trace,
                expected_stock_revision: Some(after.revision),
                expected_catalog_revision: Some(after.catalog_revision),
            })
            .await
            .unwrap();
        assert_eq!(known.revision, after.revision);
        let original_event = known
            .departures
            .iter()
            .find(|entry| entry.original.operation == record(&target_operation))
            .expect("complete original departure remains in current known evidence");
        assert_eq!(original_event.original.recipient.id, id(5));
        assert_eq!(original_event.original.packs().unwrap(), packs(2));
        assert_eq!(original_event.recipient_history.len(), 1);
        assert_eq!(original_event.quantities[0].effective(), packs(2));
        assert_eq!(actor.hub_stock(query()).await.unwrap(), after);
        actor.sign_out().await.unwrap();
        assert_eq!(
            actor
                .hub_found_correction_original(found_operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(
            actor
                .hub_additional_quantity_original(additional_operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(
            actor
                .hub_recipient_attribution_original(recipient_operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied
        );
        actor.close().await.unwrap();
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn count_explanation_and_zero_classification_actor_keep_physical_count_and_originals() {
    qualify_count_actor(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn count_return_association_actor_keeps_restricted_custody_and_original_return() {
    qualify_count_actor(true).await;
}

fn qualify_count_actor(
    associate_return: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        use harvestcircle_domain::hub::{
            actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
            corrections::{
                CorrectionObservationCommand, CorrectionObservedFacts, CountClassificationCommand,
                CountExplanationCommand, CountExplanationFacts,
            },
        };
        use harvestcircle_storage::{
            HubCountClassificationReviewOutcome, HubCountExplanationReviewOutcome,
        };
        let (actor, secrets, owner) = fixture().await;
        let catalog = actor.hub_references().await.unwrap();
        actor
            .apply_hub_reference(
                DurableRequestId::new_v7(),
                ReferenceCommand {
                    expected_catalog_revision: catalog.revision(),
                    mutation: ReferenceMutation::Create {
                        id: id(5),
                        value: ReferenceValue::Party {
                            label: HubLabel::new("Fictional recipient").unwrap(),
                        },
                    },
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let catalog = actor.hub_references().await.unwrap();
        actor
            .apply_hub_reference(
                DurableRequestId::new_v7(),
                ReferenceCommand {
                    expected_catalog_revision: catalog.revision(),
                    mutation: ReferenceMutation::Create {
                        id: id(6),
                        value: ReferenceValue::Location {
                            label: HubLabel::new("Fictional return inspection").unwrap(),
                            purpose: LocationPurpose::Other,
                        },
                    },
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let catalog = actor.hub_references().await.unwrap();
        let intake_operation = DurableRequestId::new_v7();
        let intake = actor
            .receive_hub_stock(
                intake_operation.clone(),
                ReceiptCommand {
                    expected_stock_revision: HubRevision::INITIAL,
                    expected_catalog_revision: catalog.revision(),
                    variant: id(2),
                    supplier: None,
                    producer: None,
                    source: SourceAttribution::Unknown,
                    packs: packs(20),
                    receiving: id(3),
                    planned_location: Some(id(4)),
                    occurred: Occurrence::Known(UnixTimestamp::from_seconds(10).unwrap()),
                    observation: note("Twenty fictional crates"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let page = actor.hub_stock(query()).await.unwrap();
        let observed_count = actor
            .observe_hub_count(
                DurableRequestId::new_v7(),
                CountObserveCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: page.catalog_revision,
                    group: intake.original.position.group,
                    expected_group_revision: page.rows[0].position.revision,
                    counted_packs: packs(18),
                    cutoff: Occurrence::Known(now()),
                    observation: note("Eighteen counted independently"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let count_review = actor
            .hub_count_review(observed_count.original.id)
            .await
            .unwrap();
        assert!(count_review.assessment.ready());
        let count_operation = DurableRequestId::new_v7();
        let counted = actor
            .post_hub_count(
                count_operation.clone(),
                CountPostCommand {
                    expected_stock_revision: count_review.stock_revision,
                    expected_catalog_revision: count_review.catalog_revision,
                    count: observed_count.original.id,
                    reviewed_activity: true,
                    surplus_identification: None,
                    reason: note("Physical count reviewed"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let report_operation = DurableRequestId::new_v7();
        let report = actor
            .report_hub_actual(
                report_operation.clone(),
                ActualReportCommand {
                    variant: Some(id(2)),
                    recipient: Some(id(5)),
                    packs: Some(packs(1)),
                    sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                        trace: intake.original.trace.id,
                        group: Some(intake.original.position.group),
                        location: Some(id(3)),
                    }),
                    occurred: Occurrence::Known(UnixTimestamp::from_seconds(20).unwrap()),
                    observation: note("One fictional crate reported as taken before the count"),
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        let page = actor.hub_stock(query()).await.unwrap();
        assert_eq!(page.rows[0].position.packs, packs(18));
        let explanation_operation = DurableRequestId::new_v7();
        let explain = CountExplanationCommand {
            facts: CountExplanationFacts {
                report: report.original.id,
                count_reconciliation: counted.original.id,
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                expected_prior: None,
                packs: packs(1),
                recipient: id(5),
                occurred: Occurrence::Known(UnixTimestamp::from_seconds(20).unwrap()),
                source_evidence: note("Historical original intake"),
                quantity_evidence: note("One already missing crate"),
                recipient_evidence: note("Explicit fictional recipient"),
                timing_evidence: note("Before the counted cutoff"),
            },
            source_operation: HubRecordId::from_hex(&intake_operation.as_str().replace('-', ""))
                .unwrap(),
            source_group: intake.original.position.group,
        };
        let review = actor
            .hub_count_explanation_review(explanation_operation.clone(), explain.clone())
            .await
            .unwrap();
        let HubCountExplanationReviewOutcome::Candidate(plan) = review.outcome else {
            panic!("native explanation preview")
        };
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, explanation_operation.clone())
                .await
                .unwrap()
                .is_none()
        );
        let explained = actor
            .explain_hub_count_loss(
                explanation_operation.clone(),
                explain.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        assert_eq!(explained.original, *plan);
        assert_eq!(
            actor
                .explain_hub_count_loss(
                    explanation_operation.clone(),
                    explain,
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap(),
            explained
        );
        assert_eq!(actor.hub_stock(query()).await.unwrap().rows, page.rows);
        if associate_return {
            use harvestcircle_domain::hub::{
                corrections::CountReturnAssociationCommand, returns::ReturnCommand,
            };
            use harvestcircle_storage::HubCountReturnAssociationReviewOutcome;
            let current = actor.hub_stock(query()).await.unwrap();
            let returned_operation = DurableRequestId::new_v7();
            let returned = actor
                .apply_hub_return(
                    returned_operation.clone(),
                    ReturnCommand {
                        expected_stock_revision: current.revision,
                        expected_catalog_revision: current.catalog_revision,
                        variant: id(2),
                        location: id(6),
                        returned_by: None,
                        source: SourceAttribution::Unknown,
                        packs: packs(1),
                        identification: PhysicalIdentification {
                            label: HubLabel::new(
                                "One independently counted fictional returned crate",
                            )
                            .unwrap(),
                            observation: note("Separate restricted returned custody"),
                        },
                        planned_location: None,
                        departure: None,
                        occurred: Occurrence::Known(UnixTimestamp::from_seconds(40).unwrap()),
                        observation: note("Returned after the reported earlier departure"),
                    },
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT,
                )
                .await
                .unwrap();
            let before = actor.hub_stock(query()).await.unwrap();
            let operation = DurableRequestId::new_v7();
            let command = CountReturnAssociationCommand {
                return_operation: HubRecordId::from_hex(
                    &returned_operation.as_str().replace('-', ""),
                )
                .unwrap(),
                explanation_operation: explained.original.operation,
                expected_stock_revision: before.revision,
                expected_catalog_revision: before.catalog_revision,
                expected_account_prior: Some(explained.original.operation),
                expected_returns_prior: None,
                source_operation: HubRecordId::from_hex(
                    &intake_operation.as_str().replace('-', ""),
                )
                .unwrap(),
                source_group: intake.original.position.group,
                source_evidence: note("Original intake explicitly selected"),
                quantity_evidence: note("One complete returned crate, not another physical credit"),
                timing_evidence: note("Return after established departure"),
            };
            let credential = secrets.load(owner).await.unwrap();
            secrets
                .delete(&DurableRequestId::new_v7(), owner)
                .await
                .unwrap();
            let preview = actor
                .hub_count_return_association_review(operation.clone(), command.clone())
                .await
                .unwrap();
            let HubCountReturnAssociationReviewOutcome::Candidate(plan) = preview.outcome else {
                panic!("native association comparison")
            };
            assert_eq!(
                actor
                    .associate_hub_count_return(
                        operation.clone(),
                        command.clone(),
                        actor.snapshot().revision(),
                        DEFAULT_COMMAND_TIMEOUT
                    )
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::CredentialMissing
            );
            assert!(
                actor
                    ._adapter
                    .database()
                    .hub_submission(owner, operation.clone())
                    .await
                    .unwrap()
                    .is_none()
            );
            secrets
                .put(&DurableRequestId::new_v7(), owner, credential)
                .await
                .unwrap();
            let associated = actor
                .associate_hub_count_return(
                    operation.clone(),
                    command.clone(),
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT,
                )
                .await
                .unwrap();
            assert_eq!(associated.original, *plan);
            assert_eq!(
                actor
                    .associate_hub_count_return(
                        operation.clone(),
                        command,
                        actor.snapshot().revision(),
                        DEFAULT_COMMAND_TIMEOUT
                    )
                    .await
                    .unwrap(),
                associated
            );
            secrets
                .delete(&DurableRequestId::new_v7(), owner)
                .await
                .unwrap();
            assert_eq!(actor.hub_stock(query()).await.unwrap().rows, before.rows);
            assert_eq!(
                actor
                    .hub_return_operation(returned_operation)
                    .await
                    .unwrap(),
                Some(returned)
            );
            assert_eq!(
                actor
                    .hub_count_return_association_operation(operation.clone())
                    .await
                    .unwrap(),
                Some(associated)
            );
            assert_eq!(
                actor
                    .hub_count_explanation_operation(explanation_operation)
                    .await
                    .unwrap(),
                Some(explained)
            );
            assert_eq!(
                actor
                    .hub_count_post_operation(count_operation)
                    .await
                    .unwrap(),
                Some(counted)
            );
            assert_eq!(
                actor.hub_receipt_operation(intake_operation).await.unwrap(),
                Some(intake)
            );
            actor.sign_out().await.unwrap();
            assert_eq!(
                actor
                    .hub_count_return_association_operation(operation)
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::HubPermissionDenied
            );
            actor.close().await.unwrap();
            return;
        }
        let observation_operation = DurableRequestId::new_v7();
        let observation = actor.observe_hub_correction(observation_operation.clone(), CorrectionObservationCommand {
        previous: None, facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: explained.original.operation, corrected_packs: packs(0), report: None,
            basis: note("Original explanation later disputed; preserve zero without a fabricated departure report"),
        },
    }, actor.snapshot().revision(), DEFAULT_COMMAND_TIMEOUT).await.unwrap();
        let page = actor.hub_stock(query()).await.unwrap();
        let classification_operation = DurableRequestId::new_v7();
        let classify = CountClassificationCommand {
            observation_operation: observation.original.operation,
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            expected_account_prior: Some(explained.original.operation),
            expected_prior: None,
        };
        let credential = secrets.load(owner).await.unwrap();
        secrets
            .delete(&DurableRequestId::new_v7(), owner)
            .await
            .unwrap();
        let review = actor
            .hub_count_classification_review(classification_operation.clone(), classify.clone())
            .await
            .unwrap();
        let HubCountClassificationReviewOutcome::Candidate(plan) = review.outcome else {
            panic!("native zero comparison")
        };
        assert_eq!(
            actor
                .classify_hub_count_loss(
                    classification_operation.clone(),
                    classify.clone(),
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::CredentialMissing
        );
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, classification_operation.clone())
                .await
                .unwrap()
                .is_none()
        );
        secrets
            .put(&DurableRequestId::new_v7(), owner, credential)
            .await
            .unwrap();
        let classified = actor
            .classify_hub_count_loss(
                classification_operation.clone(),
                classify.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
        assert_eq!(classified.original, *plan);
        assert_eq!(
            actor
                .classify_hub_count_loss(
                    classification_operation.clone(),
                    classify,
                    actor.snapshot().revision(),
                    DEFAULT_COMMAND_TIMEOUT
                )
                .await
                .unwrap(),
            classified
        );
        secrets
            .delete(&DurableRequestId::new_v7(), owner)
            .await
            .unwrap();
        assert_eq!(
            actor
                .hub_count_classification_operation(classification_operation.clone())
                .await
                .unwrap(),
            Some(classified)
        );
        assert_eq!(
            actor
                .hub_count_explanation_operation(explanation_operation.clone())
                .await
                .unwrap(),
            Some(explained)
        );
        assert_eq!(
            actor
                .hub_count_post_operation(count_operation)
                .await
                .unwrap(),
            Some(counted)
        );
        assert_eq!(
            actor.hub_receipt_operation(intake_operation).await.unwrap(),
            Some(intake)
        );
        assert_eq!(actor.hub_stock(query()).await.unwrap().rows, page.rows);
        actor.sign_out().await.unwrap();
        assert_eq!(
            actor
                .hub_count_classification_operation(classification_operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(
            actor
                .hub_count_explanation_operation(explanation_operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied
        );
        actor.close().await.unwrap();
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_correction_observation_retains_unverified_claim_without_stock_effect_or_custody_bypass()
 {
    use harvestcircle_domain::hub::corrections::{
        CorrectionObservationCommand, CorrectionObservedFacts,
    };

    let (actor, secrets, owner) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let intake_operation = DurableRequestId::new_v7();
    let intake = actor
        .receive_hub_stock(
            intake_operation.clone(),
            ReceiptCommand {
                expected_stock_revision: HubRevision::INITIAL,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                supplier: None,
                producer: None,
                source: SourceAttribution::Unknown,
                packs: packs(20),
                receiving: id(3),
                planned_location: Some(id(4)),
                occurred: Occurrence::Unknown,
                observation: note(
                    "Twenty fictional crates remain separate from an unverified claim",
                ),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let before = actor.hub_stock(query()).await.unwrap();
    assert_eq!(before.rows[0].position.packs, packs(20));
    let operation = DurableRequestId::new_v7();
    // The claim may name an unknown explanation. Recording the claim does not
    // authenticate that target or grant permission to classify any Count loss.
    let command = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: id(99),
            corrected_packs: packs(0),
            report: None,
            basis: note("Fictional unverified claim, explicitly not an established count event"),
        },
    };
    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor
            .observe_hub_correction(
                operation.clone(),
                command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing,
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, operation.clone())
            .await
            .unwrap()
            .is_none()
    );
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();
    let recorded = actor
        .observe_hub_correction(
            operation.clone(),
            command.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(recorded.original.observation, command);
    let recorded_page = actor.hub_stock(query()).await.unwrap();
    assert_eq!(recorded_page.rows, before.rows);
    assert_eq!(recorded_page.catalog_revision, before.catalog_revision);
    assert_eq!(recorded_page.revision, recorded.original.stock_revision);
    assert_eq!(recorded_page.revision.value(), before.revision.value() + 1);
    assert_eq!(
        actor
            .observe_hub_correction(
                operation.clone(),
                command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap(),
        recorded,
    );
    let mut conflicting = command;
    let CorrectionObservedFacts::CountClassification {
        corrected_packs, ..
    } = &mut conflicting.facts
    else {
        unreachable!()
    };
    *corrected_packs = packs(1);
    assert_eq!(
        actor
            .observe_hub_correction(
                operation.clone(),
                conflicting,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict,
    );
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor
            .hub_correction_observation_original(operation.clone())
            .await
            .unwrap(),
        recorded
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap(), recorded_page);
    assert_eq!(
        actor.hub_receipt_operation(intake_operation).await.unwrap(),
        Some(intake)
    );
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_correction_observation_original(operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    actor.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn source_correction_actor_preserves_originals_and_rechecks_custody_generation_and_deadline()
{
    let (actor, secrets, owner) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let intake_operation = DurableRequestId::new_v7();
    let intake = actor
        .receive_hub_stock(
            intake_operation.clone(),
            ReceiptCommand {
                expected_stock_revision: HubRevision::INITIAL,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                supplier: None,
                producer: None,
                source: SourceAttribution::Unknown,
                packs: packs(20),
                receiving: id(3),
                planned_location: Some(id(4)),
                occurred: Occurrence::Unknown,
                observation: note("Twenty independently counted fictional crates"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let before = actor.hub_stock(query()).await.unwrap();
    let command = SourceAttributionCommand {
        trace: intake.original.trace.id,
        expected_stock_revision: before.revision,
        expected_catalog_revision: before.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(60),
                issuer: None,
                kind: SourceReferenceKind::SupplierReference,
                code: SourceCode::new("FICTIONAL-SOURCE-60").unwrap(),
                provenance: note("Independent fictional whole-population provenance"),
            },
            certainty: SourceCertainty::Established,
        },
        basis: note("Explicit original and current source comparison; no inventory change"),
    };

    let expired = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .post_hub_source_attribution(
                expired.clone(),
                command.clone(),
                actor.snapshot().revision(),
                Duration::ZERO,
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    // Absence belongs only to this forced pre-execution fixture. It grants no
    // replay permission after real lost replies, restoration or unknown intent.
    assert!(
        actor
            .hub_source_attribution_original(expired.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, expired)
            .await
            .unwrap()
            .is_none()
    );

    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    let missing = DurableRequestId::new_v7();
    assert!(matches!(
        actor
            .hub_source_attribution_review(missing.clone(), command.clone(),)
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::Candidate(_)
    ));
    assert_eq!(
        actor
            .post_hub_source_attribution(
                missing.clone(),
                command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, missing)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap(), before);
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();

    let stale_generation = actor.session_generation();
    actor.sign_out().await.unwrap();
    actor.activate_identity(owner).await.unwrap();
    let stale = DurableRequestId::new_v7();
    let result = actor
        .dispatch_durable(
            RuntimeCommand::HubCorrection {
                generation: stale_generation,
                command: HubCorrectionCommand::Write {
                    operation: stale.clone(),
                    command: HubCorrectionWrite::Source(Box::new(command.clone())),
                },
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await;
    assert_eq!(
        result.err().unwrap().code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, stale)
            .await
            .unwrap()
            .is_none()
    );

    let operation = DurableRequestId::new_v7();
    let preview = actor
        .hub_source_attribution_review(operation.clone(), command.clone())
        .await
        .unwrap();
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, operation.clone())
            .await
            .unwrap()
            .is_none()
    );
    let HubAttributionReviewOutcome::Candidate(plan) = preview.outcome else {
        panic!("read-only source comparison through the native actor")
    };
    let posted = actor
        .post_hub_source_attribution(
            operation.clone(),
            command.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(posted.original, *plan);
    assert_eq!(posted.original.before.source, SourceAttribution::Unknown);
    assert_eq!(posted.original.after.source, command.source);
    assert_eq!(
        actor
            .post_hub_source_attribution(
                operation.clone(),
                command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap(),
        posted
    );
    let mut conflicting = command.clone();
    conflicting.source = SourceAttribution::Unknown;
    assert_eq!(
        actor
            .post_hub_source_attribution(
                operation.clone(),
                conflicting,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap().rows, before.rows);
    assert_eq!(
        actor.hub_receipt_operation(intake_operation).await.unwrap(),
        Some(intake)
    );
    assert_eq!(
        actor
            .hub_source_attribution_original(operation.clone())
            .await
            .unwrap(),
        Some(posted.clone())
    );

    // Original reads remain available without custody material; they do not
    // reconstruct a grant to submit another source correction.
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor
            .hub_source_attribution_original(operation.clone())
            .await
            .unwrap(),
        Some(posted)
    );
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_source_attribution_original(operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    actor.close().await.unwrap();
}
