use super::*;
use harvestcircle_domain::hub::reference::SourceProvenance;

#[tokio::test]
async fn shallow_history_prefix_matches_cold_cuts_eviction_and_complete_count_binding() {
    let world = corrections::count_return_write::tests::native22::association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let count = Box::new(world.explanation.original.count.clone());
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut cuts = Vec::new();
                for operation_id in [801, 803, 905, 906] {
                    cuts.push(
                        stock::read::operation_on(tx, hub, &operation(operation_id))
                            .await?
                            .unwrap()
                            .revision,
                    );
                }
                let mut expected = Vec::new();
                for cut in &cuts {
                    let mut cold = OriginalReader::new(tx);
                    cold.test_history_limits(0, 0);
                    expected.push(cold.count_history_on(hub, &count, *cut).await?);
                    assert_eq!(cold.test_history_stats().0, 0);
                }
                assert_eq!(
                    expected
                        .iter()
                        .map(|h| (h.explanations.len(), h.amendments.len()))
                        .collect::<Vec<_>>(),
                    vec![(1, 0), (1, 1), (2, 1), (2, 1)]
                );
                for (accounts, rows) in [(0, 0), (1, 1), (1, 3), (4096, 4096)] {
                    let mut warm = OriginalReader::new(tx);
                    warm.test_history_limits(accounts, rows);
                    for index in [0, 1, 2, 3, 0, 2, 1, 3] {
                        assert_eq!(
                            warm.count_history_on(hub, &count, cuts[index]).await?,
                            expected[index]
                        );
                        let stats = warm.test_history_stats();
                        assert!(stats.0 <= accounts && stats.1 <= rows);
                    }
                    if rows >= 3 {
                        assert_eq!(warm.test_history_stats(), (1, 3, Some(cuts[3])));
                    }
                    let mut alias = count.clone();
                    alias.reason = SourceProvenance::new(
                        "Different valid Count-plan content under the same operation",
                    )
                    .unwrap();
                    assert!(alias.validate().is_ok());
                    assert_eq!(
                        warm.count_history_on(hub, &alias, cuts[3])
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert_eq!(
                        warm.count_history_on(hub, &count, cuts[3]).await?,
                        expected[3]
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
