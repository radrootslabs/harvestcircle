use super::*;
use crate::hub::corrections::{funding, quantity};
use harvestcircle_domain::hub::{
    corrections::{OriginalPhysicalDebit, PhysicalDebitFunding, PhysicalDebitKind},
    reference::SourceAttribution,
    returns::{ReturnCommand, ReturnDepartureClaim, ReturnLinkResolution},
};

pub(in crate::hub::corrections) async fn record_return(
    db: &Database,
    claim: Option<ReturnDepartureClaim>,
    time: i64,
) -> crate::HubReturnReceipt {
    record_return_variant(db, claim, time, id(2)).await
}

async fn record_return_variant(
    db: &Database,
    claim: Option<ReturnDepartureClaim>,
    time: i64,
    variant: HubRecordId,
) -> crate::HubReturnReceipt {
    let command = return_command(db, claim, variant).await;
    db.apply_hub_return(owner(), operation(100), command, at(time))
        .await
        .unwrap()
}

pub(super) async fn return_command(
    db: &Database,
    claim: Option<ReturnDepartureClaim>,
    variant: HubRecordId,
) -> ReturnCommand {
    let state = db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    ReturnCommand {
        expected_stock_revision: state.revision,
        expected_catalog_revision: catalog.revision(),
        variant,
        location: id(6),
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: WholePacks::from_value(2).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new("Independently identified returned two").unwrap(),
            observation: note("Actual returned custody kept separate from any found remainder"),
        },
        planned_location: None,
        departure: claim,
        occurred: Occurrence::Unknown,
        observation: note("Observed return, exact original scope remains independently reviewed"),
    }
}

async fn read(
    db: &Database,
    target: DurableRequestId,
    group: HubRecordId,
    cut: HubRevision,
) -> funding::ReturnReservations {
    let hub = db.load_hub().await.unwrap().unwrap().id();
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let line = quantity::original_on(tx, hub, &target, group, cut).await?;
                funding::returns_on(tx, hub, &line, cut).await
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn native_established_return_reserves_only_its_exact_original_debit_at_the_cut() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let before = departure.record.consumed[0].before.clone();
    let original = read(
        &db,
        departure.operation.clone(),
        before.id,
        departure.record.stock_revision,
    )
    .await;
    assert!(original.established.is_empty());
    assert!(!original.unresolved);
    let returned = record_return(
        &db,
        Some(ReturnDepartureClaim {
            departure: departure.record.id,
            group: Some(before.id),
        }),
        120,
    )
    .await;
    assert_eq!(
        returned.original.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    let reservations = read(
        &db,
        departure.operation.clone(),
        before.id,
        returned.original.stock_revision,
    )
    .await;
    assert_eq!(reservations.total.value(), 2);
    assert_eq!(reservations.established.len(), 1);
    assert!(!reservations.unresolved);
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let target = departure.operation.clone();
    let group = before.id;
    let cut = returned.original.stock_revision;
    let line = db
        .host()
        .transaction(|tx| {
            Box::pin(async move { quantity::original_on(tx, hub, &target, group, cut).await })
        })
        .await
        .unwrap();
    let account = PhysicalDebitFunding::restore(
        OriginalPhysicalDebit {
            key: PhysicalDebitKey {
                operation: line.original.operation,
                group,
            },
            kind: PhysicalDebitKind::NormalDeparture,
            target: line.original,
            before,
            packs: line.original.packs,
            stock_revision: line.original.stock_revision,
            recorded_at: line.recorded_at,
        },
        Vec::new(),
        reservations.established,
        reservations.unresolved,
    )
    .unwrap();
    assert_eq!(account.available().unwrap().value(), 4);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_unlinked_same_variant_return_is_uncertainty_without_arbitrary_deduction() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let returned = record_return(&db, None, 120).await;
    let reservations = read(
        &db,
        departure.operation.clone(),
        departure.record.consumed[0].before.id,
        returned.original.stock_revision,
    )
    .await;
    assert!(reservations.unresolved);
    assert!(reservations.established.is_empty());
    assert_eq!(reservations.total.value(), 0);
    db.close().await.unwrap();
}

#[tokio::test]
async fn return_recorded_before_historical_actual_review_still_blocks_its_funding() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) = actual_debit_with_early_return(&db, true).await;
    let cut = db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap()
        .revision;
    let reservations = read(&db, target, before.id, cut).await;
    assert!(reservations.unresolved);
    assert!(reservations.established.is_empty());
    assert_eq!(reservations.total.value(), 0);
    db.close().await.unwrap();
}

