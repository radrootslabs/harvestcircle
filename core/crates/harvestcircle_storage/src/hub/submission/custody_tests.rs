//! Real new-kind admission, native material capture and no-effect session/unknown fences.
use super::*;
use crate::hub::custody::tests::material;
use crate::hub::handling::departure::tests::custody_departure_fixture;
use crate::hub::stock::tests::{Fixture, id, now, operation, other, owner, query};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::SafeErrorCode;
use harvestcircle_domain::hub::{
    departure::DepartureRecord,
    disposition::{DispositionCommand, DispositionKind},
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceAttribution, SourceProvenance, WholePacks},
    returns::{ReturnCommand, ReturnDepartureClaim},
    stock::Occurrence,
};

async fn active(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}
async fn mutation(
    db: &Database,
    departure: &DepartureRecord,
    group: HubRecordId,
    returned: bool,
) -> HubMutation {
    let current = db.hub_group(owner(), group).await.unwrap();
    let note = || SourceProvenance::new("Native admission custody test observation").unwrap();
    if returned {
        HubMutation::Return(Box::new(ReturnCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            variant: id(2),
            location: id(6),
            returned_by: Some(id(9)),
            source: SourceAttribution::Unknown,
            packs: WholePacks::from_value(2).unwrap(),
            identification: PhysicalIdentification {
                label: HubLabel::new("Two actual returned crates").unwrap(),
                observation: note(),
            },
            planned_location: Some(id(7)),
            departure: Some(ReturnDepartureClaim {
                departure: departure.id,
                group: Some(departure.consumed[0].before.id),
            }),
            occurred: Occurrence::Unknown,
            observation: note(),
        }))
    } else {
        HubMutation::Disposition(Box::new(DispositionCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group,
            expected_group_revision: current.group.revision,
            packs: current.group.packs,
            kind: DispositionKind::OtherObservedRemoval,
            occurred: Occurrence::Unknown,
            observation: note(),
        }))
    }
}

