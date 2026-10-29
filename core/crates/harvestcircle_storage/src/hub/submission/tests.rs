use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, now, operation, other, owner, query, setup,
};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubRuntimeIncarnation},
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        counts::{CountObserveCommand, CountPostCommand},
        reference::{
            HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceProvenance,
            WholePacks,
        },
        stock::Occurrence,
    },
};
use serde_json::Value;

fn scope() -> HubSubmissionScope {
    HubSubmissionScope {
        runtime: HubRuntimeIncarnation::from_bytes([7; 16]).unwrap(),
        generation: SessionGeneration::initial(),
        actor: owner(),
    }
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Fictional capture; no admission or stock effect").unwrap()
}
fn count_input(
    catalog: &ReferenceCatalog,
    group: HubRecordId,
    revision: HubRevision,
) -> CountObserveCommand {
    CountObserveCommand {
        expected_stock_revision: revision,
        expected_catalog_revision: catalog.revision(),
        group,
        expected_group_revision: HubRevision::INITIAL,
        counted_packs: WholePacks::from_value(18).unwrap(),
        cutoff: Occurrence::Known(now()),
        observation: note(),
    }
}
fn payload(archive: &ArchivedHubSubmission) -> Value {
    serde_json::from_slice(archive.as_bytes()).unwrap()
}

