//! Actual fresh admission/effect/original recovery; no seeded terminal results.
use super::return_funding_tests::{record_return, return_command};
use super::*;
use harvestcircle_domain::hub::{
    returns::{ReturnDepartureClaim, ReturnLinkResolution, UnresolvedReturnLink},
    treatment::AcceptanceStatus,
};
use sqlx::AssertSqlSafe;

struct CurrentWorld {
    fixture: Fixture,
    db: Database,
    departure: crate::HubDepartureReceipt,
    recorded: crate::HubReturnReceipt,
}
async fn current_world() -> CurrentWorld {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    record_return(&db, None, 120).await;
    let scope = activate(&db, 140).await;
    let claim = Some(ReturnDepartureClaim {
        departure: departure.record.id,
        group: Some(departure.record.consumed[0].before.id),
    });
    let mut input = return_command(&db, claim, id(2)).await;
    input.identification.label = HubLabel::new("Distinct fresh paired return").unwrap();
    let crate::HubMutationResult::Returned(recorded) = db
        .submit_hub_mutation(
            scope,
            operation(1001),
            HubMutation::Return(Box::new(input)),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("fresh actual return")
    };
    assert!(recorded.original.current_allowances.is_some());
    CurrentWorld {
        fixture,
        db,
        departure,
        recorded: *recorded,
    }
}

async fn activate(db: &Database, time: i64) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(at(time)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(time),
        )
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}

#[tokio::test]
async fn old_none_return_replays_after_activation_but_new_direct_effect_has_no_admission() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let claim = Some(ReturnDepartureClaim {
        departure: departure.record.id,
        group: Some(departure.record.consumed[0].before.id),
    });
    let input = return_command(&db, claim, id(2)).await;
    let old = db
        .apply_hub_return(owner(), operation(100), input.clone(), at(120))
        .await
        .unwrap();
    assert!(old.original.current_allowances.is_none());
    activate(&db, 140).await;
    assert_eq!(
        db.apply_hub_return(owner(), operation(100), input, at(150))
            .await
            .unwrap(),
        old
    );
    let mut new = return_command(&db, claim, id(2)).await;
    new.identification.label = HubLabel::new("Distinct direct return without admission").unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert!(
        db.apply_hub_return(owner(), operation(1001), new, at(150))
            .await
            .is_err()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
}

