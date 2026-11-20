//! Ordinary native count history; no retained historical fixture is opened.
use super::*;
use harvestcircle_application::hub_submission::HubMutation;
use harvestcircle_domain::hub::{
    corrections::{
        CorrectedDepartureFamily, CorrectionImpact, CorrectionObservationCommand,
        CorrectionObservedFacts,
    },
    reference::WholePacks,
};
use std::{future::Future, pin::Pin};

#[tokio::test]
async fn count_explanation_and_zero_classification_reviews_match_fresh_writes_and_cold_originals() {
    qualify_count_reviews().await;
}

// Keep the ordinary writer and full-original futures outside the test caller's
// concrete frame; retain the default test thread stack.
fn qualify_count_reviews() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        for zero in [false, true] {
            let (fixture, db, scope, explanation) =
                crate::hub::corrections::count_review::tests::explained_world().await;
            if zero {
                crate::hub::corrections::count_return_write::tests::native22::classification(
                    &db,
                    scope,
                    explanation.original.operation,
                    (explanation.original.operation, None),
                    0,
                    810,
                    210,
                )
                .await
                .unwrap();
            }
            let trace = explanation.original.known_event.trace;
            let source_input = source_command(
                &db,
                trace,
                source(id(60), Some(id(3)), SourceCertainty::Established),
            )
            .await;
            let recipient_input = recipient_command(&db, &operation(801), Some(id(4))).await;
            let before = crate::hub::custody::tests::material(&fixture).await;
            let source_review = db
                .hub_source_attribution_review(
                    owner(),
                    operation(1950),
                    source_input.clone(),
                    at(300),
                )
                .await
                .unwrap();
            let recipient_review = db
                .hub_recipient_attribution_review(
                    owner(),
                    operation(1951),
                    recipient_input,
                    at(300),
                )
                .await
                .unwrap();
            assert!(!source_review.uncertain_intent);
            assert!(!recipient_review.uncertain_intent);
            assert_eq!(source_review.affected.len(), 1);
            let affected = &source_review.affected[0];
            assert_eq!(affected.operation, explanation.original.operation);
            assert_eq!(affected.populations, vec![trace]);
            assert!(affected.issued_versions.is_empty());
            assert!(affected.recipients.iter().any(
                |recipient| matches!(recipient, AmendedRecipient::Known(party) if party.id == id(5))
            ));
            assert_eq!(
                recipient_review.original.family,
                CorrectedDepartureFamily::ExplainedCountLoss
            );
            assert_eq!(recipient_review.original.recipient.id, id(5));
            assert_eq!(recipient_review.original.populations, vec![trace]);
            assert!(recipient_review.linked_issued_versions.is_empty());
            let HubAttributionReviewOutcome::Candidate(source_plan) = source_review.outcome else {
                panic!("count source review must produce a checked candidate")
            };
            let HubAttributionReviewOutcome::Candidate(recipient_plan) = recipient_review.outcome
            else {
                panic!("count recipient review must produce a checked candidate")
            };
            for impact in [
                CorrectionImpact::Population(trace),
                CorrectionImpact::Departure(explanation.original.operation),
                CorrectionImpact::Recipient(id(5)),
                CorrectionImpact::Recipient(id(4)),
            ] {
                assert!(recipient_plan.impact.contains(impact));
            }
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
            let stock_before = db.hub_stock_page(owner(), query()).await.unwrap();
            let crate::HubMutationResult::SourceAttributed(source_posted) = db
                .submit_hub_mutation(
                    scope,
                    operation(1950),
                    HubMutation::SourceAttribution(Box::new(source_input)),
                    at(300),
                )
                .await
                .unwrap()
            else {
                panic!("ordinary count-aware source write")
            };
            assert_eq!(source_posted.original, *source_plan);
            // The source write advances the cut. Obtain a fresh recipient review,
            // rather than treating the earlier candidate as execution authority.
            let recipient_input = recipient_command(&db, &operation(801), Some(id(4))).await;
            let current = db
                .hub_recipient_attribution_review(
                    owner(),
                    operation(1951),
                    recipient_input.clone(),
                    at(310),
                )
                .await
                .unwrap();
            let HubAttributionReviewOutcome::Candidate(current_plan) = current.outcome else {
                panic!("fresh recipient comparison after source write")
            };
            let crate::HubMutationResult::RecipientAttributed(recipient_posted) = db
                .submit_hub_mutation(
                    scope,
                    operation(1951),
                    HubMutation::RecipientAttribution(Box::new(recipient_input)),
                    at(310),
                )
                .await
                .unwrap()
            else {
                panic!("ordinary count-aware recipient write")
            };
            assert_eq!(recipient_posted.original, *current_plan);
            assert_eq!(
                db.hub_stock_page(owner(), query()).await.unwrap().rows,
                stock_before.rows
            );
            assert_eq!(
                db.hub_count_explanation_operation(owner(), operation(801))
                    .await
                    .unwrap(),
                Some(*explanation.clone()),
            );
            let after = crate::hub::custody::tests::material(&fixture).await;
            db.close().await.unwrap();
            let db = fixture.open().await;
            assert_eq!(
                db.hub_source_attribution_original(owner(), operation(1950))
                    .await
                    .unwrap(),
                Some(*source_posted),
            );
            assert_eq!(
                db.hub_recipient_attribution_original(owner(), operation(1951))
                    .await
                    .unwrap(),
                Some(*recipient_posted),
            );
            assert_eq!(
                db.hub_count_explanation_operation(owner(), operation(801))
                    .await
                    .unwrap(),
                Some(*explanation),
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, after);
            db.close().await.unwrap();
        }
    })
}

