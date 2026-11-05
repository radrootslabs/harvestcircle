use super::*;

#[tokio::test]
async fn native20_evicted_receipts_keep_exact_certified_known_and_complete_before_but_no_active_edge_or_new_phase_grant()
 {
    let (fixture, db, hub, _) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::for_test_budget(tx, 0, 0);
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                let receipt = reader.actual_review(hub, op).await?;
                let HubActualReviewOriginal::Departure(plan) = &receipt.original else {
                    panic!("physical")
                };
                assert!(!plan.reduction.before.work.is_empty());
                let expected_known = plan.physical_projection().unwrap();
                for _ in 0..2 {
                    let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                        .await?
                        .unwrap();
                    assert_eq!(reader.actual_known(hub, &op).await?, expected_known);
                    assert_eq!(reader.actual_before(hub, &op).await?, plan.reduction.before);
                }
                assert_eq!(reader.proof_counts().0, 1);
                assert_eq!(reader.proof_counts().2, 0);
                assert_eq!(reader.certificate_counts(), (4, 1));
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                let (_, key) = reader.canonical_key(hub, &op).await?;
                for stock in [key.stock, key.stock - 1] {
                    let mut parent = key.clone();
                    parent.stock = stock;
                    reader.proofs.active.push(parent);
                    assert_eq!(
                        reader
                            .actual_is_departure(hub, &op)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    reader.proofs.active.pop();
                }
                assert_eq!(reader.certificate_counts(), (4, 1));
                drop(reader);
                let mut renewed = OriginalReader::for_test_budget(tx, 0, 0);
                assert_eq!(renewed.certificate_counts(), (0, 0));
                assert_eq!(
                    renewed.actual_before(hub, &op).await?,
                    plan.reduction.before
                );
                assert_eq!(renewed.proof_counts().0, 1);
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native20_new_phase_rejects_damaged_work_frozen_parent_and_projection_after_prior_full_certificate()
 {
    for fault in 0..3 {
        let (fixture, db, hub, _) = world().await;
        db.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let mut reader = OriginalReader::for_test_budget(tx, 0, 0);
                    let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                        .await?
                        .unwrap();
                    reader.actual_before(hub, &op).await?;
                    assert_eq!(reader.certificate_counts().1, 1);
                    Ok::<_, SafeError>(())
                })
            })
            .await
            .unwrap();
        let (guard, update) = match fault {
            0 => (
                "hub_actual_review_work_scope_no_update",
                "UPDATE hub_actual_review_work_scope SET before_remaining=before_remaining+1 WHERE operation_id=?",
            ),
            1 => (
                "hub_actual_review_lineage_no_update",
                "UPDATE hub_actual_review_lineage SET parent_group_id=group_id WHERE operation_id=?",
            ),
            2 => (
                "hub_actual_reviews_no_update",
                "UPDATE hub_actual_reviews SET projection_hash=zeroblob(32) WHERE operation_id=?",
            ),
            _ => unreachable!(),
        };
        let mut raw = fixture.raw().await;
        assert!(
            sqlx::query(update)
                .bind(operation(81).as_str())
                .execute(&mut raw)
                .await
                .is_err()
        );
        let ddl: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=? AND type='trigger'")
                .bind(guard)
                .fetch_one(&mut raw)
                .await
                .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {guard}")))
            .execute(&mut raw)
            .await
            .unwrap();
        assert!(
            sqlx::query(update)
                .bind(operation(81).as_str())
                .execute(&mut raw)
                .await
                .unwrap()
                .rows_affected()
                > 0
        );
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_actual_review_operation(owner(), operation(81))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "fault{fault}"
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_actual_review_operation(owner(), operation(81))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "reopenfault{fault}"
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