#[tokio::test]
async fn fresh_return_captures_uncertainty_and_recovers_both_sealed_records_after_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    record_return(&db, None, 120).await;
    let scope = activate(&db, 140).await;
    let claim = Some(ReturnDepartureClaim {
        departure: departure.record.id,
        group: Some(departure.record.consumed[0].before.id),
    });
    let mut input = return_command(&db, claim, id(2)).await;
    input.identification.label = HubLabel::new("Distinct fresh current returned two").unwrap();
    let crate::HubMutationResult::Returned(recorded) = db
        .submit_hub_mutation(
            scope,
            operation(1001),
            HubMutation::Return(Box::new(input.clone())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("recorded returned custody")
    };
    let recorded = *recorded;
    let capture = recorded
        .original
        .current_allowances
        .as_ref()
        .expect("current frozen allowance");
    assert_eq!(capture.lines.len(), 1);
    assert!(capture.lines[0].unresolved_returns);
    assert!(capture.lines[0].quantity_head.is_none());
    assert_eq!(capture.lines[0].effective_departure.value(), 6);
    assert_eq!(capture.lines[0].original_debit_return_bound.value(), 6);
    assert_eq!(
        recorded.original.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    assert_eq!(recorded.original.intake.packs.value(), 2);
    assert_eq!(
        recorded.original.acceptance,
        AcceptanceStatus::ReviewRequired
    );
    let mut raw = fixture.raw().await;
    let pairs:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM hub_correction_captures WHERE operation_id=?),(SELECT count(*) FROM hub_correction_return_projections WHERE operation_id=?)")
        .bind(operation(1001).as_str()).bind(operation(1001).as_str()).fetch_one(&mut raw).await.unwrap();
    assert_eq!(pairs, (1, 1));
    raw.close().await.unwrap();
    assert_eq!(
        db.hub_return_operation(owner(), operation(1001))
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    let fresh = activate(&reopened, 170).await;
    assert_eq!(
        reopened
            .hub_return_operation(owner(), operation(1001))
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    let crate::HubMutationResult::Returned(replay) = reopened
        .submit_hub_mutation(
            fresh,
            operation(1001),
            HubMutation::Return(Box::new(input)),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("same original")
    };
    assert_eq!(*replay, recorded);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn lost_or_damaged_current_evidence_never_falls_back_to_a_legacy_none_original() {
    use crate::hub::corrections::{funding, quantity};
    for fault in 0..5 {
        let world = current_world().await;
        match fault {
            0 => {
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_captures_no_delete",
                    "DELETE FROM hub_correction_captures",
                )
                .await
            }
            1 => {
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_return_projections_no_delete",
                    "DELETE FROM hub_correction_return_projections",
                )
                .await
            }
            2 => {
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_captures_no_delete",
                    "DELETE FROM hub_correction_captures",
                )
                .await;
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_return_projections_no_delete",
                    "DELETE FROM hub_correction_return_projections",
                )
                .await;
            }
            3 => {
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_captures_no_update",
                    "UPDATE hub_correction_captures SET scope_hash=zeroblob(32)",
                )
                .await
            }
            4 => {
                crate::hub::custody::tests::corrupt(
                    &world.fixture,
                    "hub_correction_return_projections_no_update",
                    "UPDATE hub_correction_return_projections SET projection_hash=zeroblob(32)",
                )
                .await
            }
            _ => unreachable!(),
        }
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(1001))
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::StorageCorrupt,
            "fault {fault}"
        );
        let hub = world.db.load_hub().await.unwrap().unwrap().id();
        let target = world.departure.operation.clone();
        let group = world.departure.record.consumed[0].before.id;
        let cut = world.recorded.original.stock_revision;
        let result = world
            .db
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
            harvestcircle_domain::SafeErrorCode::StorageCorrupt,
            "funding fault {fault}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn chained_current_returns_keep_each_original_cut_and_uncertain_custody_after_restart() {
    let world = current_world().await;
    let scope = activate(&world.db, 170).await;
    let claim = Some(ReturnDepartureClaim {
        departure: world.departure.record.id,
        group: Some(world.departure.record.consumed[0].before.id),
    });
    let mut input = return_command(&world.db, claim, id(2)).await;
    input.identification.label = HubLabel::new("Second current paired return").unwrap();
    let crate::HubMutationResult::Returned(second) = world
        .db
        .submit_hub_mutation(
            scope,
            operation(1002),
            HubMutation::Return(Box::new(input.clone())),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("second current return");
    };
    let first = world.recorded.original.current_allowances.as_ref().unwrap();
    let later = second.original.current_allowances.as_ref().unwrap();
    assert!(later.stock_revision.value() > first.stock_revision.value());
    assert_ne!(later.lines[0].funding_hash, first.lines[0].funding_hash);
    assert!(later.lines[0].unresolved_returns);
    assert_eq!(
        second.original.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    assert_eq!(second.original.intake.packs.value(), 2);
    assert_eq!(second.original.acceptance, AcceptanceStatus::ReviewRequired);
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    let scope = activate(&reopened, 190).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        reopened
            .hub_return_operation(owner(), operation(1001))
            .await
            .unwrap(),
        Some(world.recorded)
    );
    assert_eq!(
        reopened
            .hub_return_operation(owner(), operation(1002))
            .await
            .unwrap(),
        Some(*second.clone())
    );
    let crate::HubMutationResult::Returned(replay) = reopened
        .submit_hub_mutation(
            scope,
            operation(1002),
            HubMutation::Return(Box::new(input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("second original recovery");
    };
    assert_eq!(replay, second);
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn internally_resealed_allowance_data_is_not_a_canonical_head_or_funding_proof() {
    use crate::hub::corrections::return_capture;
    use harvestcircle_domain::hub::returns::ReturnQuantityHead;
    for false_head in [false, true] {
        let world = current_world().await;
        let hub = world.db.load_hub().await.unwrap().unwrap().id();
        let mut forged = world.recorded.original.current_allowances.clone().unwrap();
        if false_head {
            forged.lines[0].quantity_head = Some(ReturnQuantityHead {
                operation: id(77),
                stock_revision: forged.stock_revision,
                projection_hash: [7; 32],
            });
        } else {
            forged.lines[0].funding_hash = [8; 32];
        }
        let bytes = return_capture::encode(&forged).unwrap();
        let mut raw = world.fixture.raw().await;
        let result: Vec<u8> =
            sqlx::query_scalar("SELECT result_hash FROM hub_returns WHERE operation_id=?")
                .bind(operation(1001).as_str())
                .fetch_one(&mut raw)
                .await
                .unwrap();
        raw.close().await.unwrap();
        let op = world
            .db
            .host()
            .transaction(|tx| {
                Box::pin(async move { stock::read::operation_on(tx, hub, &operation(1001)).await })
            })
            .await
            .unwrap()
            .unwrap();
        let result: [u8; 32] = result.try_into().unwrap();
        let seal = return_capture::scope_hash(&op, hub, result, &bytes).unwrap();
        let mut raw = world.fixture.raw().await;
        let update =
            "UPDATE hub_correction_captures SET payload=?,scope_hash=? WHERE operation_id=?";
        assert!(
            sqlx::query(update)
                .bind(&bytes)
                .bind(seal.as_slice())
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
            .bind(&bytes)
            .bind(seal.as_slice())
            .bind(op.id.as_str())
            .execute(&mut raw)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
        raw.close().await.unwrap();
        let decoded = world
            .db
            .host()
            .transaction(|tx| {
                Box::pin(async move { return_capture::read_on(tx, hub, &op, result).await })
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            decoded, forged,
            "the capture codec and its own seal are valid"
        );
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(1001))
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
    }
}