#[tokio::test]
async fn canonical_established_return_to_a_disjoint_group_does_not_reserve_this_debit() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, first) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let returned = record_return(
        &db,
        Some(ReturnDepartureClaim {
            departure: first.record.id,
            group: Some(first.record.consumed[0].before.id),
        }),
        100,
    )
    .await;
    assert_eq!(
        returned.original.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    let second = crate::hub::handling::departure::tests::custody_depart_current_group(
        &db,
        intake.original.position.group,
        220,
    )
    .await;
    let reservations = read(
        &db,
        second.operation.clone(),
        second.record.consumed[0].before.id,
        second.record.stock_revision,
    )
    .await;
    assert!(!reservations.unresolved);
    assert!(reservations.established.is_empty());
    assert_eq!(reservations.total.value(), 0);
    db.close().await.unwrap();
}

#[tokio::test]
async fn damaged_return_variant_cannot_hide_a_potential_reservation_from_the_scan() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let returned = record_return(&db, None, 120).await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    // Valid reference coordinates with the wrong kind expose a pre-filter bug:
    // this row must be verified before its claimed variant can exclude it.
    let mut raw = fixture.raw().await;
    assert!(
        sqlx::query("UPDATE hub_receipts SET variant_id=? WHERE trace_id=?")
            .bind(id(3).as_bytes().as_slice())
            .bind(
                returned
                    .original
                    .identities
                    .population
                    .as_bytes()
                    .as_slice()
            )
            .execute(&mut raw)
            .await
            .is_err()
    );
    let ddl: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='hub_receipts_no_update'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    sqlx::query("DROP TRIGGER hub_receipts_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE hub_receipts SET variant_id=? WHERE trace_id=?")
            .bind(id(3).as_bytes().as_slice())
            .bind(
                returned
                    .original
                    .identities
                    .population
                    .as_bytes()
                    .as_slice()
            )
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query(sqlx::AssertSqlSafe(ddl.clone()))
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_schema WHERE name='hub_receipts_no_update'"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        ddl
    );
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let target = departure.operation;
    let group = departure.record.consumed[0].before.id;
    let cut = returned.original.stock_revision;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let line = quantity::original_on(tx, hub, &target, group, cut).await?;
                funding::returns_on(tx, hub, &line, cut).await
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        result.err().unwrap().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn explicit_conflicting_variant_claim_keeps_the_target_blocked_without_unlike_pack_deduction()
{
    use harvestcircle_domain::hub::{
        reference::{ReferenceCommand, ReferenceMutation, ReferenceValue},
        returns::UnresolvedReturnLink,
    };
    for claimed in [false, true] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let (_, departure) =
            crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(500),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create {
                    id: id(12),
                    value: ReferenceValue::Pack {
                        product: id(1),
                        label: HubLabel::new("Different original pack variant").unwrap(),
                        nominal_grams: None,
                    },
                },
            },
            at(100),
        )
        .await
        .unwrap();
        let claim = claimed.then_some(ReturnDepartureClaim {
            departure: departure.record.id,
            group: Some(departure.record.consumed[0].before.id),
        });
        let returned = record_return_variant(&db, claim, 120, id(12)).await;
        assert_eq!(
            returned.original.linkage.resolution,
            if claimed {
                ReturnLinkResolution::Unresolved(UnresolvedReturnLink::VariantConflict)
            } else {
                ReturnLinkResolution::Unknown
            }
        );
        let reservations = read(
            &db,
            departure.operation.clone(),
            departure.record.consumed[0].before.id,
            returned.original.stock_revision,
        )
        .await;
        assert_eq!(reservations.unresolved, claimed);
        assert!(reservations.established.is_empty());
        assert_eq!(reservations.total.value(), 0);
        db.close().await.unwrap();
    }
}
