use super::*;
use crate::hub::stock::tests::{Fixture, id, now, operation, other, owner, query, setup};
use crate::{HubGroupCreationKind, HubLineageQuery, HubTraceSnapshotQuery, HubTreatmentScope};
use harvestcircle_domain::SafeErrorCode;
use harvestcircle_domain::hub::HubRevision;
use harvestcircle_domain::hub::movement::MovementCommand;
use harvestcircle_domain::hub::reference::{
    ReferenceCatalog, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceAttribution,
    WholePacks,
};
use harvestcircle_domain::hub::stock::Occurrence;
use sqlx::Connection;

pub(in crate::hub) fn command(catalog: &ReferenceCatalog) -> OpeningCommand {
    OpeningCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(7),
        packs: WholePacks::from_value(8).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new("Opening blue rack").unwrap(),
            observation: SourceProvenance::new("Eight physically distinguishable original crates")
                .unwrap(),
        },
        source: SourceAttribution::Unknown,
        history: OpeningHistory::Unknown,
        counted: Occurrence::Known(now()),
        observation: SourceProvenance::new("Counted present stock; earlier intake is not recorded")
            .unwrap(),
    }
}

#[tokio::test]
async fn counted_opening_is_present_stock_with_unknown_history_and_no_invented_work() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    let original = db
        .apply_hub_opening(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        original.original.population.origin,
        PopulationOrigin::CountedOpening {
            history: OpeningHistory::Unknown
        }
    );
    assert!(original.original.population.supplier.is_none());
    assert!(original.original.population.producer.is_none());
    assert!(original.original.population.planned_location.is_none());
    assert_eq!(original.original.population.initial_location.id, id(7));
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].position.packs.value(), 8);
    assert!(page.rows[0].put_away.is_none());
    assert_eq!(page.rows[0].trace, original.original.population);
    let group = db
        .hub_group(owner(), original.original.group.id)
        .await
        .unwrap();
    assert!(group.group.work.is_empty());
    assert_eq!(group.group.causal_floor, Some(now()));
    let treatment = db
        .hub_treatment(
            owner(),
            HubTreatmentScope {
                trace: original.original.population.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(treatment.projection.summary.review_required.value(), 8);
    assert_eq!(treatment.projection.summary.departure_eligible.value(), 0);
    let lineage = db
        .hub_lineage(
            owner(),
            HubLineageQuery {
                trace: original.original.population.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(lineage.rows.len(), 1);
    assert_eq!(
        lineage.rows[0].created_kind,
        HubGroupCreationKind::CountedOpening
    );
    assert_eq!(
        lineage.rows[0].identification.as_ref(),
        Some(&input.identification)
    );
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            HubTraceSnapshotQuery {
                trace: original.original.population.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.departed.value()
        ),
        (8, 8, 0)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let mut raw = fixture.raw().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hub_work_requirements")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hub_group_work")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        0
    );
    raw.close().await.unwrap();
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_opening_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
}

#[tokio::test]
async fn original_opening_survives_movement_reference_changes_and_duplicate_retry() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    let original = db
        .apply_hub_opening(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    let current = db
        .hub_group(owner(), original.original.group.id)
        .await
        .unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        MovementCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group: current.group.id,
            expected_group_revision: current.group.revision,
            to: id(6),
            packs: current.group.packs,
            separated_portion: None,
            occurred: Occurrence::Known(now()),
            observation: SourceProvenance::new("All eight actually moved to Receiving").unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    db.apply_hub_reference(
        owner(),
        operation(3),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(1),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Product {
                    label: HubLabel::new("Renamed current apples").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.apply_hub_opening(owner(), operation(1), input.clone(), now())
            .await
            .unwrap(),
        original
    );
    let mut changed = input.clone();
    changed.packs = WholePacks::from_value(9).unwrap();
    assert_eq!(
        db.apply_hub_opening(owner(), operation(1), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows[0].position.packs.value(), 8);
    assert_eq!(page.rows[0].position.location, id(6));
    assert_eq!(
        page.rows[0].product.value.label().as_str(),
        "Renamed current apples"
    );
    assert_eq!(
        page.rows[0].trace.product.value.label().as_str(),
        "Fictional apples"
    );
    assert!(page.rows[0].put_away.is_none());
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_opening_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
}

#[tokio::test]
async fn stale_competing_openings_and_foreign_actor_cannot_add_another_population() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    assert_eq!(
        db.apply_hub_opening(other(), operation(1), command(&catalog), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let a = command(&catalog);
    let mut b = a.clone();
    b.identification.label = HubLabel::new("Other physical rack").unwrap();
    let (a, b) = tokio::join!(
        db.apply_hub_opening(owner(), operation(1), a, now()),
        db.apply_hub_opening(owner(), operation(2), b, now())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code(),
        SafeErrorCode::HubConflict
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].position.packs.value(), 8);
    db.close().await.unwrap();
}

#[tokio::test]
async fn opening_failure_after_all_inserts_rolls_back_history_and_all_physical_state() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let input_for_failure = input.clone();
    let failure = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                reference::validate_basis_on(tx, hub.id()).await?;
                let op = operation(1);
                let ids = identities(&op)?;
                let hash = input_for_failure.fingerprint(hub.id(), owner(), ids);
                handling::treatment::capacity::ensure_on(
                    tx,
                    owner(),
                    &op,
                    handling::treatment::capacity::Reservation::Ordinary(1),
                )
                .await?;
                super::apply_on(tx, hub, op, input_for_failure, ids, hash, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    assert!(
        db.hub_opening_operation(owner(), operation(1))
            .await
            .unwrap()
            .is_none()
    );
    let mut raw = fixture.raw().await;
    for query in [
        "SELECT count(*) FROM hub_receipts",
        "SELECT count(*) FROM hub_counted_openings",
        "SELECT count(*) FROM hub_stock_operations",
        "SELECT count(*) FROM hub_stock_groups",
        "SELECT count(*) FROM hub_stock_positions",
        "SELECT count(*) FROM hub_group_identification",
        "SELECT count(*) FROM hub_stock_events",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .fetch_one(&mut raw)
                .await
                .unwrap(),
            0
        );
    }
    raw.close().await.unwrap();
    let original = db
        .apply_hub_opening(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        db.apply_hub_opening(owner(), operation(1), input, now())
            .await
            .unwrap(),
        original
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn opening_requires_known_count_and_distinct_active_identification() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    for counted in [
        Occurrence::Unknown,
        Occurrence::Interval {
            earliest: now(),
            latest: now(),
        },
        Occurrence::Known(UnixTimestamp::from_seconds(101).unwrap()),
    ] {
        let mut input = command(&catalog);
        input.counted = counted;
        assert_eq!(
            db.apply_hub_opening(owner(), operation(1), input, now())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubInvalidInput
        );
    }
    let mut empty = command(&catalog);
    empty.packs = WholePacks::from_value(0).unwrap();
    assert_eq!(
        db.apply_hub_opening(owner(), operation(1), empty, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap().total, 0);
    let original = db
        .apply_hub_opening(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let mut duplicate = command(&catalog);
    duplicate.expected_stock_revision = original.original.stock_revision;
    assert_eq!(
        db.apply_hub_opening(owner(), operation(2), duplicate, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let current = db
        .hub_group(owner(), original.original.group.id)
        .await
        .unwrap();
    assert_eq!(
        db.apply_hub_movement(
            owner(),
            operation(3),
            MovementCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: current.group.id,
                expected_group_revision: current.group.revision,
                to: id(6),
                packs: current.group.packs,
                separated_portion: None,
                occurred: Occurrence::Known(UnixTimestamp::from_seconds(99).unwrap()),
                observation: SourceProvenance::new("Contradicts the later confirmed count")
                    .unwrap(),
            },
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        8
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn counted_opening_departs_only_after_explicit_acceptance_and_keeps_original_after_exit() {
    use harvestcircle_domain::hub::departure::DepartureCommand;
    use harvestcircle_domain::hub::dispatch::{
        DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection,
    };
    use harvestcircle_domain::hub::reference::{
        SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
    };
    use harvestcircle_domain::hub::treatment::{
        AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection,
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut input = command(&catalog);
    input.source = SourceAttribution::Referenced {
        source: SourceReference {
            id: identities(&operation(1)).unwrap().source,
            issuer: Some(id(3)),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("VISIBLE-LABEL-08").unwrap(),
            provenance: SourceProvenance::new(
                "Current label manually read from the counted crates",
            )
            .unwrap(),
        },
        certainty: SourceCertainty::Established,
    };
    input.history = OpeningHistory::ExternalReference(
        SourceProvenance::new("Unverified earlier paper reference <OLD&8>").unwrap(),
    );
    let original = db
        .apply_hub_opening(owner(), operation(1), input.clone(), now())
        .await
        .unwrap();
    let group = original.original.group.id;
    let scope = HubTreatmentScope {
        trace: original.original.population.id,
        expected_revision: None,
        expected_catalog_revision: None,
    };
    assert_eq!(
        db.hub_treatment(owner(), scope.clone())
            .await
            .unwrap()
            .projection
            .summary
            .departure_eligible
            .value(),
        0
    );
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(2),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::SetAcceptance {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
                status: AcceptanceStatus::Accepted,
            },
            occurred: Occurrence::Known(now()),
            observation: SourceProvenance::new("Explicit sample acceptance after review").unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_treatment(owner(), scope)
            .await
            .unwrap()
            .projection
            .summary
            .departure_eligible
            .value(),
        8
    );
    let current = db.hub_group(owner(), group).await.unwrap();
    let prepared = db
        .apply_hub_dispatch(
            owner(),
            operation(3),
            DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: id(5),
                    lines: DispatchSelection::new(vec![DispatchLineInput {
                        group,
                        expected_group_revision: current.group.revision,
                        packs: current.group.packs,
                    }])
                    .unwrap(),
                },
                observation: SourceProvenance::new(
                    "Fictional recipient and all eight counted crates reviewed",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    let issued = db
        .apply_hub_dispatch(
            owner(),
            operation(4),
            DispatchCommand {
                expected_stock_revision: prepared.record.updated.stock_revision,
                expected_catalog_revision: prepared.catalog_revision,
                action: DispatchAction::Issue {
                    preparation: prepared.record.id,
                    expected_revision: prepared.record.revision,
                },
                observation: SourceProvenance::new("Issued counted-stock identification").unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    assert_eq!(
        issued.record.versions[0].material.lines[0].trace,
        original.original.population
    );
    let departed = db
        .apply_hub_departure(
            owner(),
            operation(5),
            DepartureCommand {
                expected_stock_revision: issued.record.updated.stock_revision,
                expected_catalog_revision: issued.catalog_revision,
                preparation: issued.record.id,
                expected_preparation_revision: issued.record.revision,
                identification: issued.record.versions[0].id,
                occurred: Occurrence::Known(now()),
                observation: SourceProvenance::new("All eight actually departed to this recipient")
                    .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    assert!(departed.record.work_changes.is_empty());
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            HubTraceSnapshotQuery {
                trace: original.original.population.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.departed.value()
        ),
        (8, 0, 8)
    );
    assert_eq!(
        db.apply_hub_opening(owner(), operation(1), input, now())
            .await
            .unwrap(),
        original
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_opening_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    db.close().await.unwrap();
}
