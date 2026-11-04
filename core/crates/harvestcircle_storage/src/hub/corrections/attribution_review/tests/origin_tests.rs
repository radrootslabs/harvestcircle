use super::*;
use harvestcircle_domain::hub::{
    counts::{CountObserveCommand, CountPostCommand},
    movement::PhysicalIdentification,
    reference::WholePacks,
    stock::Occurrence,
};

// Fault injection belongs only to this fresh owned fixture. Restore exact DDL
// and foreign-key enforcement before making a product call.
pub(super) async fn remove_population(raw: &mut sqlx::SqliteConnection, op: &DurableRequestId) {
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut *raw)
        .await
        .unwrap();
    let triggers:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name='hub_receipts' ORDER BY name").fetch_all(&mut *raw).await.unwrap();
    for (name, _) in &triggers {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER \"{}\"",
            name.replace('"', "\"\"")
        )))
        .execute(&mut *raw)
        .await
        .unwrap();
    }
    let deleted = sqlx::query("DELETE FROM hub_receipts WHERE operation_id=?")
        .bind(op.as_str())
        .execute(&mut *raw)
        .await
        .unwrap();
    assert_eq!(deleted.rows_affected(), 1);
    for (_, ddl) in triggers {
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut *raw)
            .await
            .unwrap();
    }
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut *raw)
        .await
        .unwrap();
}

#[tokio::test]
async fn count_origins_keep_surplus_unknown_and_reopen_but_missing_surplus_is_corrupt() {
    for counted in [18, 20, 22] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let catalog = setup(&db).await;
        let receipt = db
            .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), at(100))
            .await
            .unwrap();
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let group = db
            .hub_group(owner(), page.rows[0].position.group)
            .await
            .unwrap();
        let count = db
            .observe_hub_count(
                owner(),
                operation(2),
                CountObserveCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: catalog.revision(),
                    group: group.group.id,
                    expected_group_revision: group.group.revision,
                    counted_packs: WholePacks::from_value(counted).unwrap(),
                    cutoff: Occurrence::Known(at(100)),
                    observation: note(
                        "Fictional current count, independent extra population if surplus",
                    ),
                },
                at(100),
            )
            .await
            .unwrap();
        let review = db
            .hub_count_review(owner(), count.original.id)
            .await
            .unwrap();
        let posted = db
            .apply_hub_count_post(
                owner(),
                operation(3),
                CountPostCommand {
                    expected_stock_revision: review.stock_revision,
                    expected_catalog_revision: review.catalog_revision,
                    count: count.original.id,
                    reviewed_activity: true,
                    surplus_identification: (counted > 20).then(|| PhysicalIdentification {
                        label: HubLabel::new("Separately identified fictional surplus").unwrap(),
                        observation: note("Distinct packs whose source remains unknown"),
                    }),
                    reason: note("Reviewed original and complete current count scope"),
                },
                at(100),
            )
            .await
            .unwrap();
        let trace = posted
            .surplus
            .as_ref()
            .map(|s| s.population.id)
            .unwrap_or(receipt.original.trace.id);
        let command = source_command(
            &db,
            trace,
            source(id(90), None, SourceCertainty::ReportedPossible),
        )
        .await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        let comparison = db
            .hub_source_attribution_review(owner(), operation(1900), command.clone(), at(200))
            .await
            .unwrap();
        assert_eq!(
            comparison.original.source.source,
            SourceAttribution::Unknown
        );
        assert!(matches!(
            comparison.outcome,
            HubAttributionReviewOutcome::Candidate(_)
        ));
        assert!(comparison.known_sources.is_empty());
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_source_attribution_review(owner(), operation(1900), command.clone(), at(200))
                .await
                .unwrap(),
            comparison
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        if counted == 22 {
            let mut raw = fixture.raw().await;
            remove_population(&mut raw, &operation(3)).await;
            raw.close().await.unwrap();
            let damaged = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                db.hub_source_attribution_review(owner(), operation(1900), command, at(200))
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::StorageCorrupt
            );
            assert_eq!(
                crate::hub::custody::tests::material(&fixture).await,
                damaged
            );
        }
        db.close().await.unwrap();
    }
}
