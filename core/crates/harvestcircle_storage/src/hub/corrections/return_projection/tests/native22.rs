use super::*;
use crate::hub::stock::tests::{id, operation, owner, query};
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_domain::hub::{
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceAttribution, SourceProvenance},
    returns::{ReturnCommand, ReturnDepartureClaim},
    stock::Occurrence,
};
use harvestcircle_domain::{SafeErrorCode, UnixTimestamp};
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn native22_resealed_output_and_coordinated_saved_totals_cannot_replace_full_return_original()
{
    for fault in 0..2 {
        let (fixture, db, found) = crate::hub::corrections::projection_native_tests::found_review_tests::posting_tests::posted_fixture().await;
        let target = &found.original.original_target;
        let target_operation = crate::hub::actuals::common::operation(target.operation).unwrap();
        let departure = db
            .hub_departure_operation(owner(), target_operation.clone())
            .await
            .unwrap()
            .unwrap();
        let runtime = db.hub_runtime_session().await.unwrap().unwrap();
        let scope = HubSubmissionScope {
            runtime: runtime.runtime,
            generation: runtime.generation,
            actor: owner(),
        };
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let evidence = SourceProvenance::new(
            "Independent later restricted intake, not a renewed departure grant",
        )
        .unwrap();
        let crate::HubMutationResult::Returned(returned) = db
            .submit_hub_mutation(
                scope,
                operation(4000),
                HubMutation::Return(Box::new(ReturnCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: page.catalog_revision,
                    variant: id(2),
                    location: id(6),
                    returned_by: None,
                    source: SourceAttribution::Unknown,
                    packs: WholePacks::from_value(1).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Later identified returned crate for output proof")
                            .unwrap(),
                        observation: evidence.clone(),
                    },
                    planned_location: None,
                    departure: Some(ReturnDepartureClaim {
                        departure: departure.record.id,
                        group: Some(target.group),
                    }),
                    occurred: Occurrence::Unknown,
                    observation: evidence,
                })),
                UnixTimestamp::from_seconds(210).unwrap(),
            )
            .await
            .unwrap()
        else {
            panic!("ordinary owned Return")
        };
        assert!(returned.original.current_allowances.is_some());
        assert_eq!(
            db.hub_return_operation(owner(), operation(4000))
                .await
                .unwrap()
                .as_ref(),
            Some(returned.as_ref())
        );
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let mut raw = fixture.raw().await;
        let original: Vec<u8> = sqlx::query_scalar(
            "SELECT payload FROM hub_correction_return_projections WHERE operation_id=?",
        )
        .bind(operation(4000).as_str())
        .fetch_one(&mut raw)
        .await
        .unwrap();
        let mut data = decode(&original).unwrap();
        if fault == 0 {
            data.packs += 1;
        } else {
            assert_eq!(data.totals.len(), 1);
            assert_eq!(data.totals[0].packs, 4);
            data.totals[0].packs = 3;
        }
        let result: Vec<u8> =
            sqlx::query_scalar("SELECT result_hash FROM hub_returns WHERE operation_id=?")
                .bind(operation(4000).as_str())
                .fetch_one(&mut raw)
                .await
                .unwrap();
        let result: [u8; 32] = result.try_into().unwrap();
        let bytes = encode(&data).unwrap();
        let projection_hash = seal(data.allowance_scope_hash, result, &bytes).unwrap();
        assert_eq!(decode(&bytes).unwrap(), data);
        let update = "UPDATE hub_correction_return_projections SET payload=?,projection_hash=? WHERE operation_id=?";
        assert!(
            sqlx::query(update)
                .bind(&bytes)
                .bind(projection_hash.as_slice())
                .bind(operation(4000).as_str())
                .execute(&mut raw)
                .await
                .is_err()
        );
        let mut tx = raw.begin().await.unwrap();
        let mut ddls = Vec::new();
        for name in if fault == 0 {
            vec!["hub_correction_return_projections_no_update"]
        } else {
            vec![
                "hub_correction_return_projections_no_update",
                "hub_return_sources_no_update",
            ]
        } {
            let ddl: String =
                sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?")
                    .bind(name)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {name}")))
                .execute(&mut *tx)
                .await
                .unwrap();
            ddls.push(ddl);
        }
        assert_eq!(
            sqlx::query(update)
                .bind(bytes)
                .bind(projection_hash.as_slice())
                .bind(operation(4000).as_str())
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        if fault == 1 {
            assert_eq!(
                sqlx::query(
                    "UPDATE hub_return_sources SET previously_returned_packs=3 WHERE return_id=?"
                )
                .bind(returned.original.identities.returned.as_bytes().as_slice())
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
                1
            );
        }
        for ddl in ddls {
            sqlx::query(AssertSqlSafe(ddl))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut raw)
                .await
                .unwrap()
                .is_empty()
        );
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_full_rejection(&db, hub, target_operation.clone(), target.group, fault).await;
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_full_rejection(&db, hub, target_operation, target.group, fault).await;
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

async fn assert_full_rejection(
    db: &Database,
    hub: HubId,
    target: DurableRequestId,
    group: HubRecordId,
    fault: u8,
) {
    if fault == 1 {
        // A structurally sealed lower projection is deliberately insufficient:
        // only full scope/original reconstruction authenticates the saved totals.
        db.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let op = stock::read::operation_on(tx, hub, &operation(4000))
                        .await?
                        .unwrap();
                    assert!(read_on(tx, hub, &op).await?.is_some());
                    Ok::<_, SafeError>(())
                })
            })
            .await
            .unwrap();
    }
    assert_eq!(
        db.hub_return_operation(owner(), operation(4000))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_correction_target_funding(
            owner(),
            crate::HubCorrectionFundingQuery {
                target_operation: target,
                original_group: group,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::StorageCorrupt
    );
}
