use super::*;

#[tokio::test]
async fn native22_completed_count_body_keeps_canonical_edges_and_eviction_is_a_miss() {
    let (fixture, db, _, expected) = corrections::count_review::tests::explained_world().await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                assert!(
                    reader
                        .completed_count_explanation(hub, &op)
                        .await?
                        .is_none()
                );
                let first = reader.count_explanation(hub, op).await?;
                let op = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                let cached = reader.completed_count_explanation(hub, &op).await?.unwrap();
                assert!(Arc::ptr_eq(&first, &cached));
                assert_eq!(*cached, *expected);
                let mut forged = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                forged.hash[0] ^= 1;
                assert_eq!(
                    reader
                        .completed_count_explanation(hub, &forged)
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                let (_, key) = reader
                    .canonical_count_key(hub, &op, "correction_count_explain")
                    .await?;
                for stock in [key.stock, key.stock - 1] {
                    let mut parent = key.clone();
                    parent.stock = stock;
                    reader.proofs.active.push(parent.clone());
                    assert_eq!(
                        reader
                            .completed_count_explanation(hub, &op)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert_eq!(reader.proofs.active.pop(), Some(parent));
                }
                drop(reader);
                let mut bounded = OriginalReader::for_test_budget(tx, 0, 0);
                assert!(
                    bounded
                        .completed_count_explanation(hub, &op)
                        .await?
                        .is_none()
                );
                let rebuilt_op = stock::read::operation_on(&mut bounded, hub, &operation(801))
                    .await?
                    .unwrap();
                assert_eq!(
                    *bounded.count_explanation(hub, rebuilt_op).await?,
                    *expected
                );
                assert!(
                    bounded
                        .completed_count_explanation(hub, &op)
                        .await?
                        .is_none()
                );
                assert_eq!(bounded.proof_counts().2, 0);
                assert_eq!(bounded.certificate_counts().1, 1);
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
