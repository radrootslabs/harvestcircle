use super::*;
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::corrections::{CorrectedDepartureFamily, PhysicalDebitKey};

#[tokio::test]
async fn actual_and_zero_effective_events_keep_original_recipient_and_complete_source_scope_after_reopen()
 {
    for zero in [false, true] {
        let (fixture, db, additional) = crate::hub::corrections::additional_review_fixture().await;
        let target =
            crate::hub::actuals::common::operation(additional.facts.target_operation).unwrap();
        let original = db
            .hub_actual_review_operation(owner(), target.clone())
            .await
            .unwrap()
            .unwrap();
        let trace = db
            .hub_group(owner(), additional.balancing_group)
            .await
            .unwrap()
            .group
            .trace;
        if zero {
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
            let group = additional.balancing_group;
            let crate::HubMutationResult::AdditionalCorrected(posted) = db
                .submit_hub_mutation(
                    scope,
                    operation(1900),
                    HubMutation::AdditionalQuantity(Box::new(additional)),
                    at(200),
                )
                .await
                .unwrap()
            else {
                panic!("additional")
            };
            for (n, time, debit) in [
                (
                    2200,
                    210,
                    crate::hub::actuals::common::operation_id(&target).unwrap(),
                ),
                (2210, 220, posted.original.operation),
            ] {
                crate::hub::corrections::additional_write_tests::found(
                    &db,
                    scope,
                    &target,
                    group,
                    PhysicalDebitKey {
                        operation: debit,
                        group,
                    },
                    n,
                    time,
                )
                .await;
            }
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
            assert_eq!(known.departures.len(), 1);
            assert_eq!(known.departures[0].quantities[0].effective().value(), 0);
        }
        let source_command = source_command(
            &db,
            trace,
            source(id(60), Some(id(3)), SourceCertainty::Established),
        )
        .await;
        let recipient_command = recipient_command(&db, &target, Some(id(4))).await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        let sources = db
            .hub_source_attribution_review(
                owner(),
                operation(1950),
                source_command.clone(),
                at(250),
            )
            .await
            .unwrap();
        assert_eq!(sources.affected.len(), 1);
        assert_eq!(
            sources.affected[0].operation,
            crate::hub::actuals::common::operation_id(&target).unwrap()
        );
        assert_eq!(sources.affected[0].populations, vec![trace]);
        assert!(sources.affected[0].issued_versions.is_empty());
        assert!(matches!(
            sources.outcome,
            HubAttributionReviewOutcome::Candidate(_)
        ));
        assert_eq!(sources.unresolved_actual, !zero);
        let recipients = db
            .hub_recipient_attribution_review(
                owner(),
                operation(1951),
                recipient_command.clone(),
                at(250),
            )
            .await
            .unwrap();
        assert_eq!(
            recipients.original.family,
            CorrectedDepartureFamily::ReviewedActual
        );
        assert!(recipients.linked_issued_versions.is_empty());
        let HubAttributionReviewOutcome::Candidate(plan) = &recipients.outcome else {
            panic!("actual recipient")
        };
        assert_eq!(
            plan.before,
            AmendedRecipient::Known(recipients.original.recipient.clone())
        );
        assert!(matches!(&plan.after,AmendedRecipient::Known(party) if party.id==id(4)));
        assert_eq!(recipients.unresolved_actual, !zero);
        assert_eq!(
            db.hub_recipient_attribution_review(
                other(),
                operation(1951),
                recipient_command.clone(),
                at(250)
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(
            db.hub_recipient_attribution_review(
                owner(),
                target.clone(),
                recipient_command.clone(),
                at(250)
            )
            .await
            .unwrap()
            .outcome,
            HubAttributionReviewOutcome::OperationAlreadyUsed
        );
        let mut wrong = recipient_command.clone();
        wrong.departure_operation =
            crate::hub::actuals::common::operation_id(&operation(1800)).unwrap();
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1951), wrong, at(250))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubInvalidInput
        );
        let mut wrong = recipient_command.clone();
        wrong.recipient = Some(id(1));
        assert_eq!(
            db.hub_recipient_attribution_review(owner(), operation(1951), wrong, at(250))
                .await
                .unwrap()
                .outcome,
            HubAttributionReviewOutcome::Blocked(AttributionError::InvalidReference)
        );
        assert_eq!(
            db.hub_actual_review_operation(owner(), target.clone())
                .await
                .unwrap(),
            Some(original.clone())
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_source_attribution_review(owner(), operation(1950), source_command, at(250))
                .await
                .unwrap(),
            sources
        );
        assert_eq!(
            db.hub_recipient_attribution_review(
                owner(),
                operation(1951),
                recipient_command,
                at(250)
            )
            .await
            .unwrap(),
            recipients
        );
        assert_eq!(
            db.hub_actual_review_operation(owner(), target)
                .await
                .unwrap(),
            Some(original)
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
