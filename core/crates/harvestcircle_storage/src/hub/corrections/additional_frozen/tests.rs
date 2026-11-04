use super::*;
use crate::hub::stock::tests::{operation, owner};
use harvestcircle_application::hub_submission::HubMutation;
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection};
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

#[tokio::test]
async fn resealed_frozen_nodes_and_work_cannot_change_immutable_admitted_material() {
    for fault in 0..4 {
        let (fixture, db, command, scope, _) =
            super::super::additional_write_tests::normal_world(false, false).await;
        let crate::HubMutationResult::AdditionalCorrected(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::AdditionalQuantity(Box::new(command.clone())),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("legal additional")
        };
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let op = db
            .host()
            .transaction(|tx| {
                Box::pin(async move { stock::read::operation_on(tx, hub, &operation(1900)).await })
            })
            .await
            .unwrap()
            .unwrap();
        let mut raw = fixture.raw().await;
        let payload: Vec<u8> =
            sqlx::query_scalar("SELECT payload FROM hub_correction_captures WHERE operation_id=?")
                .bind(op.id.as_str())
                .fetch_one(&mut raw)
                .await
                .unwrap();
        let mut data: Data = serde_json::from_slice(&payload).unwrap();
        assert!(data.nodes.len() >= 2);
        assert!(!data.group.work.is_empty());
        match fault {
            0 => data.nodes.retain(|n| n.id == data.group.id),
            1 => data.nodes.reverse(),
            2 => data.group.work.clear(),
            3 => data.group.packs -= 1,
            _ => unreachable!(),
        }
        let payload = bytes(&data).unwrap();
        let seal = super::scope(hub, &op, &payload).unwrap();
        let triggers:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name='hub_correction_captures' ORDER BY name").fetch_all(&mut raw).await.unwrap();
        for (name, _) in &triggers {
            sqlx::query(AssertSqlSafe(format!(
                "DROP TRIGGER \"{}\"",
                name.replace('"', "\"\"")
            )))
            .execute(&mut raw)
            .await
            .unwrap();
        }
        assert_eq!(
            sqlx::query(
                "UPDATE hub_correction_captures SET payload=?,scope_hash=? WHERE operation_id=?"
            )
            .bind(payload)
            .bind(seal.as_slice())
            .bind(op.id.as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
            1
        );
        for (_, ddl) in triggers {
            sqlx::query(AssertSqlSafe(ddl))
                .execute(&mut raw)
                .await
                .unwrap();
        }
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        // Direct restore must fail before checking the unchanged full result:
        // recomputing a sidecar seal cannot change admitted facts or order.
        let result = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let mut reader = OriginalReader::new(tx);
                    restore_with_proofs_on(&mut reader, hub, &op, &command)
                        .await
                        .map(|_| ())
                })
            })
            .await
            .map_err(map_transaction_error);
        assert_eq!(
            result.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt,
            "restore {fault}"
        );
        assert_eq!(
            db.hub_additional_quantity_original(owner(), receipt.operation)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "original {fault}"
        );
        assert_eq!(
            db.hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "current {fault}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            before,
            "no repair {fault}"
        );
        db.close().await.unwrap();
    }
}
