//! Default-stack native composition; historical profiles remain unopened.
use super::*;
use crate::hub::actuals::proofs::OriginalReader;

#[tokio::test]
#[ignore = "Explicit native19 mixed original-proof qualification; disposable profile only"]
async fn native19_source_recipient_additional_found_and_link_share_one_original_proof_owner() {
    let mut fixture = Fixture::new();
    fixture.build = build();
    let db = fixture.open().await;
    Box::pin(drive_native19(&db)).await;
    let original = db
        .hub_actual_review_operation(owner(), operation(81))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = original.original else {
        panic!("actual")
    };
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let trace = plan.reduction.before.trace;
    let expected = db
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
    let (stock, catalog) = (expected.stock_revision, expected.catalog_revision);
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host().transaction(|tx| Box::pin(async move {
        let mut reader = OriginalReader::new(tx);
        Box::pin(crate::hub::corrections::headers_with_proofs_on(&mut reader, hub)).await?;
        assert_eq!(Box::pin(crate::hub::corrections::actual_interpretations_at_with_proofs_on(&mut reader, hub, trace, stock, catalog)).await?, expected);
        let counts = reader.proof_counts();
        assert_eq!(counts.0, 2, "One complete proof for the physical review and one zero-effect link");
        assert!(counts.1 >= 8 && counts.2 == 2 && counts.3 <= 64*1024*1024);
        println!("Mixed Source/Recipient/Additional/Found/Link phase rebuild/hit/entry/charged-byte counts: {counts:?}");
        Ok::<_, SafeError>(())
    })).await.unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
