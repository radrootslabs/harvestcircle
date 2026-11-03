use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::HubMutation;

#[tokio::test]
async fn current_actual_preview_has_no_admission_effect_and_is_invalidated_by_native_changes() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let group = intake.original.position.group;
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
        panic!("report");
    };
    let command = review(&db, saved.original.id, group).await;
    for prior in [operation(1), operation(101), operation(65535)] {
        assert!(
            db.hub_submission(owner(), prior.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.hub_actual_departure_preview(owner(), prior, command.clone(), at(160))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubConflict
        );
    }
    let before = db.hub_group(owner(), group).await.unwrap();
    let raw_before = db
        .hub_actual_reports(owner(), HubActualReportQuery::default())
        .await
        .unwrap();
    let preview = db
        .hub_actual_departure_preview(owner(), operation(81), command.clone(), at(160))
        .await
        .unwrap();
    assert_eq!(preview.revision, before.revision);
    assert_eq!(preview.plan.reduction.on_hand_after, packs(18));
    assert_eq!(preview.plan.reduction.work_changes[0].retired, packs(2));
    assert_eq!(db.hub_group(owner(), group).await.unwrap(), before);
    assert_eq!(
        db.hub_actual_reports(owner(), HubActualReportQuery::default())
            .await
            .unwrap(),
        raw_before
    );
    assert!(
        db.hub_submission(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.hub_actual_departure_preview(other(), operation(81), command.clone(), at(160))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.submit_hub_mutation(
        scope,
        operation(82),
        HubMutation::ActualReport(Box::new(report())),
        at(161),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(81), command.clone(), at(162))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_submission(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    let fresh = review(&db, saved.original.id, group).await;
    let current_preview = db
        .hub_actual_departure_preview(owner(), operation(81), fresh.clone(), at(163))
        .await
        .unwrap();
    assert_ne!(current_preview.revision, preview.revision);
    let crate::HubMutationResult::ActualReviewed(committed) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(fresh.clone())),
            at(163),
        )
        .await
        .unwrap()
    else {
        panic!("review");
    };
    assert_eq!(
        committed.original,
        HubActualReviewOriginal::Departure(Box::new(current_preview.plan))
    );
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(81), fresh, at(164))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_group(owner(), group).await.unwrap().group.packs,
        packs(18)
    );
    db.close().await.unwrap();
}