#[tokio::test]
async fn count_reviews_reject_foreign_stale_and_non_explanation_targets_without_admission() {
    qualify_count_review_faults().await;
}

fn qualify_count_review_faults() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let (fixture, db, scope, explanation) =
            crate::hub::corrections::count_review::tests::explained_world().await;
        let classified =
            crate::hub::corrections::count_return_write::tests::native22::classification(
                &db,
                scope,
                explanation.original.operation,
                (explanation.original.operation, None),
                0,
                810,
                210,
            )
            .await
            .unwrap();
        let input = recipient_command(&db, &operation(801), Some(id(4))).await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_recipient_attribution_review(other(), operation(1950), input.clone(), at(300))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubPermissionDenied,
        );
        let mut stale = input.clone();
        stale.expected_stock_revision = explanation.original.explanation.stock_revision;
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1950), stale, at(300))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubConflict,
        );
        let mut stale_catalog = input.clone();
        stale_catalog.expected_catalog_revision = HubRevision::from_value(
            input
                .expected_catalog_revision
                .value()
                .checked_add(1)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1950), stale_catalog, at(300))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubConflict,
        );
        let mut wrong_prior = input.clone();
        wrong_prior.expected_prior = Some(id(60));
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1950), wrong_prior, at(300))
                .await
                .unwrap()
                .outcome,
            HubAttributionReviewOutcome::Blocked(AttributionError::StaleRevision),
        );
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(801), input.clone(), at(300))
                .await
                .unwrap()
                .outcome,
            HubAttributionReviewOutcome::OperationAlreadyUsed,
        );
        for target in [
            classified.original.amendment.operation,
            crate::hub::actuals::common::operation_id(&operation(810)).unwrap(),
        ] {
            let mut wrong = input.clone();
            wrong.departure_operation = target;
            assert_eq!(
                db.hub_recipient_attribution_review(owner(), operation(1950), wrong, at(300))
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::HubInvalidInput,
            );
        }
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        // A valid independently reported fact is admitted normally. Its execution
        // fails the monotonic clock check and retains that exact pending intent.
        // This is not a fabricated Pending row or a request to replay saved input.
        let error = db
            .submit_hub_mutation(
                scope,
                operation(1960),
                HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                    previous: None,
                    facts: CorrectionObservedFacts::CountClassification {
                        explanation_operation: explanation.original.operation,
                        corrected_packs: WholePacks::from_value(1).unwrap(),
                        report: None,
                        basis: note("Independent fact retained after a backwards execution clock"),
                    },
                })),
                at(199),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), SafeErrorCode::HubOutcomeUnknown);
        assert_eq!(
            db.hub_submission(owner(), operation(1960))
                .await
                .unwrap()
                .unwrap()
                .status,
            crate::HubSubmissionStatus::Pending,
        );
        let pending = crate::hub::custody::tests::material(&fixture).await;
        let blocked = db
            .hub_recipient_attribution_review(owner(), operation(1950), input.clone(), at(300))
            .await
            .unwrap();
        assert!(blocked.uncertain_intent);
        assert!(!blocked.unresolved_actual);
        assert_eq!(
            blocked.outcome,
            HubAttributionReviewOutcome::Blocked(AttributionError::UncertainSubmission)
        );
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1960), input, at(300))
                .await
                .unwrap()
                .outcome,
            HubAttributionReviewOutcome::OperationAlreadyUsed,
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            pending
        );
        db.close().await.unwrap();
    })
}

#[tokio::test]
async fn count_reviews_keep_zero_events_and_established_return_scope_without_rewriting_custody() {
    qualify_count_return_reviews().await;
}

