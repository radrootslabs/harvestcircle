use super::*;
use crate::hub::stock::tests::{Fixture, command, id, now, operation, other, owner, setup};
use harvestcircle_domain::hub::{
    actuals::ActualPhysicalBirthKind,
    disposition::{DispositionCommand, DispositionKind},
    movement::PhysicalIdentification,
    portions::SplitCommand,
    reference::{HubLabel, SourceProvenance, WholePacks},
    stock::Occurrence,
};
use harvestcircle_domain::{SafeErrorCode, UnixTimestamp};
use sqlx::Connection;

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn query(trace: HubRecordId) -> HubActualLineageQuery {
    HubActualLineageQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
    }
}
async fn received(db: &Database) -> crate::HubStockReceipt {
    let catalog = setup(db).await;
    let mut input = command(&catalog);
    input.occurred = Occurrence::Known(now());
    db.apply_hub_receipt(owner(), operation(1), input, now())
        .await
        .unwrap()
}
async fn separated(db: &Database, group: HubRecordId) -> HubRecordId {
    let source = db.hub_group(owner(), group).await.unwrap();
    let result = db
        .apply_hub_split(
            owner(),
            operation(2),
            SplitCommand {
                expected_stock_revision: source.revision,
                expected_catalog_revision: source.catalog_revision,
                group,
                expected_group_revision: source.group.revision,
                packs: packs(6),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Six distinct original crates").unwrap(),
                    observation: note("Physically separated original six in the marked stack"),
                },
                occurred: Occurrence::Known(at(110)),
                observation: note("Original observed separation at Receiving"),
            },
            at(110),
        )
        .await
        .unwrap();
    let crate::HubHandlingResult::Separated(result) = result.result else {
        panic!("split")
    };
    result.portion_group
}

