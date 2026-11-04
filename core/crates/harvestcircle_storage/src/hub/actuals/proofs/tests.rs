use super::*;
use crate::{
    Database,
    hub::{
        corrections,
        stock::tests::{Fixture, operation, owner},
    },
};
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection};
mod branching;

async fn world() -> (
    Fixture,
    Database,
    HubId,
    harvestcircle_domain::hub::reference::HubRecordId,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (group, _, _) = corrections::projection_native_tests::actual_debit(&db).await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    (fixture, db, hub, group.trace)
}

#[tokio::test]
async fn canonical_original_is_shared_but_a_cached_self_dependency_still_fails() {
    let (fixture, db, hub, _) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                let key = Key::canonical(hub, &op);
                let first = reader.actual_review(hub, op).await?;
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                let second = reader.actual_review(hub, op).await?;
                assert!(Arc::ptr_eq(&first, &second));
                assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (1, 1));
                let mut forged = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                forged.hash[0] ^= 1;
                assert_eq!(
                    reader.actual_review(hub, forged).await.unwrap_err().code(),
                    SafeErrorCode::StorageCorrupt
                );
                assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (1, 1));
                // A corrupt dependency edge must fail even when its target is cached.
                reader.proofs.active.push(key.clone());
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                assert_eq!(
                    reader.actual_review(hub, op).await.unwrap_err().code(),
                    SafeErrorCode::StorageCorrupt
                );
                assert_eq!(reader.proofs.active.pop(), Some(key));
                assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (1, 1));
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn complete_known_reconstruction_reuses_one_original_and_new_read_phase_rebuilds() {
    let (fixture, db, hub, trace) = world().await;
    let current = db
        .hub_actual_known_interpretations(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap()
        .interpretations;
    let (stock, catalog) = (current.stock_revision, current.catalog_revision);
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                {
                    let mut reader = OriginalReader::new(tx);
                    let reconstructed =
                        Box::pin(corrections::actual_interpretations_at_with_proofs_on(
                            &mut reader,
                            hub,
                            trace,
                            stock,
                            catalog,
                        ))
                        .await?;
                    assert_eq!(reconstructed, current);
                    assert_eq!(reader.proofs.rebuilt, 1);
                    assert!(reader.proofs.hits >= 2);
                }
                // A fresh phase owns an empty store, even in the same transaction.
                let mut reader = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                reader.actual_review(hub, op).await?;
                assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (1, 0));
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_original_is_not_cached_and_unwinds_its_active_dependency() {
    let (fixture, db, hub, _) = world().await;
    let mut raw = fixture.raw().await;
    let trigger: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='hub_actual_reviews_no_update'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER hub_actual_reviews_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_actual_reviews SET result_hash=zeroblob(32) WHERE operation_id=?")
        .bind(operation(81).as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(trigger))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                for expected in 1..=2 {
                    let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                        .await?
                        .unwrap();
                    assert_eq!(
                        reader.actual_review(hub, op).await.unwrap_err().code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert!(reader.proofs.completed.is_empty() && reader.proofs.active.is_empty());
                    assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (expected, 0));
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn cache_pressure_evicts_proofs_without_reducing_native_operation_limits() {
    let (_fixture, db, hub, _) = world().await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                let key = Key::canonical(hub, &op);
                let receipt = reader.actual_review(hub, op).await?;
                let one = charge(&key, &receipt).unwrap();
                let mut bounded = Proofs::default();
                // Small cache budgets exercise eviction, not smaller product capacities.
                for n in 0..16 {
                    let mut next = key.clone();
                    next.operation = operation(1000 + n).as_str().to_owned();
                    bounded.retain_bounded(next, Arc::clone(&receipt), 2, 2 * one);
                    assert!(bounded.completed.len() <= 2 && bounded.bytes <= 2 * one);
                }
                assert_eq!(bounded.completed.len(), 2);
                let old = bounded.completed.len();
                let mut next = key.clone();
                next.operation = operation(9000).as_str().to_owned();
                bounded.retain_bounded(next, Arc::clone(&receipt), 2, one - 1);
                assert_eq!(bounded.completed.len(), old);
                assert!(reader.proofs.active.is_empty());
                reader.proofs = bounded;
                let op = stock::read::operation_on(&mut reader, hub, &operation(81))
                    .await?
                    .unwrap();
                assert_eq!(*reader.actual_review(hub, op).await?, *receipt);
                assert_eq!((reader.proofs.rebuilt, reader.proofs.hits), (1, 0));
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    db.close().await.unwrap();
}