fn qualify_count_return_reviews() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let world =
            crate::hub::corrections::count_return_write::tests::native22::association_world().await;
        let source_input = source_command(
            &world.db,
            world.trace,
            source(id(60), Some(id(3)), SourceCertainty::Established),
        )
        .await;
        let recipient_input = recipient_command(&world.db, &operation(905), Some(id(4))).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        let sources = world
            .db
            .hub_source_attribution_review(owner(), operation(1950), source_input, at(300))
            .await
            .unwrap();
        let recipients = world
            .db
            .hub_recipient_attribution_review(
                owner(),
                operation(1951),
                recipient_input.clone(),
                at(300),
            )
            .await
            .unwrap();
        let expected = [801, 905]
            .map(|number| crate::hub::actuals::common::operation_id(&operation(number)).unwrap());
        assert_eq!(sources.affected.len(), 2);
        for event in expected {
            let affected = sources
                .affected
                .iter()
                .find(|row| row.operation == event)
                .unwrap();
            assert_eq!(affected.populations, vec![world.trace]);
            assert!(affected.issued_versions.is_empty());
            assert!(affected.recipients.iter().any(
                |recipient| matches!(recipient, AmendedRecipient::Known(party) if party.id == id(5))
            ));
        }
        assert!(matches!(
            sources.outcome,
            HubAttributionReviewOutcome::Candidate(_)
        ));
        let HubAttributionReviewOutcome::Candidate(plan) = recipients.outcome else {
            panic!("established Return retains the CountE recipient comparison")
        };
        assert_eq!(
            recipients.original.family,
            CorrectedDepartureFamily::ExplainedCountLoss
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        let crate::HubMutationResult::RecipientAttributed(posted) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(1951),
                HubMutation::RecipientAttribution(Box::new(recipient_input)),
                at(300),
            )
            .await
            .unwrap()
        else {
            panic!("fresh native recipient attribution after established Return")
        };
        assert_eq!(posted.original, *plan);
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(900))
                .await
                .unwrap(),
            Some(*world.returned.clone()),
        );
        assert_eq!(
            world
                .db
                .hub_count_return_association_operation(owner(), operation(906))
                .await
                .unwrap(),
            Some(*world.associated.clone()),
        );
        let after = crate::hub::custody::tests::material(&world.fixture).await;
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_eq!(
            db.hub_recipient_attribution_original(owner(), operation(1951))
                .await
                .unwrap(),
            Some(*posted),
        );
        assert_eq!(
            db.hub_count_return_association_operation(owner(), operation(906))
                .await
                .unwrap(),
            Some(*world.associated),
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            after
        );
        db.close().await.unwrap();
    })
}

#[tokio::test]
async fn count_attribution_reviews_and_fresh_write_reject_missing_complete_proofs_without_repair() {
    qualify_damaged_count_reviews().await;
}

fn qualify_damaged_count_reviews() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        for (table, number) in [
            ("hub_correction_count_explanations", 801),
            ("hub_correction_quantity_changes", 811),
            ("hub_submissions", 801),
        ] {
            let (fixture, db, scope, explanation) =
                crate::hub::corrections::count_review::tests::explained_world().await;
            crate::hub::corrections::count_return_write::tests::native22::classification(
                &db,
                scope,
                explanation.original.operation,
                (explanation.original.operation, None),
                0,
                810,
                210,
            )
            .await
            .unwrap();
            let source_input = source_command(
                &db,
                explanation.original.known_event.trace,
                source(id(60), Some(id(3)), SourceCertainty::Established),
            )
            .await;
            let recipient_input = recipient_command(&db, &operation(801), Some(id(4))).await;
            // This raw connection belongs solely to this fresh test fixture.
            // Restore exact immutable trigger SQL before every product call.
            let mut raw = fixture.raw().await;
            let original_rows: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT count(*) FROM \"{table}\" WHERE operation_id=?",
            )))
            .bind(operation(number).as_str())
            .fetch_one(&mut raw)
            .await
            .unwrap();
            assert_eq!(original_rows, 1, "the damaged original must really exist");
            let triggers: Vec<(String, String)> = sqlx::query_as(
                "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY name",
            ).bind(table).fetch_all(&mut raw).await.unwrap();
            assert!(!triggers.is_empty());
            assert!(
                sqlx::query(AssertSqlSafe(format!(
                    "DELETE FROM \"{table}\" WHERE operation_id=?",
                )))
                .bind(operation(number).as_str())
                .execute(&mut raw)
                .await
                .is_err()
            );
            for (name, _) in &triggers {
                sqlx::query(AssertSqlSafe(format!(
                    "DROP TRIGGER \"{}\"",
                    name.replace('"', "\"\"")
                )))
                .execute(&mut raw)
                .await
                .unwrap();
            }
            let changed = sqlx::query(AssertSqlSafe(format!(
                "DELETE FROM \"{table}\" WHERE operation_id=?",
            )))
            .bind(operation(number).as_str())
            .execute(&mut raw)
            .await
            .unwrap();
            assert_eq!(changed.rows_affected(), 1);
            for (_, ddl) in triggers {
                sqlx::query(AssertSqlSafe(ddl))
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
            raw.close().await.unwrap();
            let damaged = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                db.hub_source_attribution_review(owner(), operation(1950), source_input, at(300))
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::StorageCorrupt,
            );
            assert_eq!(
                db.hub_recipient_attribution_review(
                    owner(),
                    operation(1951),
                    recipient_input.clone(),
                    at(300)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::StorageCorrupt,
            );
            assert_eq!(
                db.submit_hub_mutation(
                    scope,
                    operation(1951),
                    HubMutation::RecipientAttribution(Box::new(recipient_input)),
                    at(300)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::StorageCorrupt,
            );
            assert_eq!(
                crate::hub::custody::tests::material(&fixture).await,
                damaged
            );
            db.close().await.unwrap();
        }
    })
}
