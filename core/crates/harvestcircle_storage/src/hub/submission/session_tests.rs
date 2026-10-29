//! Actual protected native scope lifecycle; fixture SQL is never a session API.
use super::*;
use crate::hub::stock::tests::{Fixture, now, other, owner, setup};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::{SafeErrorCode, UnixTimestamp};

#[tokio::test]
async fn current_scope_advances_on_switch_and_signout_and_reopen_starts_with_another_incarnation() {
    let f = Fixture::new();
    let db = f.open().await;
    setup(&db).await;
    assert!(db.hub_runtime_session().await.unwrap().is_none());
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let initial = db.hub_runtime_session().await.unwrap().unwrap();
    assert_eq!(initial.runtime, runtime);
    assert_eq!(initial.generation, SessionGeneration::initial());
    assert_eq!(initial.actor, None);
    let generation = db
        .advance_hub_session(runtime, initial.generation, Some(owner()), now())
        .await
        .unwrap();
    assert_eq!(generation.value(), 1);
    let switched = db
        .advance_hub_session(runtime, generation, Some(other()), now())
        .await
        .unwrap();
    assert_eq!(switched.value(), 2);
    assert_eq!(
        db.hub_runtime_session().await.unwrap().unwrap().actor,
        Some(other())
    );
    let signed_out = db
        .advance_hub_session(runtime, switched, None, now())
        .await
        .unwrap();
    assert_eq!(signed_out.value(), 3);
    assert_eq!(db.hub_runtime_session().await.unwrap().unwrap().actor, None);
    assert_eq!(
        db.advance_hub_session(runtime, generation, Some(owner()), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
    let db = f.open().await;
    let restarted = db
        .begin_hub_runtime(UnixTimestamp::from_seconds(99).unwrap())
        .await
        .unwrap();
    assert_ne!(runtime, restarted);
    let current = db.hub_runtime_session().await.unwrap().unwrap();
    assert_eq!(current.generation, SessionGeneration::initial());
    assert_eq!(current.actor, None);
    assert_eq!(
        db.advance_hub_session(runtime, signed_out, Some(owner()), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(db.load_hub().await.unwrap().unwrap().owner(), owner());
    db.close().await.unwrap();
}

#[tokio::test]
async fn generation_keeps_the_full_unsigned_range_and_overflow_cannot_replace_the_session() {
    let f = Fixture::new();
    let db = f.open().await;
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                sqlx::query("UPDATE hub_runtime_session SET generation=? WHERE singleton=1")
                    .bind((u64::MAX - 1).to_be_bytes().as_slice())
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::from_value(u64::MAX - 1),
            Some(owner()),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(generation.value(), u64::MAX);
    assert_eq!(
        db.hub_runtime_session()
            .await
            .unwrap()
            .unwrap()
            .generation
            .value(),
        u64::MAX
    );
    assert_eq!(
        db.advance_hub_session(runtime, generation, None, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    assert_eq!(
        db.hub_runtime_session().await.unwrap().unwrap().actor,
        Some(owner())
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn an_active_native_fence_rejects_missing_admission_and_cannot_be_deleted_or_rewound() {
    use crate::hub::stock::tests::{command, operation, query};
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    assert!(
        db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
            .await
            .is_err()
    );
    let stock = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(stock.revision, HubRevision::INITIAL);
    assert!(stock.rows.is_empty());
    for sql in [
        "DELETE FROM hub_runtime_session WHERE singleton=1",
        "UPDATE hub_runtime_session SET generation=zeroblob(8) WHERE singleton=1",
    ] {
        assert!(
            db.host()
                .transaction(|tx| Box::pin(async move {
                    sqlx::query(sql)
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| storage_unavailable())?;
                    Ok::<_, SafeError>(())
                }))
                .await
                .is_err()
        );
    }
    assert_eq!(
        db.hub_runtime_session().await.unwrap().unwrap().generation,
        generation
    );
    db.close().await.unwrap();
}
