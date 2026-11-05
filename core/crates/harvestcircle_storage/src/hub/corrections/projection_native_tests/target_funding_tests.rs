//! Actual original debits and returns; synthetic found rows qualify only their
//! already-sealed metadata readers, never a physical correction posting.
use super::*;
use crate::hub::corrections::target_funding::{self, HubCorrectionFundingQuery};
use harvestcircle_domain::{SafeErrorCode, hub::corrections::PhysicalCorrectionError};

fn query(
    target_operation: DurableRequestId,
    original_group: HubRecordId,
) -> HubCorrectionFundingQuery {
    HubCorrectionFundingQuery {
        target_operation,
        original_group,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    }
}

#[tokio::test]
async fn full_normal_debit_and_established_return_keep_exact_original_scope_after_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let original = departure.record.consumed[0].before.clone();
    let claim = harvestcircle_domain::hub::returns::ReturnDepartureClaim {
        departure: departure.record.id,
        group: Some(original.id),
    };
    super::return_funding_tests::record_return(&db, Some(claim), 120).await;
    let input = query(departure.operation, original.id);
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_correction_target_funding(owner(), input.clone())
        .await
        .unwrap();
    assert_eq!(evidence.quantity.original().packs.value(), 6);
    assert_eq!(evidence.quantity.established_returns().value(), 2);
    assert_eq!(evidence.debits.len(), 1);
    assert_eq!(evidence.debits[0].original().before, original);
    assert_eq!(evidence.debits[0].available().unwrap().value(), 4);
    assert!(!evidence.uncertain_intent && !evidence.unresolved_actual);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_correction_target_funding(owner(), input)
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn actual_debit_uses_the_full_original_twenty_before_two_left_not_current_eighteen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (group, target, packs) = actual_debit(&db).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_correction_target_funding(owner(), query(target, group.id))
        .await
        .unwrap();
    assert_eq!(packs, 2);
    assert_eq!(evidence.quantity.original().packs.value(), 2);
    assert_eq!(evidence.debits[0].original().before.packs.value(), 20);
    assert_eq!(evidence.debits[0].original().packs.value(), 2);
    assert_eq!(evidence.debits[0].available().unwrap().value(), 2);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn uncertain_return_blocks_funding_without_an_arbitrary_reservation() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    super::return_funding_tests::record_return(&db, None, 120).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_correction_target_funding(
            owner(),
            query(departure.operation, departure.record.consumed[0].before.id),
        )
        .await
        .unwrap();
    assert_eq!(evidence.quantity.established_returns().value(), 0);
    assert_eq!(evidence.debits[0].returned().value(), 0);
    assert_eq!(
        evidence.debits[0].available().unwrap_err(),
        PhysicalCorrectionError::UnresolvedReturnScope
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn foreign_or_stale_funding_reads_cannot_substitute_another_cut() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let input = query(departure.operation, departure.record.consumed[0].before.id);
    let current = db
        .hub_correction_target_funding(owner(), input.clone())
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_correction_target_funding(crate::hub::stock::tests::other(), input.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = input.clone();
    stale.expected_stock_revision = Some(HubRevision::INITIAL);
    stale.expected_catalog_revision = Some(current.catalog_revision);
    assert_eq!(
        db.hub_correction_target_funding(owner(), stale.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    stale.expected_stock_revision = None;
    assert_eq!(
        db.hub_correction_target_funding(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    let mut missing = input;
    missing.original_group = id(99);
    assert_eq!(
        db.hub_correction_target_funding(owner(), missing)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

async fn metadata(world: &World) -> Result<target_funding::LoadedFunding, SafeError> {
    let hub = world.hub;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let group = world.projection.target_group;
    let cut = world.op.revision;
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move { target_funding::load_on(tx, hub, &target, group, cut).await })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn canonical_found_credit_keeps_full_debit_and_fixed_review_without_effect_adoption() {
    for actual in [false, true] {
        let world = seed_debit(false, false, actual).await;
        super::quantity_tests::insert_projection(&world, false).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        if actual {
            // A qualified actual requires its full historical interpretation,
            // including the global journal envelope. This deliberately synthetic
            // future Found lacks stock-state/event accounting and cannot fund it.
            assert_eq!(
                metadata(&world).await.err().unwrap().code(),
                SafeErrorCode::StorageCorrupt
            );
        } else {
            let loaded = metadata(&world).await.unwrap();
            assert_eq!(loaded.debits[0].recovered().value(), 2);
            assert_eq!(loaded.debits[0].available().unwrap().value(), 4);
            assert_eq!(loaded.quantity.effective().value(), 4);
        }
        assert_eq!(
            world
                .db
                .hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_initial_review_never_funds_a_credit() {
    let world = seed(false, true).await;
    super::quantity_tests::insert_projection(&world, false).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        metadata(&world).await.err().unwrap().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn intact_actual_summary_cannot_replace_a_damaged_full_debit_original() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (group, target, _) = actual_debit(&db).await;
    crate::hub::custody::tests::corrupt(
        &fixture,
        "hub_actual_reviews_no_update",
        "UPDATE hub_actual_reviews SET result_hash=zeroblob(32)",
    )
    .await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let named = target.clone();
    let id = group.id;
    let state = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let state = stock::read::state_on(tx, hub).await?;
                quantity::original_on(tx, hub, &named, id, state.revision).await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        state.original.packs.value(),
        2,
        "the separate physical summary is intact"
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_correction_target_funding(owner(), query(target, group.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_funding_reads_keep_all_pending_and_unresolved_facts_visible_without_replay() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let runtime = db.begin_hub_runtime(at(140)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(140),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: id(99),
            corrected_packs: WholePacks::from_value(0).unwrap(),
            report: None,
            basis: note(
                "Raw zero classification remains an unresolved fact, not physical permission",
            ),
        },
    };
    db.submit_hub_mutation(
        scope,
        operation(1500),
        HubMutation::CorrectionObservation(Box::new(input.clone())),
        at(150),
    )
    .await
    .unwrap();
    let named = query(departure.operation, departure.record.consumed[0].before.id);
    let recorded = db
        .hub_correction_target_funding(owner(), named.clone())
        .await
        .unwrap();
    assert!(!recorded.uncertain_intent && recorded.unresolved_actual);
    assert_eq!(recorded.debits[0].available().unwrap().value(), 6);
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1501),
            HubMutation::CorrectionObservation(Box::new(input)),
            at(100)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let pending = db
        .hub_submission(owner(), operation(1501))
        .await
        .unwrap()
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let blocked = db
        .hub_correction_target_funding(owner(), named)
        .await
        .unwrap();
    assert!(blocked.uncertain_intent && blocked.unresolved_actual);
    assert_eq!(blocked.debits[0].available().unwrap().value(), 6);
    assert_eq!(
        db.hub_submission(owner(), operation(1501))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
