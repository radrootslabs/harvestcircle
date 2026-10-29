use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receiving, now, operation, other, owner, query, setup,
};
use crate::{HubLineageQuery, HubTraceSnapshotQuery};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        counts::CountObserveCommand,
        reference::{LocationPurpose, ReferenceCatalog, ReferenceValue},
        stock::PopulationOrigin,
    },
};

async fn observation(
    db: &Database,
    catalog: &ReferenceCatalog,
    counted: u32,
) -> HubCountObservationReceipt {
    observation_at(db, catalog, counted, 2).await
}

async fn observation_at(
    db: &Database,
    catalog: &ReferenceCatalog,
    counted: u32,
    identity: u32,
) -> HubCountObservationReceipt {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = db
        .hub_group(owner(), page.rows[0].position.group)
        .await
        .unwrap();
    db.observe_hub_count(
        owner(),
        operation(identity),
        CountObserveCommand {
            expected_stock_revision: page.revision,
            expected_catalog_revision: catalog.revision(),
            group: group.group.id,
            expected_group_revision: group.group.revision,
            counted_packs: WholePacks::from_value(counted).unwrap(),
            cutoff: Occurrence::Known(now()),
            observation: SourceProvenance::new(
                "Observed a fictional count for native reconciliation",
            )
            .unwrap(),
        },
        now(),
    )
    .await
    .unwrap()
}

async fn input(db: &Database, count: &CountObservation) -> CountPostCommand {
    let view = db.hub_count_review(owner(), count.id).await.unwrap();
    CountPostCommand {
        expected_stock_revision: view.stock_revision,
        expected_catalog_revision: view.catalog_revision,
        count: count.id,
        reviewed_activity: true,
        surplus_identification: if count.counted_packs.value() > count.scope.recorded_packs.value()
        {
            Some(PhysicalIdentification {
                label: HubLabel::new("Separately tagged counted extras").unwrap(),
                observation: SourceProvenance::new("Extras are distinct from the original twenty")
                    .unwrap(),
            })
        } else {
            None
        },
        reason: SourceProvenance::new(
            "Reviewed current scope and activity; original count retained",
        )
        .unwrap(),
    }
}