#[tokio::test]
async fn native_count_capture_retains_original_observation_population_and_work_without_posting() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = before.rows[0].position.group;
    let observed = db
        .observe_hub_count(
            owner(),
            operation(2),
            count_input(&catalog, group, before.revision),
            now(),
        )
        .await
        .unwrap();
    let input = CountPostCommand {
        expected_stock_revision: observed.original.recorded_revision,
        expected_catalog_revision: catalog.revision(),
        count: observed.original.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note(),
    };
    let saved = db
        .capture_hub_submission(
            owner(),
            scope(),
            operation(3),
            HubMutation::CountPosting(Box::new(input.clone())),
        )
        .await
        .unwrap();
    let value = payload(&saved);
    assert_eq!(value["kind"], "count_post");
    let material = value["material"].as_array().unwrap();
    assert!(
        material
            .iter()
            .any(|m| m["count"]["counted_packs"] == 18
                && m["count"]["scope"]["recorded_packs"] == 20)
    );
    assert!(
        material
            .iter()
            .any(|m| m["population"]["original_packs"] == 20
                && m["population"]["origin"] == "receipt")
    );
    assert!(
        material
            .iter()
            .any(|m| m["group"]["packs"] == 20 && m["group"]["work"][0]["packs"] == 20)
    );
    assert!(
        material
            .iter()
            .any(|m| m["work"]["quantity"]["remaining"] == 20)
    );
    assert!(
        material
            .iter()
            .all(|m| m.get("unresolved_actual").is_none() && m.get("uncertain_intent").is_none())
    );
    assert_eq!(
        saved,
        db.capture_hub_submission(
            owner(),
            scope(),
            operation(3),
            HubMutation::CountPosting(Box::new(input))
        )
        .await
        .unwrap()
    );
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.revision, observed.original.recorded_revision);
    assert_eq!(after.rows, before.rows);
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
async fn native_capture_rejects_foreign_and_stale_cuts_and_keeps_original_labels_after_rename() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = receipt_command(&catalog);
    let saved = db
        .capture_hub_submission(
            owner(),
            scope(),
            operation(1),
            HubMutation::Receive(Box::new(receipt.clone())),
        )
        .await
        .unwrap();
    let original = payload(&saved);
    let variant = catalog.get(receipt.variant).unwrap();
    let ReferenceValue::Pack { product, .. } = variant.value else {
        panic!("pack")
    };
    let current = catalog.get(product).unwrap();
    let next = db
        .apply_hub_reference(
            owner(),
            operation(70),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: product,
                    expected_record_revision: current.revision,
                    value: ReferenceValue::Product {
                        label: HubLabel::new("Later fictional apples").unwrap(),
                    },
                },
            },
            now(),
        )
        .await
        .unwrap();
    assert_eq!(
        db.capture_hub_submission(
            owner(),
            scope(),
            operation(1),
            HubMutation::Receive(Box::new(receipt.clone()))
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    let mut fresh = receipt;
    fresh.expected_catalog_revision = next.change.catalog_revision;
    let later = db
        .capture_hub_submission(
            owner(),
            scope(),
            operation(1),
            HubMutation::Receive(Box::new(fresh.clone())),
        )
        .await
        .unwrap();
    assert_ne!(saved.digest(), later.digest());
    assert_eq!(payload(&saved), original);
    assert!(
        !String::from_utf8(saved.as_bytes().to_vec())
            .unwrap()
            .contains("Later fictional apples")
    );
    assert_eq!(
        db.capture_hub_submission(
            other(),
            scope(),
            operation(1),
            HubMutation::Receive(Box::new(fresh.clone()))
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut foreign = scope();
    foreign.actor = other();
    assert_eq!(
        db.capture_hub_submission(
            other(),
            foreign,
            operation(1),
            HubMutation::Receive(Box::new(fresh))
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().revision,
        HubRevision::INITIAL
    );
    db.close().await.unwrap();
}

async fn capture(db: &Database, op: u32, command: HubMutation) -> Value {
    payload(
        &db.capture_hub_submission(owner(), scope(), operation(op), command)
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn native_capture_covers_intake_handling_and_original_follow_up_without_inventing_effects() {
    use harvestcircle_domain::hub::{
        HubId, HubOwnership,
        movement::{
            DiscrepancyAction, DiscrepancyCommand, HandlingCommand, MovementCommand,
            NewWorkCommand, PhysicalIdentification, ReplanCommand,
        },
        portions::SplitCommand,
        reference::SourceAttribution,
        stock::{OpeningCommand, OpeningHistory},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let initial = capture(
        &db,
        90,
        HubMutation::Initialize(Box::new(HubOwnership::new(
            HubId::from_bytes([1; 16]).unwrap(),
            owner(),
        ))),
    )
    .await;
    assert_eq!(initial["kind"], "initialize");
    assert!(db.load_hub().await.unwrap().is_none());
    let catalog = setup(&db).await;
    let input = receipt_command(&catalog);
    let archived = capture(&db, 1, HubMutation::Receive(Box::new(input.clone()))).await;
    assert!(
        archived["material"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["reference"]["id"] == input.variant.to_hex())
    );
    let opening = OpeningCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: input.variant,
        location: input.receiving,
        packs: WholePacks::from_value(8).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new("Observed separate opening crates").unwrap(),
            observation: note(),
        },
        source: SourceAttribution::Unknown,
        history: OpeningHistory::Unknown,
        counted: Occurrence::Known(now()),
        observation: note(),
    };
    assert_eq!(
        capture(&db, 91, HubMutation::Opening(Box::new(opening))).await["kind"],
        "opening"
    );
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    let group = receipt.original.position.group;
    let current = db.hub_group(owner(), group).await.unwrap();
    let target = input.planned_location.unwrap();
    let stock = current.revision;
    let group_revision = current.group.revision;
    let expected_catalog_revision = catalog.revision();
    for (n, command) in [
        (
            92,
            HandlingCommand::Move(MovementCommand {
                expected_stock_revision: stock,
                expected_catalog_revision,
                group,
                expected_group_revision: group_revision,
                to: target,
                packs: current.group.packs,
                separated_portion: None,
                occurred: Occurrence::Known(now()),
                observation: note(),
            }),
        ),
        (
            93,
            HandlingCommand::Split(SplitCommand {
                expected_stock_revision: stock,
                expected_catalog_revision,
                group,
                expected_group_revision: group_revision,
                packs: WholePacks::from_value(8).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Observed tagged eight").unwrap(),
                    observation: note(),
                },
                occurred: Occurrence::Known(now()),
                observation: note(),
            }),
        ),
        (
            94,
            HandlingCommand::NewWork(NewWorkCommand {
                expected_stock_revision: stock,
                expected_catalog_revision,
                groups: vec![group],
                target,
                observation: note(),
            }),
        ),
        (
            95,
            HandlingCommand::Replan(ReplanCommand {
                expected_stock_revision: stock,
                expected_catalog_revision,
                requirement: current.group.work[0].requirement.id,
                expected_work_revision: current.group.work[0].requirement.revision,
                target,
                observation: note(),
            }),
        ),
    ] {
        let value = capture(&db, n, HubMutation::Handling(Box::new(command))).await;
        assert!(
            value["material"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["group"]["id"] == group.to_hex())
        );
    }
    let discrepancy = DiscrepancyCommand {
        expected_stock_revision: stock,
        expected_catalog_revision,
        action: DiscrepancyAction::Open {
            trace: receipt.original.trace.id,
            group: Some(group),
            observation: note(),
        },
    };
    assert_eq!(
        capture(
            &db,
            96,
            HubMutation::Handling(Box::new(HandlingCommand::Discrepancy(discrepancy.clone())))
        )
        .await["kind"],
        "discrepancy_open"
    );
    let opened = db
        .apply_hub_discrepancy(owner(), operation(96), discrepancy, now())
        .await
        .unwrap();
    let handling::HubHandlingResult::DiscrepancyChanged(record) = opened.result else {
        panic!("discrepancy")
    };
    let resolve = DiscrepancyCommand {
        expected_stock_revision: opened.revision,
        expected_catalog_revision,
        action: DiscrepancyAction::Resolve {
            discrepancy: record.id,
            expected_revision: record.revision,
            resolution: note(),
        },
    };
    let archived = capture(
        &db,
        97,
        HubMutation::Handling(Box::new(HandlingCommand::Discrepancy(resolve))),
    )
    .await;
    assert!(
        archived["material"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["discrepancy"]["observation"] == note().as_str())
    );
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.revision, opened.revision);
    assert_eq!(after.rows[0].position.packs.value(), 20);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_hold_and_dispatch_captures_keep_exact_original_scopes_and_issued_material() {
    use harvestcircle_domain::hub::{
        departure::DepartureCommand,
        dispatch::{DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection},
        movement::HandlingCommand,
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let catalog = db.hub_references(owner()).await.unwrap();
    let recipient = HubRecordId::from_bytes([8; 16]).unwrap();
    let create = ReferenceCommand {
        expected_catalog_revision: catalog.revision(),
        mutation: ReferenceMutation::Create {
            id: recipient,
            value: ReferenceValue::Party {
                label: HubLabel::new("Fictional recipient").unwrap(),
            },
        },
    };
    assert_eq!(
        capture(&db, 50, HubMutation::Reference(Box::new(create.clone()))).await["kind"],
        "reference"
    );
    db.apply_hub_reference(owner(), operation(50), create, now())
        .await
        .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let stock = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let group = stock.original.position.group;
    let hold_input = TreatmentCommand {
        expected_stock_revision: stock.original.stock_revision,
        expected_catalog_revision: catalog.revision(),
        action: TreatmentAction::PlaceHold {
            selection: TreatmentSelection::new(vec![group]).unwrap(),
        },
        occurred: Occurrence::Known(now()),
        observation: note(),
    };
    assert_eq!(
        capture(
            &db,
            2,
            HubMutation::Handling(Box::new(HandlingCommand::Treatment(hold_input.clone())))
        )
        .await["kind"],
        "hold_place"
    );
    let held = db
        .apply_hub_treatment(owner(), operation(2), hold_input, now())
        .await
        .unwrap();
    let handling::HubHandlingResult::Treatment(result) = held.result else {
        panic!("treatment")
    };
    let harvestcircle_domain::hub::treatment::TreatmentChange::HoldPlaced(hold) = result.change
    else {
        panic!("hold")
    };
    let release = TreatmentCommand {
        expected_stock_revision: held.revision,
        expected_catalog_revision: catalog.revision(),
        action: TreatmentAction::ReleaseHold {
            hold: hold.stamp.id,
            expected_revision: hold.revision(),
        },
        occurred: Occurrence::Known(now()),
        observation: note(),
    };
    let value = capture(
        &db,
        3,
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(release))),
    )
    .await;
    assert!(
        value["material"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["hold"]["scope"]["anchors"][0]["captured_packs"] == 20
                && m["hold"]["stamp"]["basis"]["id"] == "whole-pack-demo-v1")
    );
    let prepare = DispatchCommand {
        expected_stock_revision: held.revision,
        expected_catalog_revision: catalog.revision(),
        action: DispatchAction::Prepare {
            recipient,
            lines: DispatchSelection::new(vec![DispatchLineInput {
                group,
                expected_group_revision: HubRevision::INITIAL,
                packs: WholePacks::from_value(20).unwrap(),
            }])
            .unwrap(),
        },
        observation: note(),
    };
    assert_eq!(
        capture(&db, 4, HubMutation::Dispatch(Box::new(prepare.clone()))).await["kind"],
        "dispatch_prepare"
    );
    let prepared = db
        .apply_hub_dispatch(owner(), operation(4), prepare, now())
        .await
        .unwrap();
    let issue = DispatchCommand {
        expected_stock_revision: prepared.record.updated.stock_revision,
        expected_catalog_revision: catalog.revision(),
        action: DispatchAction::Issue {
            preparation: prepared.record.id,
            expected_revision: prepared.record.revision,
        },
        observation: note(),
    };
    assert_eq!(
        capture(&db, 5, HubMutation::Dispatch(Box::new(issue.clone()))).await["kind"],
        "dispatch_issue"
    );
    let issued = db
        .apply_hub_dispatch(owner(), operation(5), issue, now())
        .await
        .unwrap();
    let departure = DepartureCommand {
        expected_stock_revision: issued.record.updated.stock_revision,
        expected_catalog_revision: catalog.revision(),
        preparation: issued.record.id,
        expected_preparation_revision: issued.record.revision,
        identification: issued.record.versions.last().unwrap().id,
        occurred: Occurrence::Known(now()),
        observation: note(),
    };
    let value = capture(&db, 6, HubMutation::Departure(Box::new(departure))).await;
    assert!(
        value["material"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["dispatch"]["identification"]["material"]["lines"][0]["packs"] == 20)
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().revision,
        issued.record.updated.stock_revision
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn posting_capture_keeps_observation_labels_separate_from_the_current_catalog_cut() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let count = db
        .observe_hub_count(
            owner(),
            operation(2),
            count_input(&catalog, page.rows[0].position.group, page.revision),
            now(),
        )
        .await
        .unwrap()
        .original;
    let old_label = count.scope.product.value.label().as_str().to_owned();
    let change = db
        .apply_hub_reference(
            owner(),
            operation(70),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: count.scope.product.id,
                    expected_record_revision: count.scope.product.revision,
                    value: ReferenceValue::Product {
                        label: HubLabel::new("Current posting-cut apples").unwrap(),
                    },
                },
            },
            now(),
        )
        .await
        .unwrap();
    let value = capture(
        &db,
        3,
        HubMutation::CountPosting(Box::new(CountPostCommand {
            expected_stock_revision: count.recorded_revision,
            expected_catalog_revision: change.change.catalog_revision,
            count: count.id,
            reviewed_activity: true,
            surplus_identification: None,
            reason: note(),
        })),
    )
    .await;
    let material = value["material"].as_array().unwrap();
    assert!(
        material
            .iter()
            .any(|m| m["count"]["scope"]["product"]["value"]["product"]["label"] == old_label)
    );
    assert!(
        material
            .iter()
            .any(|m| m["reference"]["id"] == count.scope.product.id.to_hex()
                && m["reference"]["value"]["product"]["label"] == "Current posting-cut apples")
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().revision,
        count.recorded_revision
    );
    db.close().await.unwrap();
}
