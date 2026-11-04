//! Disposable exact-cut reads; no retained world or historical consumer opens.
use super::*;
use harvestcircle_domain::hub::{
    actuals::ActualKnownInterpretations,
    reference::{HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue},
};
use sqlx::{AssertSqlSafe, Connection};

pub(super) async fn observe(
    db: &Database,
    trace: HubRecordId,
    enabled: bool,
    cuts: &mut Vec<ActualKnownInterpretations>,
) {
    if enabled {
        cuts.push(
            Box::pin(db.hub_actual_known_interpretations(
                owner(),
                crate::HubCorrectionKnownQuery {
                    trace,
                    expected_stock_revision: None,
                    expected_catalog_revision: None,
                },
            ))
            .await
            .unwrap()
            .interpretations,
        );
    }
}

async fn at(
    db: &Database,
    trace: HubRecordId,
    cut: &ActualKnownInterpretations,
) -> Result<ActualKnownInterpretations, SafeError> {
    let (stock, catalog) = (cut.stock_revision, cut.catalog_revision);
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                Box::pin(
                    super::super::super::super::known::actual_interpretations_at_on(
                        tx,
                        hub.id(),
                        trace,
                        stock,
                        catalog,
                    ),
                )
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
#[ignore = "Explicit exact-cut interpretation qualification; disposable native19 only"]
async fn historical_interpretations_preserve_saved_cuts_across_quantity_recipient_source_catalogue_and_reopen()
 {
    let mut fixture = Fixture::new();
    fixture.build = build();
    let db = fixture.open().await;
    let (_, mut cuts, scope) = Box::pin(drive_native19_observed(&db, true)).await;
    assert_eq!(cuts.len(), 9);
    let trace = cuts[0].records[0].original.trace;
    let effective: Vec<_> = cuts
        .iter()
        .map(|c| c.records[0].view().unwrap().effective_packs.value())
        .collect();
    assert_eq!(effective, vec![2, 2, 4, 2, 0, 0, 0, 0, 0]);
    assert_eq!(cuts[0].records[0].recipient_history.len(), 0);
    assert_eq!(cuts[6].records[0].recipient_history.len(), 1);
    assert!(cuts[6].records[0].view().unwrap().recipients.has_unknown());
    assert_eq!(cuts[7].records[0].recipient_history.len(), 2);
    assert_eq!(cuts[8].latest_recorded_at, at_time(380));
    let current = cuts.last().unwrap();
    let changed = db
        .submit_hub_mutation(
            scope,
            operation(31000),
            HubMutation::Reference(Box::new(ReferenceCommand {
                expected_catalog_revision: current.catalog_revision,
                mutation: ReferenceMutation::Revise {
                    id: id(4),
                    expected_record_revision: HubRevision::INITIAL,
                    value: ReferenceValue::Party {
                        label: HubLabel::new("Later fictional recipient label").unwrap(),
                    },
                },
            })),
            at_time(400),
        )
        .await
        .unwrap();
    assert!(matches!(changed, crate::HubMutationResult::Reference(_)));
    observe(&db, trace, true, &mut cuts).await;
    assert_eq!(cuts[9].stock_revision, cuts[8].stock_revision);
    assert_ne!(cuts[9].catalog_revision, cuts[8].catalog_revision);
    assert_eq!(cuts[9].latest_recorded_at, cuts[8].latest_recorded_at);
    let before = crate::hub::custody::tests::material(&fixture).await;
    for saved in &cuts {
        assert_eq!(at(&db, trace, saved).await.unwrap(), *saved);
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    for saved in &cuts {
        assert_eq!(at(&db, trace, saved).await.unwrap(), *saved);
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    // A future zero-effect link is absent at the first saved cut. Its damaged
    // full original must fail when included, rather than entering that old cut.
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
        .bind(operation(LINK).as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(trigger))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let damaged = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(at(&db, trace, &cuts[0]).await.unwrap(), cuts[0]);
    assert_eq!(
        at(&db, trace, &cuts[1]).await.unwrap_err().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_actual_known_interpretations(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            }
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        damaged
    );
    db.close().await.unwrap();
}

fn at_time(seconds: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(seconds).unwrap()
}
