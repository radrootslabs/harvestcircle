//! Forged sidecars carry valid canonical encoding and their own recomputed seal.
//! Only isolated legally posted worlds are changed; immutable admission stays.
use super::*;
use crate::hub::stock::tests::{operation, owner};
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn resealed_frozen_active_identity_omission_reorder_and_replacement_fail_immutable_admission()
{
    for fault in 0..3 {
        let (fixture, db, receipt) = super::super::projection_native_tests::found_review_tests::posting_tests::posted_fixture().await;
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let op = db
            .host()
            .transaction(|tx| {
                Box::pin(async move { stock::read::operation_on(tx, hub, &operation(1700)).await })
            })
            .await
            .unwrap()
            .unwrap();
        let mut raw = fixture.raw().await;
        let bytes: Vec<u8> =
            sqlx::query_scalar("SELECT payload FROM hub_correction_captures WHERE operation_id=?")
                .bind(op.id.as_str())
                .fetch_one(&mut raw)
                .await
                .unwrap();
        let mut data: Data = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            data.identities.len(),
            2,
            "complete positive global set at admission"
        );
        let old = receipt.observation.clone();
        db.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let checked = stock::read::operation_on(tx, hub, &operation(1700))
                        .await?
                        .ok_or_else(corrupt_storage)?;
                    let mut reader = OriginalReader::new(tx);
                    Box::pin(restore_with_proofs_on(&mut reader, hub, &checked, old))
                        .await
                        .map(|_| ())
                })
            })
            .await
            .unwrap();
        match fault {
            0 => {
                data.identities.remove(0);
            }
            1 => data.identities.reverse(),
            2 => {
                let depleted = receipt.original.allocations[0]
                    .funding
                    .before
                    .identification
                    .as_ref()
                    .unwrap();
                assert!(
                    data.identities
                        .iter()
                        .all(|i| i.label != depleted.label.as_str())
                );
                data.identities[0] = Identity {
                    label: depleted.label.as_str().into(),
                    observation: depleted.observation.as_str().into(),
                };
            }
            _ => unreachable!(),
        }
        let forged = encode_data(&data).unwrap();
        assert_ne!(forged, bytes);
        let hash = scope(hub, &op, &forged).unwrap();
        // Normal protected SQL rejects even a correctly resealed mutation.
        let update =
            "UPDATE hub_correction_captures SET payload=?,scope_hash=? WHERE operation_id=?";
        assert!(
            sqlx::query(update)
                .bind(&forged)
                .bind(hash.as_slice())
                .bind(op.id.as_str())
                .execute(&mut raw)
                .await
                .is_err()
        );
        let ddl: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE name='hub_correction_captures_no_update'",
        )
        .fetch_one(&mut raw)
        .await
        .unwrap();
        sqlx::query("DROP TRIGGER hub_correction_captures_no_update")
            .execute(&mut raw)
            .await
            .unwrap();
        sqlx::query(update)
            .bind(&forged)
            .bind(hash.as_slice())
            .bind(op.id.as_str())
            .execute(&mut raw)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
        raw.close().await.unwrap();
        let material = crate::hub::custody::tests::material(&fixture).await;
        let observation = receipt.observation.clone();
        assert_eq!(
            db.host()
                .transaction(|tx| Box::pin(async move {
                    let checked = stock::read::operation_on(tx, hub, &operation(1700))
                        .await?
                        .ok_or_else(corrupt_storage)?;
                    let mut reader = OriginalReader::new(tx);
                    Box::pin(restore_with_proofs_on(
                        &mut reader,
                        hub,
                        &checked,
                        observation,
                    ))
                    .await
                    .map(|_| ())
                }))
                .await
                .map_err(map_transaction_error)
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            db.hub_found_correction_original(owner(), op.id)
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            material,
            "failed reads never repair or replay"
        );
        db.close().await.unwrap();
    }
}
