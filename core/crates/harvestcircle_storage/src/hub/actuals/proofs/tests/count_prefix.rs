use super::*;
use harvestcircle_domain::hub::HubRevision;

#[tokio::test]
async fn completed_count_prefix_keeps_empty_extension_parent_edges_hub_and_cold_phase() {
    let world = corrections::count_return_write::tests::native22::association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                let first = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                let target = stock::read::operation_on(&mut reader, hub, &operation(905))
                    .await?
                    .unwrap();
                let returned = stock::read::operation_on(&mut reader, hub, &operation(900))
                    .await?
                    .unwrap();
                let empty = HubRevision::from_value(first.revision.value() - 1).unwrap();
                assert_eq!(reader.count_prefix_checkpoint(), None);
                corrections::test_count_prefix_on(&mut reader, hub, empty).await?;
                assert_eq!(reader.count_prefix_checkpoint(), Some((hub, empty)));
                corrections::test_count_prefix_on(&mut reader, hub, target.revision).await?;
                assert!(reader.count_prefix_covers(hub, target.revision).await?);
                let (_, count_key) = reader
                    .canonical_count_key(hub, &target, "correction_count_explain")
                    .await?;
                let return_key = reader.test_return_key(hub, &returned).await?;
                for key in [count_key.clone(), return_key] {
                    for stock in [target.revision.value(), target.revision.value() - 1] {
                        let mut parent = key.clone();
                        parent.stock = stock;
                        reader.proofs.active.push(parent.clone());
                        assert_eq!(
                            corrections::test_count_global_on(&mut reader, hub)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        assert_eq!(
                            corrections::test_count_prefix_on(&mut reader, hub, target.revision)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        assert_eq!(reader.proofs.active.pop(), Some(parent));
                    }
                }
                let mut foreign = count_key;
                foreign.hub[0] ^= 1;
                reader.proofs.active.push(foreign.clone());
                assert_eq!(
                    corrections::test_count_global_on(&mut reader, hub)
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                assert_eq!(
                    corrections::test_count_prefix_on(&mut reader, hub, target.revision)
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                assert_eq!(reader.proofs.active.pop(), Some(foreign));
                let other = HubId::from_bytes([8; 16]).unwrap();
                assert!(!reader.count_prefix_covers(other, target.revision).await?);
                assert!(reader.count_prefix_covers(hub, empty).await?);
                drop(reader);
                let mut cold = OriginalReader::new(tx);
                assert_eq!(cold.count_prefix_checkpoint(), None);
                corrections::test_count_prefix_on(&mut cold, hub, target.revision).await?;
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

#[tokio::test]
async fn same_connection_writes_rollback_and_schema_change_require_a_new_count_read_phase() {
    for change in 0..3 {
        let (fixture, db, _, expected) = corrections::count_review::tests::explained_world().await;
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let cut = expected.original.explanation.stock_revision;
        let count = expected.original.count.clone();
        let before = crate::hub::custody::tests::material(&fixture).await;
        db.host()
            .transaction(|tx| {
                Box::pin(async move {
                    if change == 1 {
                        sqlx::query("CREATE TABLE count_prefix_probe(value INTEGER)")
                            .execute(&mut *tx).await.unwrap();
                        sqlx::raw_sql(concat!(
                            "CREATE TRIGGER count_prefix_abort AFTER UPDATE ON hub_stock_state BEGIN ",
                            "INSERT INTO count_prefix_probe VALUES (1); ",
                            "SELECT RAISE(ABORT,'owned count prefix rollback probe'); END"
                        )).execute(&mut *tx).await.unwrap();
                    }
                    let mut reader = OriginalReader::new(tx);
                    corrections::test_count_prefix_on(&mut reader, hub, cut).await?;
                    reader.count_history_on(hub,&count,cut).await?;
                    reader.correction_rows_on(hub,count.before.trace,cut).await?;
                    if change == 2 {
                        sqlx::query("CREATE TABLE count_prefix_probe(value INTEGER)")
                            .execute(&mut *reader)
                            .await
                            .unwrap();
                    } else {
                        let result = sqlx::query(
                            "UPDATE hub_stock_state SET event_count=event_count WHERE singleton=1",
                        )
                        .execute(&mut *reader)
                        .await;
                        if change == 1 {
                            assert!(result.is_err());
                            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM count_prefix_probe")
                                .fetch_one(&mut *reader).await.unwrap(), 0);
                        } else {
                            result.unwrap();
                        }
                    }
                    for requested in [cut, HubRevision::from_value(cut.value() + 1).unwrap()] {
                        assert_eq!(corrections::test_count_global_on(&mut reader, hub).await.unwrap_err().code(),SafeErrorCode::StorageCorrupt);
                        assert_eq!(reader.correction_rows_on(hub,count.before.trace,requested).await.unwrap_err().code(),SafeErrorCode::StorageCorrupt);
                        assert_eq!(reader.count_history_on(hub,&count,requested).await.unwrap_err().code(),SafeErrorCode::StorageCorrupt);
                        assert_eq!(
                            corrections::test_count_prefix_on(&mut reader, hub, requested)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                    }
                    drop(reader);
                    if change == 1 {
                        sqlx::query("DROP TRIGGER count_prefix_abort").execute(&mut *tx).await.unwrap();
                    }
                    if change != 0 {
                        sqlx::query("DROP TABLE count_prefix_probe")
                            .execute(&mut *tx)
                            .await
                            .unwrap();
                    }
                    let mut fresh = OriginalReader::new(tx);
                    corrections::test_count_prefix_on(&mut fresh, hub, cut).await?;
                    fresh.count_history_on(hub,&count,cut).await?;
                    fresh.correction_rows_on(hub,count.before.trace,cut).await?;
                    Ok::<_, SafeError>(())
                })
            })
            .await
            .unwrap();
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
