use super::*;
use harvestcircle_domain::hub::{HubRevision, reference::HubRecordId};

#[tokio::test]
async fn completed_inventory_points_match_cold_older_cuts_trace_hub_and_eviction() {
    let world = corrections::count_return_write::tests::native22::association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let trace = world.explanation.original.known_event.trace;
    let other = HubRecordId::from_bytes([249; 16]).unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut cuts = vec![HubRevision::from_value(1).unwrap()];
                for id in [801, 803, 905, 906] {
                    cuts.push(
                        stock::read::operation_on(tx, hub, &operation(id))
                            .await?
                            .unwrap()
                            .revision,
                    );
                }
                let mut expected = Vec::new();
                for cut in &cuts {
                    let mut cold = OriginalReader::new(tx);
                    cold.test_count_row_limit(0);
                    expected.push(cold.correction_rows_on(hub, trace, *cut).await?);
                    assert!(cold.test_count_row_points().is_empty());
                }
                assert_eq!(expected[0], 0);
                assert!(expected.windows(2).all(|pair| pair[0] <= pair[1]));
                assert!(expected[4] > expected[1]);
                for limit in [0, 1, 2, 4096] {
                    let mut warm = OriginalReader::new(tx);
                    warm.test_count_row_limit(limit);
                    // First extension, then unknown older cuts and exact hits.
                    for index in [1, 4, 2, 0, 3, 4, 1, 3] {
                        assert_eq!(
                            warm.correction_rows_on(hub, trace, cuts[index]).await?,
                            expected[index]
                        );
                        assert_eq!(warm.correction_rows_on(hub, other, cuts[index]).await?, 0);
                        assert!(warm.test_count_row_points().len() <= limit);
                    }
                    let checkpoint = warm.test_count_row_points();
                    for _ in 0..2 {
                        let invalid = HubRevision::from_value(cuts[4].value() + 1).unwrap();
                        assert_eq!(
                            warm.correction_rows_on(hub, trace, invalid)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        assert_eq!(warm.test_count_row_points(), checkpoint);
                        assert_eq!(
                            warm.correction_rows_on(
                                HubId::from_bytes([248; 16]).unwrap(),
                                trace,
                                cuts[4]
                            )
                            .await
                            .unwrap_err()
                            .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        assert_eq!(warm.test_count_row_points(), checkpoint);
                    }
                    assert_eq!(
                        warm.correction_rows_on(hub, trace, cuts[1]).await?,
                        expected[1]
                    );
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
