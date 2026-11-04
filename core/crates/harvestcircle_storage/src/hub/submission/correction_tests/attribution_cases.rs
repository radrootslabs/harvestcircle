//! Real pending admission stays separate from retained raw actual uncertainty.
use super::*;
use crate::{HubAttributionReviewOutcome as Outcome, HubSubmissionStatus};
use harvestcircle_domain::hub::{
    corrections::{AttributionError, RecipientAttributionCommand, SourceAttributionCommand},
    reference::{
        SourceAttribution, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
    },
};
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

#[tokio::test]
async fn attribution_candidates_preserve_raw_facts_but_cannot_exclude_pending_admission_after_restart()
 {
    let (fixture, db, additional) = crate::hub::corrections::additional_review_fixture().await;
    let current = db
        .hub_group(owner(), additional.balancing_group)
        .await
        .unwrap();
    let basis = SourceProvenance::new(
        "Read-only comparison is not a new admission or closure of raw facts",
    )
    .unwrap();
    let source = SourceAttributionCommand {
        trace: current.group.trace,
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: crate::hub::stock::tests::id(99),
                issuer: None,
                kind: SourceReferenceKind::Other,
                code: SourceCode::new("Independent reported source").unwrap(),
                provenance: basis.clone(),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
        basis: basis.clone(),
    };
    let recipient = RecipientAttributionCommand {
        departure_operation: additional.facts.target_operation,
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        expected_prior: None,
        recipient: Some(crate::hub::stock::tests::id(4)),
        basis: basis.clone(),
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    let source_before = db
        .hub_source_attribution_review(owner(), operation(1951), source.clone(), at(200))
        .await
        .unwrap();
    let recipient_before = db
        .hub_recipient_attribution_review(owner(), operation(1952), recipient.clone(), at(200))
        .await
        .unwrap();
    assert!(matches!(source_before.outcome, Outcome::Candidate(_)));
    assert!(matches!(recipient_before.outcome, Outcome::Candidate(_)));
    assert!(!source_before.uncertain_intent && source_before.unresolved_actual);
    assert!(!recipient_before.uncertain_intent && recipient_before.unresolved_actual);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
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
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1950),
                    HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                        previous: None,
                        facts: CorrectionObservedFacts::CountClassification {
                            explanation_operation: crate::hub::stock::tests::id(99),
                            corrected_packs: WholePacks::from_value(0).unwrap(),
                            report: None,
                            basis,
                        },
                    })),
                    at(190),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("real owned pending grant")
    };
    let pending = db
        .hub_submission(owner(), operation(1950))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, HubSubmissionStatus::Pending);
    let raw = db
        .hub_actual_report_operation(owner(), operation(1800))
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let source_after = db
        .hub_source_attribution_review(owner(), operation(1951), source.clone(), at(200))
        .await
        .unwrap();
    let recipient_after = db
        .hub_recipient_attribution_review(owner(), operation(1952), recipient.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(
        source_after.outcome,
        Outcome::Blocked(AttributionError::UncertainSubmission)
    );
    assert_eq!(
        recipient_after.outcome,
        Outcome::Blocked(AttributionError::UncertainSubmission)
    );
    assert!(source_after.uncertain_intent && source_after.unresolved_actual);
    assert!(recipient_after.uncertain_intent && recipient_after.unresolved_actual);
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1950), source.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::OperationAlreadyUsed
    );
    assert_eq!(
        db.hub_recipient_attribution_review(owner(), operation(1950), recipient.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::OperationAlreadyUsed
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1800))
            .await
            .unwrap(),
        raw
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    drop(grant);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_submission(owner(), operation(1950))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1951), source, at(200))
            .await
            .unwrap(),
        source_after
    );
    assert_eq!(
        db.hub_recipient_attribution_review(owner(), operation(1952), recipient, at(200))
            .await
            .unwrap(),
        recipient_after
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1800))
            .await
            .unwrap(),
        raw
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
