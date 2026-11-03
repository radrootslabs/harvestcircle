//! Historical findings never release the independently current hold.
use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::HubMutation;
use harvestcircle_domain::hub::{
    actuals::{HistoricalHoldScopeAssessment, HistoricalHoldTiming},
    treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
};

async fn retained_hold_world(occurred: Occurrence, expected: HistoricalHoldTiming) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let group = intake.original.position.group;
    let trace = intake.original.trace.id;
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(2),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred: Occurrence::Known(at(120)),
            observation: note("Observed sample restriction became effective at120, recorded130"),
        },
        at(130),
    )
    .await
    .unwrap();
    let held = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let scope = scope(&db).await;
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let mut command = review(&db, saved.original.id, group).await;
    command.facts.occurred = occurred;
    let crate::HubMutationResult::ActualReviewed(original) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("review")
    };
    assert_eq!(original.historical_holds.len(), 1);
    let HistoricalHoldScopeAssessment::Verified(finding) = &original.historical_holds[0] else {
        panic!("canonical birth proves original whole cohort")
    };
    assert_eq!(finding.timing(), expected);
    assert_eq!(finding.occurred(), occurred);
    assert_eq!(finding.group(), group);
    let after = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(after.holds.len(), held.holds.len());
    assert_eq!(after.holds[0].hold, held.holds[0].hold);
    assert_eq!(
        after.holds[0].created_operation,
        held.holds[0].created_operation
    );
    assert_eq!(
        after.holds[0].released_operation,
        held.holds[0].released_operation
    );
    assert_eq!(after.projection.groups.len(), 1);
    assert_eq!(after.projection.groups[0].active_holds.len(), 1);
    assert_eq!(after.projection.groups[0].packs, packs(18));
    assert_eq!(
        db.hub_group(owner(), group).await.unwrap().group.packs,
        packs(18)
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap(),
        Some(*original)
    );
    let restored = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(restored, after);
    db.close().await.unwrap();
}
#[tokio::test]
async fn established_historical_breach_does_not_clear_current_hold() {
    retained_hold_world(
        Occurrence::Known(at(130)),
        HistoricalHoldTiming::EstablishedBreach,
    )
    .await;
}
#[tokio::test]
async fn verified_pre_hold_departure_does_not_clear_current_hold() {
    retained_hold_world(Occurrence::Known(at(110)), HistoricalHoldTiming::BeforeHold).await;
}
#[tokio::test]
async fn verified_source_with_interval_crossing_hold_retains_uncertain_timing() {
    retained_hold_world(
        Occurrence::Interval {
            earliest: at(110),
            latest: at(130),
        },
        HistoricalHoldTiming::TimingUnknown,
    )
    .await;
}