async fn snapshot(
    db: &Database,
    trace: HubRecordId,
) -> harvestcircle_domain::hub::trace::TraceSnapshot {
    db.hub_trace_snapshot(
        owner(),
        HubTraceSnapshotQuery {
            trace,
            expected_revision: None,
            expected_catalog_revision: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn native_shortage_posts_eighteen_retires_two_pending_and_preserves_original_twenty_over_replay_and_reopen()
 {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let count = observation(&db, &catalog, 18).await;
    let command = input(&db, &count.original).await;
    let posted = db
        .apply_hub_count_post(owner(), operation(3), command.clone(), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].position.packs.value(), 18);
    let work = page.rows[0].put_away.as_ref().unwrap();
    assert_eq!(work.quantity.required().value(), 20);
    assert_eq!(work.quantity.completed().value(), 0);
    assert_eq!(work.quantity.no_longer_required().value(), 2);
    assert_eq!(work.quantity.remaining().value(), 18);
    let trace = snapshot(&db, receipt.original.trace.id).await;
    let balance = trace.balance().unwrap();
    assert_eq!(balance.original.value(), 20);
    assert_eq!(balance.on_hand.value(), 18);
    assert_eq!(balance.departed.value(), 0);
    assert_eq!(balance.counted_loss.value(), 2);
    assert!(trace.exits.is_empty());
    assert_eq!(trace.count_losses.len(), 1);
    assert_eq!(
        db.apply_hub_count_post(owner(), operation(3), command.clone(), now())
            .await
            .unwrap(),
        posted
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(count.clone())
    );
    let mut changed = command;
    changed.reason = SourceProvenance::new("Changed original intent").unwrap();
    assert_eq!(
        db.apply_hub_count_post(owner(), operation(3), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_count_post_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(posted)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    assert_eq!(snapshot(&db, receipt.original.trace.id).await, trace);
    assert_eq!(
        db.hub_count_post_operation(other(), operation(3))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_zero_count_retains_an_empty_historical_group_and_all_work_and_loss_history() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let count = observation(&db, &catalog, 0).await;
    let command = input(&db, &count.original).await;
    let posted = db
        .apply_hub_count_post(owner(), operation(3), command, now())
        .await
        .unwrap();
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.hub_group(owner(), count.original.scope.group)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let trace = snapshot(&db, receipt.original.trace.id).await;
    assert!(trace.on_hand.is_empty() && trace.exits.is_empty());
    assert_eq!(trace.balance().unwrap().counted_loss.value(), 20);
    let rows = db
        .hub_lineage(
            owner(),
            HubLineageQuery {
                trace: receipt.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.rows[0].packs.value(), 0);
    let CountEffect::Shortage { work_changes, .. } = &posted.original.effect else {
        panic!("shortage")
    };
    assert_eq!(work_changes[0].after.quantity.completed().value(), 0);
    assert_eq!(
        work_changes[0].after.quantity.no_longer_required().value(),
        20
    );
    assert_eq!(work_changes[0].after.quantity.remaining().value(), 0);
    assert_eq!(
        db.hub_count_post_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(posted)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_surplus_creates_two_unknown_review_required_packs_without_enlarging_the_original() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let count = observation(&db, &catalog, 22).await;
    let command = input(&db, &count.original).await;
    let posted = db
        .apply_hub_count_post(owner(), operation(3), command, now())
        .await
        .unwrap();
    let new = posted.surplus.as_ref().unwrap();
    assert_ne!(posted.original.id, new.population.id);
    assert_ne!(receipt.original.trace.id, new.population.id);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 2);
    let old = page
        .rows
        .iter()
        .find(|r| r.trace.id == receipt.original.trace.id)
        .unwrap();
    assert_eq!(old.position.packs.value(), 20);
    assert_eq!(
        old.put_away.as_ref().unwrap().quantity.remaining().value(),
        20
    );
    let extra = page
        .rows
        .iter()
        .find(|r| r.trace.id == new.population.id)
        .unwrap();
    assert_eq!(extra.position.packs.value(), 2);
    assert!(extra.put_away.is_none());
    assert_eq!(extra.trace.source, SourceAttribution::Unknown);
    assert_eq!(
        extra.trace.origin,
        PopulationOrigin::CountedSurplus {
            count: count.original.id,
            related_population: receipt.original.trace.id
        }
    );
    let trace = snapshot(&db, new.population.id).await;
    assert_eq!(trace.on_hand.len(), 1);
    assert!(trace.on_hand[0].treatment.active_holds.is_empty());
    assert_eq!(
        trace.on_hand[0].treatment.acceptance,
        AcceptanceStatus::ReviewRequired
    );
    assert_eq!(trace.balance().unwrap().original.value(), 2);
    assert_eq!(trace.balance().unwrap().counted_loss.value(), 0);
    assert_eq!(
        snapshot(&db, receipt.original.trace.id)
            .await
            .balance()
            .unwrap()
            .original
            .value(),
        20
    );
    assert_eq!(
        db.hub_count_post_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(posted)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_no_variance_closes_only_the_observation_and_competing_shortage_has_one_effect() {
    for counted in [20, 18] {
        let f = Fixture::new();
        let db = f.open().await;
        let catalog = setup(&db).await;
        db.apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
            .await
            .unwrap();
        let count = observation(&db, &catalog, counted).await;
        let command = input(&db, &count.original).await;
        let (a, b) = tokio::join!(
            db.apply_hub_count_post(owner(), operation(3), command.clone(), now()),
            db.apply_hub_count_post(owner(), operation(4), command, now())
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        assert_eq!(
            a.err().or_else(|| b.err()).unwrap().code(),
            SafeErrorCode::HubConflict
        );
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        assert_eq!(page.rows[0].position.packs.value(), counted);
        if counted == 20 {
            assert_eq!(
                page.rows[0].position.revision,
                count.original.scope.group_revision
            );
            assert_eq!(
                page.rows[0]
                    .put_away
                    .as_ref()
                    .unwrap()
                    .quantity
                    .remaining()
                    .value(),
                20
            );
        }
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_count_shortage_keeps_completed_work_in_removed_scope_history() {
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let group = db
        .hub_group(owner(), receipt.original.position.group)
        .await
        .unwrap();
    let target = catalog
        .records()
        .find(|r| {
            matches!(
                r.value,
                ReferenceValue::Location {
                    purpose: LocationPurpose::Storage,
                    ..
                }
            )
        })
        .unwrap()
        .id;
    db.apply_hub_movement(
        owner(),
        operation(10),
        harvestcircle_domain::hub::movement::MovementCommand {
            expected_stock_revision: group.revision,
            expected_catalog_revision: catalog.revision(),
            group: group.group.id,
            expected_group_revision: group.group.revision,
            to: target,
            packs: group.group.packs,
            separated_portion: None,
            occurred: Occurrence::Known(now()),
            observation: SourceProvenance::new("All twenty placed before the count").unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    let count = observation(&db, &catalog, 18).await;
    let command = input(&db, &count.original).await;
    let posted = db
        .apply_hub_count_post(owner(), operation(3), command, now())
        .await
        .unwrap();
    let group = db
        .hub_group(owner(), count.original.scope.group)
        .await
        .unwrap();
    let quantity = &group.group.work[0].requirement.quantity;
    assert_eq!(quantity.required().value(), 20);
    assert_eq!(quantity.completed().value(), 20);
    assert_eq!(quantity.no_longer_required().value(), 0);
    assert_eq!(quantity.remaining().value(), 0);
    assert_eq!(
        posted.original.work_scope_changes().unwrap()[0].removed_progress,
        WorkProgress::Completed
    );
    assert_eq!(
        snapshot(&db, receipt.original.trace.id)
            .await
            .balance()
            .unwrap()
            .counted_loss
            .value(),
        2
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn failure_after_all_posting_effects_rolls_back_every_original_and_projection() {
    for counted in [18, 22] {
        let f = Fixture::new();
        let db = f.open().await;
        let catalog = setup(&db).await;
        db.apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
            .await
            .unwrap();
        let count = observation(&db, &catalog, counted).await;
        let command = input(&db, &count.original).await;
        let before = db.hub_stock_page(owner(), query()).await.unwrap();
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let hash = command.fingerprint(hub, owner(), operation_id(&operation(3)).unwrap());
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reached = std::sync::Arc::clone(&completed);
        let result = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    write::apply_on(tx, hub, owner(), operation(3), command, hash, now()).await?;
                    reached.store(true, std::sync::atomic::Ordering::Release);
                    Err::<(), SafeError>(stock::conflict())
                })
            })
            .await;
        assert!(result.is_err());
        assert!(
            completed.load(std::sync::atomic::Ordering::Acquire),
            "all posting effects must succeed before deliberate rollback"
        );
        assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
        assert!(
            db.hub_count_post_operation(owner(), operation(3))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.hub_count_review(owner(), count.original.id)
                .await
                .unwrap()
                .assessment
                .ready()
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn posted_originals_survive_reference_edits_movement_and_a_later_reconciliation() {
    use harvestcircle_domain::hub::reference::{ReferenceCommand, ReferenceMutation};
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let first = observation(&db, &catalog, 18).await;
    let command = input(&db, &first.original).await;
    let original = db
        .apply_hub_count_post(owner(), operation(3), command.clone(), now())
        .await
        .unwrap();
    db.apply_hub_reference(
        owner(),
        operation(10),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: receipt.original.trace.product.id,
                expected_record_revision: receipt.original.trace.product.revision,
                value: ReferenceValue::Product {
                    label: HubLabel::new("Renamed after first reconciliation").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let group = db
        .hub_group(owner(), first.original.scope.group)
        .await
        .unwrap();
    let target = catalog
        .records()
        .find(|r| {
            matches!(
                r.value,
                ReferenceValue::Location {
                    purpose: LocationPurpose::Storage,
                    ..
                }
            )
        })
        .unwrap()
        .id;
    db.apply_hub_movement(
        owner(),
        operation(11),
        harvestcircle_domain::hub::movement::MovementCommand {
            expected_stock_revision: group.revision,
            expected_catalog_revision: catalog.revision(),
            group: group.group.id,
            expected_group_revision: group.group.revision,
            to: target,
            packs: group.group.packs,
            separated_portion: None,
            occurred: Occurrence::Known(now()),
            observation: SourceProvenance::new("All eighteen placed after the first count")
                .unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    let second = observation_at(&db, &catalog, 17, 12).await;
    let next = input(&db, &second.original).await;
    db.apply_hub_count_post(owner(), operation(13), next, now())
        .await
        .unwrap();
    let trace = snapshot(&db, receipt.original.trace.id).await;
    assert_eq!(trace.balance().unwrap().on_hand.value(), 17);
    assert_eq!(trace.balance().unwrap().counted_loss.value(), 3);
    assert_eq!(trace.count_losses.len(), 2);
    assert_eq!(
        db.hub_count_post_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        db.apply_hub_count_post(owner(), operation(3), command, now())
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    let group = db
        .hub_group(owner(), first.original.scope.group)
        .await
        .unwrap();
    let work = &group.group.work[0].requirement.quantity;
    assert_eq!(work.required().value(), 20);
    assert_eq!(work.completed().value(), 18);
    assert_eq!(work.no_longer_required().value(), 2);
    assert_eq!(work.remaining().value(), 0);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_surplus_preserves_posting_reference_versions_distinct_from_observation_and_current_labels()
 {
    use harvestcircle_domain::hub::reference::{ReferenceCommand, ReferenceMutation};
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let count = observation(&db, &catalog, 22).await;
    let old_label = count
        .original
        .scope
        .product
        .value
        .label()
        .as_str()
        .to_owned();
    for (identity, label) in [(10, "Posting-time product"), (11, "Later product")] {
        let catalog = db.hub_references(owner()).await.unwrap();
        let product = catalog.get(receipt.original.trace.product.id).unwrap();
        db.apply_hub_reference(
            owner(),
            operation(identity),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: product.id,
                    expected_record_revision: product.revision,
                    value: ReferenceValue::Product {
                        label: HubLabel::new(label).unwrap(),
                    },
                },
            },
            now(),
        )
        .await
        .unwrap();
        if identity == 10 {
            let command = input(&db, &count.original).await;
            db.apply_hub_count_post(owner(), operation(3), command, now())
                .await
                .unwrap();
        }
    }
    let posted = db
        .hub_count_post_operation(owner(), operation(3))
        .await
        .unwrap()
        .unwrap();
    let new = posted.surplus.as_ref().unwrap();
    assert_eq!(
        posted.original.count.scope.product.value.label().as_str(),
        old_label
    );
    assert_eq!(
        posted.original.references.product.value.label().as_str(),
        "Posting-time product"
    );
    assert_eq!(
        new.population.product.value.label().as_str(),
        "Posting-time product"
    );
    assert_eq!(new.population.product.revision.value(), 2);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let current = page
        .rows
        .iter()
        .find(|r| r.trace.id == new.population.id)
        .unwrap();
    assert_eq!(current.product.value.label().as_str(), "Later product");
    assert_eq!(current.trace.product, new.population.product);
    db.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_count_post_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(posted)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_posting_uses_real_durable_admission_and_recovers_the_original_without_another_effect()
 {
    use crate::{HubMutationResult, HubSubmissionStatus};
    use harvestcircle_application::{
        SessionGeneration,
        hub_submission::{HubMutation, HubSubmissionScope},
    };
    let f = Fixture::new();
    let db = f.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receiving(&catalog), now())
        .await
        .unwrap();
    let count = observation(&db, &catalog, 18).await;
    let command = input(&db, &count.original).await;
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let result = db
        .submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::CountPosting(Box::new(command.clone())),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::CountPosted(original) = result else {
        panic!("count result")
    };
    let saved = db
        .hub_submission(owner(), operation(3))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, HubSubmissionStatus::Recorded);
    assert_eq!(saved.kind, "count_post");
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let result = db
        .submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::CountPosting(Box::new(command)),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::CountPosted(replay) = result else {
        panic!("count result")
    };
    assert_eq!(replay, original);
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    assert_eq!(
        saved.captured_semantics(),
        db.hub_submission(owner(), operation(3))
            .await
            .unwrap()
            .unwrap()
            .captured_semantics()
    );
    db.close().await.unwrap();
}
