use super::*;

#[tokio::test]
async fn native22_resealed_association_requires_full_plan_and_unwinds_failed_reconstruction() {
    let world = corrections::count_return_write::tests::native22::association_world().await;
    corrections::count_return_write::tests::native22::faults::reseal_two_pack_projection(&world)
        .await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_resealed_reconstruction_unwinds(&world.db, hub).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
    let db = world.fixture.open().await;
    assert_resealed_reconstruction_unwinds(&db, hub).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    db.close().await.unwrap();
}

async fn assert_resealed_reconstruction_unwinds(db: &Database, hub: HubId) {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &operation(906))
                    .await?
                    .unwrap();
                let root = corrections::count_return_ledger::read_on(tx, hub, op).await?;
                assert_eq!(root.packs, 2); // Canonical header and projection seal pass.
                let mut reader = OriginalReader::new(tx);
                for _ in 0..2 {
                    let op = stock::read::operation_on(&mut reader, hub, &operation(906))
                        .await?
                        .unwrap();
                    let rebuilt = reader.proofs.rebuilt;
                    assert_eq!(
                        reader
                            .count_return_association(hub, op)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert!(reader.proofs.rebuilt > rebuilt);
                    assert!(reader.proofs.active.is_empty());
                    assert!(
                        !reader
                            .proofs
                            .verified
                            .keys()
                            .any(|key| key.operation == operation(906).as_str())
                    );
                    assert!(
                        !reader
                            .proofs
                            .completed
                            .keys()
                            .any(|key| key.operation == operation(906).as_str())
                    );
                    let op = stock::read::operation_on(&mut reader, hub, &operation(900))
                        .await?
                        .unwrap();
                    assert_eq!(
                        reader.return_original(hub, op).await?.operation,
                        operation(900)
                    );
                    assert!(reader.proofs.active.is_empty());
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn native22_return_and_association_bodies_and_certificates_keep_canonical_edges_under_eviction()
 {
    let world = corrections::count_return_write::tests::native22::association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let expected_return = world.returned.clone();
    let expected_association = world.associated.clone();
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                for limit in [4096, 0] {
                    let mut reader = OriginalReader::for_test_budget(tx, limit, 64 * 1024 * 1024);
                    let op = stock::read::operation_on(&mut reader, hub, &operation(900))
                        .await?
                        .unwrap();
                    let returned = reader.return_original(hub, op).await?;
                    assert_eq!(returned.as_ref(), expected_return.as_ref());
                    let op = stock::read::operation_on(&mut reader, hub, &operation(900))
                        .await?
                        .unwrap();
                    let repeated = reader.return_original(hub, op).await?;
                    assert_eq!(repeated, returned);
                    if limit > 0 {
                        assert!(Arc::ptr_eq(&returned, &repeated));
                    }
                    let op = stock::read::operation_on(&mut reader, hub, &operation(906))
                        .await?
                        .unwrap();
                    let association = reader.count_return_association(hub, op).await?;
                    assert_eq!(association.as_ref(), expected_association.as_ref());
                    let op = stock::read::operation_on(&mut reader, hub, &operation(906))
                        .await?
                        .unwrap();
                    let repeated = reader.count_return_association(hub, op).await?;
                    assert_eq!(repeated, association);
                    if limit > 0 {
                        assert!(Arc::ptr_eq(&association, &repeated));
                    }
                    let op = stock::read::operation_on(&mut reader, hub, &operation(900))
                        .await?
                        .unwrap();
                    assert!(reader.return_provenance(hub, op).await?.is_none());
                    let op = stock::read::operation_on(&mut reader, hub, &operation(906))
                        .await?
                        .unwrap();
                    assert_eq!(
                        reader.count_return_projection(hub, op).await?,
                        association.original.association
                    );
                    for number in [900, 906] {
                        let mut forged =
                            stock::read::operation_on(&mut reader, hub, &operation(number))
                                .await?
                                .unwrap();
                        forged.hash[0] ^= 1;
                        let error = if number == 900 {
                            reader.return_original(hub, forged).await.unwrap_err()
                        } else {
                            reader
                                .count_return_association(hub, forged)
                                .await
                                .unwrap_err()
                        };
                        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
                        let mut forged =
                            stock::read::operation_on(&mut reader, hub, &operation(number))
                                .await?
                                .unwrap();
                        forged.hash[0] ^= 1;
                        let error = if number == 900 {
                            reader.return_provenance(hub, forged).await.unwrap_err()
                        } else {
                            reader
                                .count_return_projection(hub, forged)
                                .await
                                .unwrap_err()
                        };
                        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
                        let key = reader
                            .proofs
                            .verified
                            .keys()
                            .find(|key| key.operation == operation(number).as_str())
                            .unwrap()
                            .clone();
                        for stock in [key.stock, key.stock - 1] {
                            let mut parent = key.clone();
                            parent.stock = stock;
                            reader.proofs.active.push(parent.clone());
                            let op =
                                stock::read::operation_on(&mut reader, hub, &operation(number))
                                    .await?
                                    .unwrap();
                            let error = if number == 900 {
                                reader.return_original(hub, op).await.unwrap_err()
                            } else {
                                reader.count_return_association(hub, op).await.unwrap_err()
                            };
                            assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
                            let op =
                                stock::read::operation_on(&mut reader, hub, &operation(number))
                                    .await?
                                    .unwrap();
                            let error = if number == 900 {
                                reader.return_provenance(hub, op).await.unwrap_err()
                            } else {
                                reader.count_return_projection(hub, op).await.unwrap_err()
                            };
                            assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
                            assert_eq!(reader.proofs.active.pop(), Some(parent));
                        }
                    }
                    assert!(reader.proofs.active.is_empty());
                    assert_eq!(reader.proofs.completed.is_empty(), limit == 0);
                    assert!(reader.proofs.verified.len() >= 5);
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}
