//! Synthetic output rows are metadata fixtures, never executable admissions.
use super::*;
use crate::hub::corrections::quantity::{self, QuantityProjection};
use harvestcircle_domain::hub::corrections::{DepartureQuantityAmendment, DepartureQuantityDelta};
mod allowance_tests;

pub(super) async fn insert_projection(world: &World, broken: bool) {
    insert_projection_packs(world, broken, world.projection.group.packs).await;
}

async fn insert_projection_packs(world: &World, broken: bool, packs: WholePacks) {
    let hub = world.hub;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let group = world.projection.target_group;
    let cut = world.op.revision;
    let original = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move { quantity::original_on(tx, hub, &target, group, cut).await })
        })
        .await
        .unwrap();
    let change = DepartureQuantityAmendment {
        operation: world.projection.birth.operation,
        stock_revision: cut,
        before: original.original.packs,
        after: original.original.packs.checked_sub(packs).unwrap(),
        delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs),
    };
    let value = QuantityProjection {
        original,
        amendment: change,
        previous: None,
        report: None,
        observation: Some(world.projection.observation_operation),
    };
    let mut seal = quantity::seal(&world.op, hub, &value).unwrap();
    if broken {
        seal[0] ^= 1;
    }
    let mut raw = world.fixture.raw().await;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    sqlx::query("INSERT INTO hub_correction_quantity_changes (operation_id,target_operation_id,group_id,before_packs,after_packs,delta_kind,delta_packs,projection_hash) VALUES (?,?,?,?,?,'found',?,?)")
        .bind(world.op.id.as_str()).bind(target.as_str()).bind(group.as_bytes().as_slice()).bind(i64::from(change.before.value()))
        .bind(i64::from(change.after.value())).bind(i64::from(packs.value())).bind(seal.as_slice()).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
}