#[tokio::test]
async fn return_and_disposition_capture_native_original_context_and_commit_one_admitted_effect() {
    for returned in [true, false] {
        let f = Fixture::new();
        let db = f.open().await;
        let (intake, departure) = custody_departure_fixture(&db).await;
        let group = intake.original.position.group;
        let scope = active(&db).await;
        let input = mutation(&db, &departure.record, group, returned).await;
        let captured_input = input.submitted_request().archive().unwrap();
        let replay_input = match &input {
            HubMutation::Return(c) => HubMutation::Return(c.clone()),
            HubMutation::Disposition(c) => HubMutation::Disposition(c.clone()),
            _ => unreachable!("new custody kinds only"),
        };
        let result = db
            .submit_hub_mutation(scope, operation(100), input, now())
            .await
            .unwrap();
        assert_eq!(returned, matches!(result, HubMutationResult::Returned(_)));
        assert_eq!(!returned, matches!(result, HubMutationResult::Disposed(_)));
        let record = db
            .hub_submission(owner(), operation(100))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, HubSubmissionStatus::Recorded);
        assert_eq!(record.scope, scope);
        let captured: serde_json::Value =
            serde_json::from_slice(record.captured_semantics()).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(captured_input.as_bytes()).unwrap();
        assert_eq!(captured["input"], raw["input"]);
        let material = captured["material"].as_array().unwrap();
        if returned {
            let native = &material.iter().find(|v| v.get("return").is_some()).unwrap()["return"];
            assert_eq!(
                native["departure"],
                serde_json::to_value(&departure.record).unwrap()
            );
            assert_eq!(
                native["established_return_totals"]
                    .as_array()
                    .unwrap()
                    .len(),
                departure.record.consumed.len()
            );
            assert_eq!(native["established_return_totals"][0]["packs"], 0);
        } else {
            let native = &material
                .iter()
                .find(|v| v.get("disposition").is_some())
                .unwrap()["disposition"];
            assert_eq!(native["group"]["packs"], 14);
            assert!(native["lineage"]["nodes"].as_array().unwrap().len() >= 2);
            assert!(
                !native["treatment"]["decisions"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(native["uncertain_submissions"], false);
            assert_eq!(native["unresolved_actuals"], false);
        }
        let rows = crate::hub::custody::tests::material(&f).await;
        // Reuse the caller's exact original command, never a stored-archive decoder.
        let replay = db
            .submit_hub_mutation(scope, operation(100), replay_input, now())
            .await
            .unwrap();
        match (result, replay) {
            (HubMutationResult::Returned(a), HubMutationResult::Returned(b)) => assert_eq!(a, b),
            (HubMutationResult::Disposed(a), HubMutationResult::Disposed(b)) => assert_eq!(a, b),
            _ => panic!("same original kind"),
        }
        assert_eq!(crate::hub::custody::tests::material(&f).await, rows);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn both_new_kinds_reject_stale_foreign_and_signed_out_native_scopes_without_admission() {
    for returned in [true, false] {
        let f = Fixture::new();
        let db = f.open().await;
        let (intake, departure) = custody_departure_fixture(&db).await;
        let group = intake.original.position.group;
        let old = active(&db).await;
        let next = db
            .advance_hub_session(old.runtime, old.generation, Some(other()), now())
            .await
            .unwrap();
        for scope in [
            old,
            HubSubmissionScope {
                generation: next,
                actor: other(),
                ..old
            },
        ] {
            let before = material(&f).await;
            assert_eq!(
                db.submit_hub_mutation(
                    scope,
                    operation(100),
                    mutation(&db, &departure.record, group, returned).await,
                    now()
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubPermissionDenied
            );
            assert_eq!(material(&f).await, before);
        }
        let signed_out = db
            .advance_hub_session(old.runtime, next, None, now())
            .await
            .unwrap();
        let before = material(&f).await;
        assert_eq!(
            db.submit_hub_mutation(
                HubSubmissionScope {
                    generation: signed_out,
                    ..old
                },
                operation(100),
                mutation(&db, &departure.record, group, returned).await,
                now()
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(material(&f).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn interrupted_new_kind_admission_never_renews_execution_and_blocks_other_planned_effects() {
    for returned in [true, false] {
        let f = Fixture::new();
        let db = f.open().await;
        let (intake, departure) = custody_departure_fixture(&db).await;
        let group = intake.original.position.group;
        let scope = active(&db).await;
        let owned = mutation(&db, &departure.record, group, returned).await;
        let decision = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    admission::prepare_on(tx, scope, operation(100), owned, now()).await
                })
            })
            .await
            .unwrap();
        assert!(matches!(decision, admission::Decision::Fresh(_)));
        drop(decision);
        let before = material(&f).await;
        assert_eq!(
            db.hub_submission(owner(), operation(100))
                .await
                .unwrap()
                .unwrap()
                .status,
            HubSubmissionStatus::Pending
        );
        for (op, kind, code) in [
            (100, returned, SafeErrorCode::HubOutcomeUnknown),
            (101, returned, SafeErrorCode::HubConflict),
            (102, !returned, SafeErrorCode::HubConflict),
        ] {
            assert_eq!(
                db.submit_hub_mutation(
                    scope,
                    operation(op),
                    mutation(&db, &departure.record, group, kind).await,
                    now()
                )
                .await
                .unwrap_err()
                .code(),
                code
            );
            assert_eq!(material(&f).await, before);
        }
        assert_eq!(
            db.hub_stock_page(owner(), query())
                .await
                .unwrap()
                .rows
                .iter()
                .map(|r| r.position.packs.value())
                .sum::<u32>(),
            14
        );
        let saved = db
            .hub_submission(owner(), operation(100))
            .await
            .unwrap()
            .unwrap();
        db.close().await.unwrap();
        let db = f.open().await;
        let fresh = active(&db).await;
        assert_ne!(scope.runtime, fresh.runtime);
        let before = material(&f).await;
        assert_eq!(
            db.submit_hub_mutation(
                fresh,
                operation(100),
                mutation(&db, &departure.record, group, returned).await,
                now()
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubOutcomeUnknown
        );
        assert_eq!(
            db.hub_submission(owner(), operation(100))
                .await
                .unwrap()
                .unwrap(),
            saved
        );
        assert_eq!(material(&f).await, before);
        db.close().await.unwrap();
    }
}
