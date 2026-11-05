use super::*;
use crate::hub::stock::tests::{Fixture, command, id, operation, owner, setup};
use harvestcircle_application::SessionGeneration;
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        counts::{CountObserveCommand, CountPostCommand},
        movement::PhysicalIdentification,
        reference::{HubLabel, SourceAttribution, SourceProvenance, WholePacks},
        returns::{ReturnCommand, ReturnDepartureClaim, ReturnLinkResolution},
        stock::Occurrence,
    },
};
use sqlx::Connection;

// This world intentionally has no raw actual report: an unresolved report
// correctly blocks later native physical mutations, including a Return.
async fn counted_world() -> (Fixture, Database, HubRecordId, HubRecordId) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let at = |n| UnixTimestamp::from_seconds(n).unwrap();
    let note = || SourceProvenance::new("Independent fictional counted shortage").unwrap();
    let runtime = db.begin_hub_runtime(at(100)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(100),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let crate::HubMutationResult::Received(intake) = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(command(&catalog))),
            at(100),
        )
        .await
        .unwrap()
    else {
        panic!("intake")
    };
    let group = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: group.group.id,
                expected_group_revision: group.group.revision,
                counted_packs: WholePacks::from_value(18).unwrap(),
                cutoff: Occurrence::Known(at(110)),
                observation: note(),
            })),
            at(120),
        )
        .await
        .unwrap()
    else {
        panic!("count")
    };
    let reviewed = db
        .hub_count_review(owner(), observed.original.id)
        .await
        .unwrap();
    let crate::HubMutationResult::CountPosted(posted) = db
        .submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::CountPosting(Box::new(CountPostCommand {
                expected_stock_revision: reviewed.stock_revision,
                expected_catalog_revision: reviewed.catalog_revision,
                count: observed.original.id,
                reviewed_activity: true,
                surplus_identification: None,
                reason: note(),
            })),
            at(130),
        )
        .await
        .unwrap()
    else {
        panic!("post count")
    };
    (fixture, db, posted.original.id, group.group.id)
}

async fn read(
    db: &Database,
    reconciliation: HubRecordId,
    cut: HubRevision,
) -> Result<Returns, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                let target: String = sqlx::query_scalar(
                    "SELECT operation_id FROM hub_count_reconciliations WHERE reconciliation_id=?",
                )
                .bind(reconciliation.as_bytes().as_slice())
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
                let target = DurableRequestId::parse(target).map_err(|_| corrupt_storage())?;
                let op = stock::read::operation_on(tx, hub, &target)
                    .await?
                    .ok_or_else(corrupt_storage)?;
                let mut reader = OriginalReader::new(tx);
                let count = Box::pin(crate::hub::counts::posting::original_on(
                    &mut reader,
                    hub,
                    op,
                ))
                .await?
                .original;
                let history =
                    super::super::count_account::history_on(&mut reader, hub, &count, cut).await?;
                for_history_on(&mut reader, hub, &history, cut).await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn unresolved_count_claim_cannot_disappear_when_its_native_return_body_is_missing() {
    let (fixture, db, reconciliation, source_group) = counted_world().await;
    let current = db.hub_group(owner(), source_group).await.unwrap();
    let cut = current.revision;
    assert!(!read(&db, reconciliation, cut).await.unwrap().unresolved);
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let note = || {
        SourceProvenance::new(
            "Fictional returned custody; counted-departure claim is unresolved evidence",
        )
        .unwrap()
    };
    // No native count explanation is posted by this preactivation test. The
    // explicit count-record claim remains unverified factual input; only the
    // Return itself is a genuine owned native mutation.
    let claim = ReturnDepartureClaim {
        departure: count_explanation_id(
            crate::hub::actuals::common::operation_id(&operation(800)).unwrap(),
        )
        .unwrap(),
        group: Some(source_group),
    };
    let crate::HubMutationResult::Returned(returned) = db
        .submit_hub_mutation(
            scope,
            operation(900),
            HubMutation::Return(Box::new(ReturnCommand {
                expected_stock_revision: cut,
                expected_catalog_revision: current.catalog_revision,
                variant: id(2),
                location: id(6),
                returned_by: None,
                source: SourceAttribution::Unknown,
                packs: WholePacks::from_value(1).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Independent restricted returned crate").unwrap(),
                    observation: note(),
                },
                planned_location: None,
                departure: Some(claim),
                occurred: Occurrence::Unknown,
                observation: note(),
            })),
            UnixTimestamp::from_seconds(200).unwrap(),
        )
        .await
        .unwrap()
    else {
        panic!("return")
    };
    assert_eq!(returned.original.linkage.claim, Some(claim));
    assert_eq!(
        returned.original.linkage.resolution,
        ReturnLinkResolution::Unrecorded
    );
    let included = returned.original.stock_revision;
    let state = read(&db, reconciliation, included).await.unwrap();
    assert!(state.unresolved);
    assert!(state.totals.is_empty());
    let mut raw = fixture.raw().await;
    let guard: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='hub_returns_no_delete'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    assert!(
        sqlx::query("DELETE FROM hub_returns WHERE operation_id=?")
            .bind(operation(900).as_str())
            .execute(&mut raw)
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER hub_returns_no_delete")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("DELETE FROM hub_returns WHERE operation_id=?")
            .bind(operation(900).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    // Restore the exact validated fresh-fixture guard before product reads.
    sqlx::query(sqlx::AssertSqlSafe(guard))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert!(!read(&db, reconciliation, cut).await.unwrap().unresolved);
    assert_eq!(
        read(&db, reconciliation, included)
            .await
            .err()
            .unwrap()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