async fn history(world: &World, cut: HubRevision) -> Result<quantity::QuantityHistory, SafeError> {
    let hub = world.hub;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let group = world.projection.target_group;
    world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move { quantity::history_on(tx, hub, &target, group, cut).await })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn sealed_found_quantity_history_keeps_originals_and_exact_historical_cuts() {
    for actual in [false, true] {
        let world = seed_debit(false, false, actual).await;
        insert_projection(&world, false).await;
        let zero = WholePacks::from_value(0).unwrap();
        let old = history(&world, world.before.revision())
            .await
            .unwrap()
            .with_verified_returns(zero)
            .unwrap();
        assert!(old.amendments().is_empty());
        assert_eq!(old.effective(), old.original().packs);
        let account = history(&world, world.op.revision)
            .await
            .unwrap()
            .with_verified_returns(zero)
            .unwrap();
        assert_eq!(account.original(), old.original());
        assert_eq!(account.amendments().len(), 1);
        assert_eq!(
            account.effective(),
            old.effective()
                .checked_sub(world.projection.group.packs)
                .unwrap()
        );
        assert_eq!(account.found_remainder(), world.projection.group.packs);
        assert_eq!(account.additional_departure(), zero);
        if actual {
            assert_eq!(account.effective(), zero);
        }
        let too_many = account
            .effective()
            .checked_add(WholePacks::from_value(1).unwrap())
            .unwrap();
        assert!(
            history(&world, world.op.revision)
                .await
                .unwrap()
                .with_verified_returns(too_many)
                .is_err()
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_or_broken_quantity_projection_rejects_without_changing_rows() {
    for broken in [false, true] {
        let world = seed(false, false).await;
        if broken {
            insert_projection(&world, true).await;
        }
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            history(&world, world.op.revision)
                .await
                .err()
                .unwrap()
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

#[tokio::test]
async fn mixed_found_and_additional_heads_keep_the_original_and_each_cut() {
    let world = seed_options(false, false, false, 0, true).await;
    insert_projection(&world, false).await;
    let hub = world.hub;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let group = world.projection.target_group;
    let cut = world.op.revision;
    let original = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move { quantity::original_on(tx, hub, &target, group, cut).await })
        })
        .await
        .unwrap();
    let revision = HubRevision::from_value(cut.value().checked_add(1).unwrap()).unwrap();
    let op = OperationRecord {
        id: operation(210),
        kind: "correction_additional".into(),
        actor: owner(),
        hash: [44; 32],
        catalog_revision: world.op.catalog_revision,
        revision,
        result: id(210),
        recorded_at: at(210),
    };
    let before = original
        .original
        .packs
        .checked_sub(world.projection.group.packs)
        .unwrap();
    let value = QuantityProjection {
        original: original.clone(),
        amendment: DepartureQuantityAmendment {
            operation: crate::hub::actuals::common::operation_id(&op.id).unwrap(),
            stock_revision: revision,
            before,
            after: original.original.packs,
            delta: DepartureQuantityDelta::AdditionalDeparture(world.projection.group.packs),
        },
        previous: Some(world.projection.birth.operation),
        report: world.extra_report,
        observation: None,
    };
    let seal = quantity::seal(&op, hub, &value).unwrap();
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let mut raw = world.fixture.raw().await;
    // Copy only fixture custody coordinates. The invalid archived body supplies
    // no fresh admission and these scalar fixtures never post physical effects.
    sqlx::query("INSERT INTO hub_submissions (operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) SELECT ?,hub_id,actor_public_key,'correction_additional',runtime_id,generation,input_hash,?,capture_schema,capture_body,capture_hash,210 FROM hub_submissions WHERE operation_id=?")
        .bind(op.id.as_str()).bind(op.hash.as_slice()).bind(world.op.id.as_str()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,'correction_additional',?,?,?,?,210)")
        .bind(op.id.as_str()).bind(hub.as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(op.hash.as_slice()).bind(i64::try_from(op.catalog_revision.value()).unwrap())
        .bind(i64::try_from(revision.value()).unwrap()).bind(op.result.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,report_id,payload_schema,payload,result_hash) VALUES (?,?,'additional',?,'line',?,?,?,1,?,?)")
        .bind(op.result.as_bytes().as_slice()).bind(op.id.as_str()).bind(target.as_str()).bind(group.as_bytes().as_slice()).bind(world.op.id.as_str())
        .bind(world.extra_report.unwrap().as_bytes().as_slice()).bind(b"{}".as_slice()).bind([55_u8;32].as_slice()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_correction_quantity_changes (operation_id,target_operation_id,group_id,before_packs,after_packs,delta_kind,delta_packs,projection_hash) VALUES (?,?,?,?,?,'additional',?,?)")
        .bind(op.id.as_str()).bind(target.as_str()).bind(group.as_bytes().as_slice()).bind(i64::from(value.amendment.before.value())).bind(i64::from(value.amendment.after.value()))
        .bind(i64::from(world.projection.group.packs.value())).bind(seal.as_slice()).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let zero = WholePacks::from_value(0).unwrap();
    let first = history(&world, cut)
        .await
        .unwrap()
        .with_verified_returns(zero)
        .unwrap();
    assert_eq!(first.amendments().len(), 1);
    assert_eq!(first.effective(), before);
    let latest = history(&world, revision)
        .await
        .unwrap()
        .with_verified_returns(zero)
        .unwrap();
    assert_eq!(latest.original(), original.original);
    assert_eq!(latest.amendments().len(), 2);
    assert_eq!(latest.effective(), original.original.packs);
    assert_eq!(latest.found_remainder(), world.projection.group.packs);
    assert_eq!(latest.additional_departure(), world.projection.group.packs);
    let captured = allowance_tests::load(&world, revision).await.unwrap();
    let capture = captured
        .allowances
        .expect("net-zero amendments keep their native heads");
    let line = &capture.lines[0];
    assert_eq!(line.effective_departure, original.original.packs);
    assert_eq!(line.original_debit_return_bound, before);
    assert_eq!(
        line.quantity_head.unwrap().operation,
        value.amendment.operation
    );
    let material = crate::hub::custody::tests::material(&world.fixture).await;
    let target = crate::hub::actuals::common::operation(world.projection.target_operation).unwrap();
    let full = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                crate::hub::corrections::target_funding::load_on(tx, hub, &target, group, revision)
                    .await
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        full.err().unwrap().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt,
        "scalar additional metadata is not a complete physical debit proof"
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        material
    );
    assert_eq!(
        world
            .db
            .hub_stock_page(owner(), crate::hub::stock::tests::query())
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
}

#[tokio::test]
async fn internally_sealed_found_delta_cannot_exceed_its_preserved_observed_allocations() {
    let world = seed(false, false).await;
    insert_projection_packs(&world, false, WholePacks::from_value(3).unwrap()).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        history(&world, world.op.revision)
            .await
            .err()
            .unwrap()
            .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
}
