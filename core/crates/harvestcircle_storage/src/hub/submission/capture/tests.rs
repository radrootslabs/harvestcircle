use super::*;
use crate::hub::{
    corrections,
    stock::tests::{operation, other, owner},
};
use harvestcircle_domain::{
    SafeErrorCode, UnixTimestamp,
    hub::corrections::{
        CorrectionObservationCommand, CorrectionObservedFacts, CountClassificationCommand,
    },
};

#[tokio::test]
async fn classification_capture_shared_phase_matches_fresh_material_bytes_without_admission() {
    let (fixture, db, scope, _) = corrections::count_review::tests::explained_world().await;
    let own = |n| crate::hub::actuals::common::operation_id(&operation(n)).unwrap();
    corrections::count_return_write::tests::native22::classification(
        &db,
        scope,
        own(801),
        (own(801), None),
        0,
        802,
        210,
    )
    .await
    .unwrap();
    let crate::HubMutationResult::CorrectionObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(804),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: own(801),
                    corrected_packs: harvestcircle_domain::hub::reference::WholePacks::from_value(
                        1,
                    )
                    .unwrap(),
                    report: None,
                    basis: harvestcircle_domain::hub::reference::SourceProvenance::new(
                        "Independent revised whole-pack evidence",
                    )
                    .unwrap(),
                },
            })),
            UnixTimestamp::from_seconds(250).unwrap(),
        )
        .await
        .unwrap()
    else {
        panic!("ordinary factual observation")
    };
    let command = CountClassificationCommand {
        observation_operation: observed.original.operation,
        expected_stock_revision: observed.original.stock_revision,
        expected_catalog_revision: observed.original.catalog_revision,
        expected_account_prior: Some(own(803)),
        expected_prior: Some(own(803)),
    };
    let before = crate::hub::custody::tests::material_digest(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                // The unchanged raw material adapter deliberately owns a separate reader.
                // Classification has no extra generic target/reference material.
                let raw = corrections::capture_count_classification_on(
                    tx,
                    hub.id(),
                    owner(),
                    &operation(805),
                    &command,
                )
                .await?;
                let expected = HubSubmittedRequest::CountClassification(&command)
                    .capture(
                        &operation(805),
                        scope,
                        &HubSubmissionContext {
                            hub,
                            stock_revision: Some(command.expected_stock_revision),
                            catalog_revision: Some(command.expected_catalog_revision),
                            material: &[HubSubmittedMaterial::CountClassification(Box::new(raw))],
                        },
                    )
                    .unwrap();
                for entries in [4096, 0] {
                    let mut reader = OriginalReader::for_test_budget(tx, entries, 64 * 1024 * 1024);
                    let captured = capture_with_proofs_on(
                        &mut reader,
                        owner(),
                        scope,
                        &operation(805),
                        HubSubmittedRequest::CountClassification(&command),
                    )
                    .await?;
                    assert_eq!(captured.as_bytes(), expected.as_bytes());
                    let rebuilt = reader.proof_counts().0;
                    assert!(rebuilt > 0);
                    let again = capture_with_proofs_on(
                        &mut reader,
                        owner(),
                        scope,
                        &operation(805),
                        HubSubmittedRequest::CountClassification(&command),
                    )
                    .await?;
                    assert_eq!(again.as_bytes(), expected.as_bytes());
                    if entries != 0 {
                        assert_eq!(reader.proof_counts().0, rebuilt);
                    }
                    for foreign_scope in [
                        scope,
                        HubSubmissionScope {
                            actor: other(),
                            ..scope
                        },
                    ] {
                        assert_eq!(
                            capture_with_proofs_on(
                                &mut reader,
                                other(),
                                foreign_scope,
                                &operation(805),
                                HubSubmittedRequest::CountClassification(&command)
                            )
                            .await
                            .err()
                            .unwrap()
                            .code(),
                            SafeErrorCode::HubPermissionDenied
                        );
                    }
                    let mut stale = command.clone();
                    stale.expected_stock_revision = harvestcircle_domain::hub::HubRevision::INITIAL;
                    assert_eq!(
                        capture_with_proofs_on(
                            &mut reader,
                            owner(),
                            scope,
                            &operation(805),
                            HubSubmittedRequest::CountClassification(&stale)
                        )
                        .await
                        .err()
                        .unwrap()
                        .code(),
                        SafeErrorCode::HubConflict
                    );
                    sqlx::query(
                        "UPDATE hub_stock_state SET event_count=event_count WHERE singleton=1",
                    )
                    .execute(&mut *reader)
                    .await
                    .unwrap();
                    assert_eq!(
                        capture_with_proofs_on(
                            &mut reader,
                            owner(),
                            scope,
                            &operation(805),
                            HubSubmittedRequest::CountClassification(&command)
                        )
                        .await
                        .err()
                        .unwrap()
                        .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    drop(reader);
                    let fresh = capture_on(
                        tx,
                        owner(),
                        scope,
                        &operation(805),
                        HubSubmittedRequest::CountClassification(&command),
                    )
                    .await?;
                    assert_eq!(fresh.as_bytes(), expected.as_bytes());
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert!(
        db.hub_submission(owner(), operation(805))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.hub_count_classification_operation(owner(), operation(805))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        crate::hub::custody::tests::material_digest(&fixture).await,
        before
    );
    db.close().await.unwrap();
}
