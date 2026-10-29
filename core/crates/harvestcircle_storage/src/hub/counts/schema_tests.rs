//! Actual protected SQL guard probes. These deliberately roll back: no count
//! posting handler, current uncertainty source or product journey is qualified.
use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, now, operation, owner, query, setup,
};
use harvestcircle_domain::hub::counts::{
    CountContext, CountPostCommand, CountPostPlan, count_post_result_digest,
};
use harvestcircle_domain::hub::reference::WholePacks;
use harvestcircle_domain::hub::stock::Occurrence;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

async fn world() -> (Fixture, Database, CountPostPlan, [u8; 32]) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = db
        .hub_group(owner(), page.rows[0].position.group)
        .await
        .unwrap()
        .group;
    let observation = db
        .observe_hub_count(
            owner(),
            operation(2),
            CountObserveCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: catalog.revision(),
                group: group.id,
                expected_group_revision: group.revision,
                counted_packs: WholePacks::from_value(18).unwrap(),
                cutoff: Occurrence::Known(now()),
                observation: SourceProvenance::new(
                    "Fictional count for a rolled-back schema guard probe",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap()
        .original;
    let context = CountContext {
        hub: observation.scope.hub,
        stock_revision: observation.recorded_revision,
        catalog: &catalog,
        group: &group,
        latest_physical_recorded_at: now(),
        previous_reconciliation: None,
        latest_observation: Some(observation.id),
        // Test-owned policy inputs, never a production native activity adapter.
        unresolved_actual: false,
        uncertain_intent: false,
    };
    let input = CountPostCommand {
        expected_stock_revision: context.stock_revision,
        expected_catalog_revision: catalog.revision(),
        count: observation.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: SourceProvenance::new("Probe only; no product posting command is admitted")
            .unwrap(),
    };
    let op = operation_id(&operation(3)).unwrap();
    let hash = input.fingerprint(context.hub, owner(), op);
    let plan = input
        .plan(&observation, &context, op, owner(), now())
        .unwrap();
    (fixture, db, plan, hash)
}

async fn header_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    plan: &CountPostPlan,
    hash: [u8; 32],
) -> Result<(), SafeError> {
    let op = operation(3);
    handling::write::insert_operation_on(
        tx,
        plan.count.scope.hub,
        &stock::read::OperationRecord {
            id: op.clone(),
            kind: "count_post".into(),
            actor: owner(),
            hash,
            catalog_revision: plan.catalog_revision,
            revision: plan.stock_revision,
            result: plan.id,
            recorded_at: plan.recorded_at,
        },
    )
    .await?;
    let digest = count_post_result_digest(&hash, plan).unwrap();
    sqlx::query(concat!(
        "INSERT INTO hub_count_reconciliations (reconciliation_id,operation_id,count_id,policy_version,effect_kind,changed_packs,after_on_hand_packs,after_group_revision,",
        "before_causal_floor_unix_s,confirmed_on_hand_at_unix_s,scope_work_count,changed_work_count,surplus_population_id,reason,result_hash) ",
        "VALUES (?,?,?,1,'shortage',2,18,2,?,?,1,1,NULL,?,?)"
    )).bind(plan.id.as_bytes().as_slice()).bind(op.as_str()).bind(plan.count.id.as_bytes().as_slice())
        .bind(plan.before.causal_floor.map(UnixTimestamp::as_seconds)).bind(plan.confirmed_on_hand_at.as_seconds())
        .bind(plan.reason.as_str()).bind(digest.as_slice()).execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    Ok(())
}

async fn work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    plan: &CountPostPlan,
    completed: i64,
    revision: i64,
    removed_progress: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(concat!(
        "INSERT INTO hub_count_reconciliation_work (reconciliation_id,ordinal,work_id,before_progress,before_scoped_packs,on_hand_after,removed_packs,removed_progress,",
        "required,before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision) ",
        "VALUES (?,0,?,'pending',20,18,2,?,20,0,0,20,?,?,2,18,2)"
    )).bind(plan.id.as_bytes().as_slice()).bind(plan.before.work[0].requirement.id.as_bytes().as_slice())
        .bind(removed_progress).bind(revision).bind(completed).execute(&mut *tx).await?;
    Ok(())
}

#[tokio::test]
async fn protected_schema_accepts_original_count_scope_and_rolls_back_the_whole_probe() {
    let (fixture, db, plan, hash) = world().await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let seen = Arc::new(AtomicBool::new(false));
    let witness = Arc::clone(&seen);
    let failed: Result<(), _> = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                header_on(tx, &plan, hash).await?;
                work_on(tx, &plan, 0, 1, "no_longer_required")
                    .await
                    .map_err(|_| storage_unavailable())?;
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT count(*) FROM hub_count_reconciliation_work"
                    )
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap(),
                    1
                );
                witness.store(true, Ordering::SeqCst);
                Err(storage_unavailable())
            })
        })
        .await;
    assert!(failed.is_err());
    assert!(seen.load(Ordering::SeqCst));
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    let mut raw = fixture.raw().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hub_count_reconciliations")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        0
    );
    sqlx::Connection::close(raw).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn protected_schema_rejects_invented_completion_stale_work_and_pending_removed_scope() {
    let (_fixture, db, plan, hash) = world().await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    for (completed, revision, progress) in [
        (2, 1, "no_longer_required"),
        (0, 2, "no_longer_required"),
        (0, 1, "pending"),
    ] {
        let plan = plan.clone();
        let seen = Arc::new(AtomicBool::new(false));
        let witness = Arc::clone(&seen);
        let failed: Result<(), _> = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    header_on(tx, &plan, hash).await?;
                    assert!(
                        work_on(tx, &plan, completed, revision, progress)
                            .await
                            .is_err()
                    );
                    witness.store(true, Ordering::SeqCst);
                    Err(storage_unavailable())
                })
            })
            .await;
        assert!(failed.is_err());
        assert!(seen.load(Ordering::SeqCst));
    }
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    db.close().await.unwrap();
}