#[tokio::test]
async fn native_birth_evidence_uses_original_twenty_and_six_after_full_child_depletion_and_reopen()
{
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let child = separated(&db, intake.original.position.group).await;
    let view = db.hub_group(owner(), child).await.unwrap();
    db.apply_hub_disposition(
        owner(),
        operation(3),
        DispositionCommand {
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group: child,
            expected_group_revision: view.group.revision,
            packs: packs(6),
            kind: DispositionKind::Destroyed,
            occurred: Occurrence::Known(at(120)),
            observation: note("All six actually removed from this identified group"),
        },
        at(120),
    )
    .await
    .unwrap();
    let result = db
        .hub_actual_lineage(owner(), query(intake.original.trace.id))
        .await
        .unwrap();
    assert_eq!(result.births.len(), 2);
    assert_eq!(result.births[0].kind, ActualPhysicalBirthKind::Receipt);
    assert_eq!(result.births[0].packs, packs(20));
    assert_eq!(result.births[0].occurred, Occurrence::Known(now()));
    assert_eq!(
        result.births[1].kind,
        ActualPhysicalBirthKind::ObservedPortion
    );
    assert_eq!(result.births[1].packs, packs(6));
    assert_eq!(result.births[1].occurred, Occurrence::Known(at(110)));
    assert_eq!(result.lineage.node(child).unwrap().packs, packs(0));
    assert_eq!(result.births[1].actor, owner());
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        reopened
            .hub_actual_lineage(owner(), query(intake.original.trace.id))
            .await
            .unwrap(),
        result
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn canonical_read_requires_current_owner_and_exact_stock_and_catalog_context() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let trace = intake.original.trace.id;
    assert_eq!(
        db.hub_actual_lineage(other(), query(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = query(trace);
    stale.expected_revision = Some(HubRevision::INITIAL);
    assert_eq!(
        db.hub_actual_lineage(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let mut stale = query(trace);
    stale.expected_catalog_revision = Some(HubRevision::INITIAL);
    assert_eq!(
        db.hub_actual_lineage(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let evidence = db.hub_actual_lineage(owner(), query(trace)).await.unwrap();
    let mut exact = query(trace);
    exact.expected_revision = Some(evidence.revision);
    exact.expected_catalog_revision = Some(evidence.catalog_revision);
    assert_eq!(
        db.hub_actual_lineage(owner(), exact).await.unwrap(),
        evidence
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn unknown_physical_birth_does_not_become_its_native_recording_timestamp() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let evidence = db
        .hub_actual_lineage(owner(), query(intake.original.trace.id))
        .await
        .unwrap();
    assert_eq!(evidence.births[0].occurred, Occurrence::Unknown);
    assert_eq!(evidence.births[0].recorded_at, now());
    assert_eq!(evidence.births[0].packs, packs(20));
    assert_eq!(evidence.population.source, intake.original.trace.source);
    db.close().await.unwrap();
}

#[tokio::test]
async fn moved_portion_birth_is_its_original_eight_not_a_later_whole_group_arrival() {
    use harvestcircle_domain::hub::movement::MovementCommand;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let root = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let moved = db
        .apply_hub_movement(
            owner(),
            operation(2),
            MovementCommand {
                expected_stock_revision: root.revision,
                expected_catalog_revision: root.catalog_revision,
                group: root.group.id,
                expected_group_revision: root.group.revision,
                to: id(7),
                packs: packs(8),
                separated_portion: Some(PhysicalIdentification {
                    label: HubLabel::new("Moved eight birth").unwrap(),
                    observation: note("Eight distinctly identified crates originally moved to A3"),
                }),
                occurred: Occurrence::Known(at(110)),
                observation: note("Original partial movement and physical birth"),
            },
            at(110),
        )
        .await
        .unwrap();
    let crate::HubHandlingResult::Moved(moved) = moved.result else {
        panic!("movement")
    };
    let current = db.hub_group(owner(), moved.moved_group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(3),
        MovementCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group: current.group.id,
            expected_group_revision: current.group.revision,
            to: id(6),
            packs: packs(8),
            separated_portion: None,
            occurred: Occurrence::Known(at(120)),
            observation: note("Later whole-group arrival is not the physical birth"),
        },
        at(120),
    )
    .await
    .unwrap();
    let evidence = db
        .hub_actual_lineage(owner(), query(intake.original.trace.id))
        .await
        .unwrap();
    let birth = evidence
        .births
        .iter()
        .find(|b| b.group == moved.moved_group)
        .unwrap();
    assert_eq!(birth.kind, ActualPhysicalBirthKind::ObservedPortion);
    assert_eq!(birth.packs, packs(8));
    assert_eq!(birth.occurred, Occurrence::Known(at(110)));
    assert_eq!(birth.recorded_at, at(110));
    assert_eq!(
        db.hub_group(owner(), moved.moved_group)
            .await
            .unwrap()
            .location
            .id,
        id(6)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn counted_opening_and_surplus_have_their_distinct_original_births() {
    use harvestcircle_domain::hub::{
        counts::{CountObserveCommand, CountPostCommand},
        reference::SourceAttribution,
        stock::{OpeningCommand, OpeningHistory},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let opening = db
        .apply_hub_opening(
            owner(),
            operation(2000),
            OpeningCommand {
                expected_stock_revision: HubRevision::INITIAL,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                location: id(6),
                packs: packs(8),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Opening eight birth").unwrap(),
                    observation: note(
                        "Separately counted opening crates with unknown earlier history",
                    ),
                },
                source: SourceAttribution::Unknown,
                history: OpeningHistory::Unknown,
                counted: Occurrence::Known(now()),
                observation: note("Present counted opening, not a receipt"),
            },
            now(),
        )
        .await
        .unwrap();
    let evidence = db
        .hub_actual_lineage(owner(), query(opening.original.population.id))
        .await
        .unwrap();
    assert_eq!(
        evidence.births[0].kind,
        ActualPhysicalBirthKind::CountedOpening
    );
    assert_eq!(evidence.births[0].packs, packs(8));
    db.close().await.unwrap();

    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let root = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let counted = db
        .observe_hub_count(
            owner(),
            operation(2),
            CountObserveCommand {
                expected_stock_revision: root.revision,
                expected_catalog_revision: root.catalog_revision,
                group: root.group.id,
                expected_group_revision: root.group.revision,
                counted_packs: packs(25),
                cutoff: Occurrence::Known(now()),
                observation: note("Observed twenty-five including five physically separate extras"),
            },
            now(),
        )
        .await
        .unwrap();
    let review = db
        .hub_count_review(owner(), counted.original.id)
        .await
        .unwrap();
    let posted = db
        .apply_hub_count_post(
            owner(),
            operation(3),
            CountPostCommand {
                expected_stock_revision: review.stock_revision,
                expected_catalog_revision: review.catalog_revision,
                count: counted.original.id,
                reviewed_activity: true,
                surplus_identification: Some(PhysicalIdentification {
                    label: HubLabel::new("Counted five birth").unwrap(),
                    observation: note(
                        "Five separate extras, not enlargement of the original twenty",
                    ),
                }),
                reason: note(
                    "Reviewed native scope and current activity before admitting new counted stock",
                ),
            },
            now(),
        )
        .await
        .unwrap();
    let surplus = posted.surplus.unwrap();
    let evidence = db
        .hub_actual_lineage(owner(), query(surplus.population.id))
        .await
        .unwrap();
    assert_eq!(
        evidence.births[0].kind,
        ActualPhysicalBirthKind::CountedSurplus
    );
    assert_eq!(evidence.births[0].packs, packs(5));
    assert_eq!(evidence.births[0].occurred, Occurrence::Known(now()));
    assert_eq!(evidence.births[0].group, surplus.group.id);
    db.close().await.unwrap();
}

#[tokio::test]
async fn returned_birth_is_independent_of_the_original_departure_and_population() {
    returned_birth(Occurrence::Known(now())).await;
}

#[tokio::test]
async fn interval_return_preserves_original_birth_and_earliest_physical_floor() {
    returned_birth(Occurrence::Interval {
        earliest: at(90),
        latest: now(),
    })
    .await;
}

#[tokio::test]
async fn unknown_return_preserves_unknown_birth_without_inventing_a_floor() {
    returned_birth(Occurrence::Unknown).await;
}

async fn returned_birth(occurred: Occurrence) {
    use crate::hub::handling::departure::tests::custody_departure_fixture;
    use harvestcircle_domain::hub::{
        reference::SourceAttribution,
        returns::{ReturnCommand, ReturnDepartureClaim},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) = custody_departure_fixture(&db).await;
    let stock = db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let returned = db
        .apply_hub_return(
            owner(),
            operation(2000),
            ReturnCommand {
                expected_stock_revision: stock.revision,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                location: id(6),
                returned_by: Some(id(9)),
                source: SourceAttribution::Unknown,
                packs: packs(2),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Returned two birth evidence").unwrap(),
                    observation: note(
                        "New identified returned custody, independent of the original recipient",
                    ),
                },
                planned_location: None,
                departure: Some(ReturnDepartureClaim {
                    departure: departure.record.id,
                    group: Some(departure.record.consumed[0].before.id),
                }),
                occurred,
                observation: note("Separate returned intake, no old work or acceptance inherited"),
            },
            now(),
        )
        .await
        .unwrap();
    let evidence = db
        .hub_actual_lineage(owner(), query(returned.original.identities.population))
        .await
        .unwrap();
    assert_eq!(evidence.births.len(), 1);
    assert_eq!(evidence.births[0].kind, ActualPhysicalBirthKind::Returned);
    assert_eq!(evidence.births[0].packs, packs(2));
    assert_eq!(evidence.births[0].occurred, occurred);
    assert_eq!(evidence.births[0].parent, None);
    assert_ne!(evidence.births[0].trace, intake.original.trace.id);
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departure)
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(2000))
            .await
            .unwrap(),
        Some(returned.clone())
    );
    let group = returned.original.identities.group;
    let expected_floor = match occurred {
        Occurrence::Known(at) => Some(at),
        Occurrence::Interval { earliest, .. } => Some(earliest),
        Occurrence::Unknown => None,
    };
    assert_eq!(
        db.hub_group(owner(), group)
            .await
            .unwrap()
            .group
            .causal_floor,
        expected_floor
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_lineage(owner(), query(evidence.population.id))
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(2000))
            .await
            .unwrap(),
        Some(returned)
    );
    if let Some(floor) = expected_floor {
        let view = db.hub_group(owner(), group).await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        let error = db
            .apply_hub_movement(
                owner(),
                operation(2001),
                harvestcircle_domain::hub::movement::MovementCommand {
                    expected_stock_revision: view.revision,
                    expected_catalog_revision: view.catalog_revision,
                    group,
                    expected_group_revision: view.group.revision,
                    to: id(7),
                    packs: packs(2),
                    separated_portion: None,
                    occurred: Occurrence::Known(at(floor.as_seconds() - 1)),
                    observation: note("A movement before actual returned custody is impossible"),
                },
                now(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), SafeErrorCode::HubInvalidInput);
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    db.close().await.unwrap();
}

#[tokio::test]
async fn balanced_population_total_cannot_hide_growth_beyond_one_original_birth() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let child = separated(&db, intake.original.position.group).await;
    let mut raw = fixture.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=packs-1 WHERE group_id=?")
        .bind(intake.original.position.group.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_stock_positions SET packs=packs+1 WHERE group_id=?")
        .bind(child.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_lineage(owner(), query(intake.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
