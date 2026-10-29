use super::*;
use crate::hub::stock::tests::{
    Fixture, command as intake_command, id, now, operation, owner, query, setup,
};
use crate::{
    Database, HubDispatchFilter, HubDispatchQuery, HubDispatchReceipt, HubHandlingQuery,
    HubHandlingResult, HubLineageQuery, HubStockReceipt,
};
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection,
};
use harvestcircle_domain::hub::movement::{MovementCommand, PhysicalIdentification};
use harvestcircle_domain::hub::portions::SplitCommand;
use harvestcircle_domain::hub::reference::{
    HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceProvenance,
};
use harvestcircle_domain::hub::treatment::{
    AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection,
};
use sqlx::Connection;

fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn documents() -> HubDispatchQuery {
    HubDispatchQuery {
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
        filter: HubDispatchFilter::All,
        text: String::new(),
    }
}
async fn open(db: &Database) -> HubStockReceipt {
    setup(db).await;
    for (n, label) in [(8, "Recipient A"), (9, "Recipient B")] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(1000 + u32::from(n)),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create {
                    id: id(n),
                    value: ReferenceValue::Party {
                        label: HubLabel::new(label).unwrap(),
                    },
                },
            },
            now(),
        )
        .await
        .unwrap();
    }
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_receipt(owner(), operation(1), intake_command(&catalog), now())
        .await
        .unwrap()
}
async fn split(db: &Database, group: HubRecordId, n: u32, op: u32, label: &str) -> HubRecordId {
    let current = db.hub_group(owner(), group).await.unwrap();
    let receipt = db
        .apply_hub_split(
            owner(),
            operation(op),
            SplitCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group,
                expected_group_revision: current.group.revision,
                packs: packs(n),
                identification: PhysicalIdentification {
                    label: HubLabel::new(label).unwrap(),
                    observation: note("Observed separately tagged original crates"),
                },
                occurred: Occurrence::Unknown,
                observation: note("Observed physical separation"),
            },
            now(),
        )
        .await
        .unwrap();
    let HubHandlingResult::Separated(result) = receipt.result else {
        panic!("split")
    };
    result.portion_group
}
async fn accept(db: &Database, groups: &[HubRecordId], op: u32) {
    let current = db.hub_group(owner(), groups[0]).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(op),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::SetAcceptance {
                selection: TreatmentSelection::new(groups.to_vec()).unwrap(),
                status: AcceptanceStatus::Accepted,
            },
            occurred: Occurrence::Unknown,
            observation: note("Explicit sample review accepted these groups"),
        },
        now(),
    )
    .await
    .unwrap();
}
async fn issue(db: &Database, groups: &[HubRecordId]) -> HubDispatchReceipt {
    issue_to(db, groups, id(8), 20).await
}
async fn issue_to(
    db: &Database,
    groups: &[HubRecordId],
    recipient: HubRecordId,
    op: u32,
) -> HubDispatchReceipt {
    let current = db.hub_dispatch_page(owner(), documents()).await.unwrap();
    let mut selected = Vec::new();
    for group in groups {
        let current = db.hub_group(owner(), *group).await.unwrap();
        selected.push(DispatchLineInput {
            group: *group,
            expected_group_revision: current.group.revision,
            packs: current.group.packs,
        });
    }
    let prepared = db
        .apply_hub_dispatch(
            owner(),
            operation(op),
            DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient,
                    lines: DispatchSelection::new(selected).unwrap(),
                },
                observation: note("Reviewed actual recipient and identified source groups"),
            },
            now(),
        )
        .await
        .unwrap();
    db.apply_hub_dispatch(
        owner(),
        operation(op + 1),
        DispatchCommand {
            expected_stock_revision: prepared.record.updated.stock_revision,
            expected_catalog_revision: prepared.catalog_revision,
            action: DispatchAction::Issue {
                preparation: prepared.record.id,
                expected_revision: prepared.record.revision,
            },
            observation: note("Explicit sample issue"),
        },
        now(),
    )
    .await
    .unwrap()
}
async fn departing(db: &Database, paper: &HubDispatchReceipt) -> DepartureCommand {
    let current = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    DepartureCommand {
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        preparation: paper.record.id,
        expected_preparation_revision: paper.record.revision,
        identification: paper.record.versions.last().unwrap().id,
        occurred: Occurrence::Unknown,
        observation: note("Manually confirmed actual departure to recipient A"),
    }
}

#[tokio::test]
async fn actual_current_host_installs_departure_objects_without_changing_reference_setup() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    assert_eq!(db.metadata().state_schema_version().get(), 14);
    let intake = open(&db).await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(before.rows[0].position.packs.value(), 20);
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group
            .packs
            .value(),
        20
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(before, db.hub_stock_page(owner(), query()).await.unwrap());
    db.close().await.unwrap();
}
#[tokio::test]
async fn actual_receiving_departure_preserves_eight_completed_retires_five_and_leaves_seven() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let root = intake.original.position.group;
    let group = db.hub_group(owner(), root).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        MovementCommand {
            expected_stock_revision: group.revision,
            expected_catalog_revision: group.catalog_revision,
            group: root,
            expected_group_revision: group.group.revision,
            to: id(7),
            packs: packs(8),
            separated_portion: Some(PhysicalIdentification {
                label: HubLabel::new("Blue eight").unwrap(),
                observation: note("Eight tagged crates"),
            }),
            occurred: Occurrence::Unknown,
            observation: note("Observed actual arrival at A3"),
        },
        now(),
    )
    .await
    .unwrap();
    let five = split(&db, root, 5, 3, "Purple five").await;
    accept(&db, &[five], 4).await;
    let paper = issue(&db, &[five]).await;
    let command = departing(&db, &paper).await;
    let recorded = db
        .apply_hub_departure(owner(), operation(30), command.clone(), now())
        .await
        .unwrap();
    assert_eq!(recorded.record.paper, paper.record);
    assert_eq!(recorded.record.consumed.len(), 1);
    assert_eq!(
        db.hub_group(owner(), five).await.unwrap_err().code(),
        SafeErrorCode::HubConflict
    );
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    let source = &recorded.record.consumed[0].before;
    assert_eq!(
        db.apply_hub_movement(
            owner(),
            operation(35),
            MovementCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: five,
                expected_group_revision: source.revision,
                to: id(7),
                packs: source.packs,
                separated_portion: None,
                occurred: Occurrence::Unknown,
                observation: note(
                    "Stale selected stock cannot be moved after its actual departure"
                )
            },
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_handling_operation(owner(), operation(35))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(current, db.hub_stock_page(owner(), query()).await.unwrap());

    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        current
            .rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        15
    );
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows[0]
        .work
        .clone();
    assert_eq!(
        (
            work.quantity.required().value(),
            work.quantity.completed().value(),
            work.quantity.no_longer_required().value(),
            work.quantity.remaining().value()
        ),
        (20, 8, 5, 7)
    );
    assert_eq!(
        db.apply_hub_departure(owner(), operation(30), command, now())
            .await
            .unwrap(),
        recorded
    );
    let fresh = departing(&db, &paper).await;
    assert!(
        db.apply_hub_departure(owner(), operation(31), fresh, now())
            .await
            .is_err()
    );
    let current_doc = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    let unused = DispatchCommand {
        expected_stock_revision: current_doc.revision,
        expected_catalog_revision: current_doc.catalog_revision,
        action: DispatchAction::MarkUnused {
            preparation: paper.record.id,
            expected_revision: paper.record.revision,
        },
        observation: note("Cannot mark already-used identification unused"),
    };
    assert_eq!(
        db.apply_hub_dispatch(owner(), operation(32), unused, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), paper.operation.clone())
            .await
            .unwrap(),
        Some(paper.clone())
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    let lineage = db
        .hub_lineage(
            owner(),
            HubLineageQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        lineage
            .rows
            .iter()
            .find(|r| r.id == five)
            .unwrap()
            .packs
            .value(),
        0
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(current, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(recorded)
    );
    assert_eq!(
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows[0]
            .work,
        work
    );
    db.close().await.unwrap();
}
#[tokio::test]
async fn actual_multiline_departure_has_one_journal_revision_and_all_line_effects() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let root = intake.original.position.group;
    let six = split(&db, root, 6, 2, "Green six").await;
    let four = split(&db, root, 4, 3, "Blue four").await;
    accept(&db, &[six, four], 4).await;
    let paper = issue(&db, &[six, four]).await;
    let command = departing(&db, &paper).await;
    let before = command.expected_stock_revision;
    let result = db
        .apply_hub_departure(owner(), operation(30), command, now())
        .await
        .unwrap();
    assert_eq!(result.record.stock_revision.value(), before.value() + 1);
    assert_eq!(result.record.physical_event_count(), 2);
    assert_eq!(result.record.work_changes.len(), 1);
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].position.packs.value(), 10);
    let work = &db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows[0]
        .work
        .quantity;
    assert_eq!(
        (
            work.completed().value(),
            work.no_longer_required().value(),
            work.remaining().value()
        ),
        (0, 10, 10)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(result)
    );
    db.close().await.unwrap();
}
#[tokio::test]
async fn new_hold_on_one_issued_line_rejects_every_physical_and_work_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let root = intake.original.position.group;
    let six = split(&db, root, 6, 2, "Green six").await;
    let four = split(&db, root, 4, 3, "Blue four").await;
    accept(&db, &[six, four], 4).await;
    let paper = issue(&db, &[six, four]).await;
    let current = db.hub_group(owner(), four).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(22),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![four]).unwrap(),
            },
            occurred: Occurrence::Unknown,
            observation: note("Independent hold after paper issuance"),
        },
        now(),
    )
    .await
    .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    assert_eq!(
        db.apply_hub_departure(owner(), operation(30), departing(&db, &paper).await, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(before, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        work,
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        None
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn rollback_and_competing_attempts_preserve_one_original_and_all_current_scopes() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let six = split(
        &db,
        intake.original.position.group,
        6,
        2,
        "Six original crates",
    )
    .await;
    let four = split(
        &db,
        intake.original.position.group,
        4,
        3,
        "Four original crates",
    )
    .await;
    accept(&db, &[six, four], 4).await;
    let paper = issue(&db, &[six, four]).await;
    let command = departing(&db, &paper).await;
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap();
    let failed = command.clone();
    assert!(
        db.host()
            .transaction(|tx| Box::pin(async move {
                write::apply_on(tx, owner(), operation(30), failed, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            }))
            .await
            .is_err()
    );
    assert_eq!(before, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        work,
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        None
    );
    assert!(
        db.hub_dispatch_record(owner(), paper.record.id)
            .await
            .unwrap()
            .departure
            .is_none()
    );
    let (a, b) = tokio::join!(
        db.apply_hub_departure(owner(), operation(30), command.clone(), now()),
        db.apply_hub_departure(owner(), operation(31), command.clone(), now())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let result = match (a, b) {
        (Ok(record), Err(error)) | (Err(error), Ok(record)) => {
            assert_eq!(error.code(), SafeErrorCode::HubConflict);
            record
        }
        _ => panic!("one committed departure"),
    };
    let mut changed = command;
    changed.observation = note("Different intent cannot reuse the original operation identity");
    assert_eq!(
        db.apply_hub_departure(owner(), result.operation.clone(), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let view = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    assert_eq!(view.state, crate::HubDispatchState::Departed);
    assert_eq!(view.departure.unwrap().id, result.record.id);
    assert_eq!(view.record, paper.record);
    let mut filter = documents();
    filter.filter = HubDispatchFilter::Issued;
    assert!(
        db.hub_dispatch_page(owner(), filter)
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    let mut filter = documents();
    filter.filter = HubDispatchFilter::Departed;
    let rows = db.hub_dispatch_page(owner(), filter).await.unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, crate::HubDispatchState::Departed);
    assert!(
        db.hub_departure_operation(crate::hub::stock::tests::other(), result.operation.clone())
            .await
            .is_err()
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_departure_operation(owner(), result.operation.clone())
            .await
            .unwrap(),
        Some(result)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn completed_a3_stock_departs_six_to_a_and_four_to_b_without_erasing_completion() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let root = intake.original.position.group;
    let group = db.hub_group(owner(), root).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        MovementCommand {
            expected_stock_revision: group.revision,
            expected_catalog_revision: group.catalog_revision,
            group: root,
            expected_group_revision: group.group.revision,
            to: id(7),
            packs: packs(20),
            separated_portion: None,
            occurred: Occurrence::Unknown,
            observation: note("Observed all twenty original crates at A3"),
        },
        now(),
    )
    .await
    .unwrap();
    let six = split(&db, root, 6, 3, "Six tagged crates for A").await;
    let four = split(&db, root, 4, 4, "Four tagged crates for B").await;
    accept(&db, &[six, four], 5).await;
    let a = issue_to(&db, &[six], id(8), 20).await;
    let command = departing(&db, &a).await;
    let departed_a = db
        .apply_hub_departure(owner(), operation(30), command, now())
        .await
        .unwrap();
    let b = issue_to(&db, &[four], id(9), 40).await;
    let mut command = departing(&db, &b).await;
    command.observation = note("Observed four tagged crates depart to recipient B");
    let departed_b = db
        .apply_hub_departure(owner(), operation(50), command, now())
        .await
        .unwrap();
    assert!(departed_a.record.work_changes.is_empty());
    assert!(departed_b.record.work_changes.is_empty());
    assert_eq!(
        departed_a.record.paper.versions[0].material.recipient.id,
        id(8)
    );
    assert_eq!(
        departed_b.record.paper.versions[0].material.recipient.id,
        id(9)
    );
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(current.rows.len(), 1);
    assert_eq!(current.rows[0].position.packs.value(), 10);
    let work = &db
        .hub_work_page(
            owner(),
            HubHandlingQuery {
                open_only: false,
                ..HubHandlingQuery::default()
            },
        )
        .await
        .unwrap()
        .rows[0]
        .work
        .quantity;
    assert_eq!(
        (
            work.required().value(),
            work.completed().value(),
            work.no_longer_required().value(),
            work.remaining().value()
        ),
        (20, 20, 0, 0)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departed_a.clone())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(current, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departed_a)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(50))
            .await
            .unwrap(),
        Some(departed_b)
    );
    db.close().await.unwrap();
}
#[tokio::test]
async fn unlike_products_keep_separate_trace_mass_in_one_actual_departure() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let first = open(&db).await;
    for (n, value) in [
        (
            10,
            ReferenceValue::Product {
                label: HubLabel::new("Fictional pears").unwrap(),
            },
        ),
        (
            11,
            ReferenceValue::Pack {
                product: id(10),
                label: HubLabel::new("Original pear carton").unwrap(),
                nominal_grams: None,
            },
        ),
    ] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(1000 + u32::from(n)),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create { id: id(n), value },
            },
            now(),
        )
        .await
        .unwrap();
    }
    let catalog = db.hub_references(owner()).await.unwrap();
    let mut input = intake_command(&catalog);
    input.expected_stock_revision = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    input.variant = id(11);
    input.packs = packs(7);
    input.planned_location = None;
    input.observation = note("Seven fictional original pear cartons received independently");
    let second = db
        .apply_hub_receipt(owner(), operation(2), input, now())
        .await
        .unwrap();
    let a = first.original.position.group;
    let b = second.original.position.group;
    accept(&db, &[a], 3).await;
    accept(&db, &[b], 4).await;
    let paper = issue(&db, &[a, b]).await;
    let result = db
        .apply_hub_departure(owner(), operation(30), departing(&db, &paper).await, now())
        .await
        .unwrap();
    assert_eq!(result.record.consumed.len(), 2);
    assert_eq!(result.record.work_changes.len(), 2);
    assert_ne!(
        result.record.paper.versions[0].material.lines[0].variant.id,
        result.record.paper.versions[0].material.lines[1].variant.id
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    for intake in [&first, &second] {
        let lineage = db
            .hub_lineage(
                owner(),
                HubLineageQuery {
                    trace: intake.original.trace.id,
                    expected_revision: None,
                    expected_catalog_revision: None,
                    offset: 0,
                    limit: 64,
                },
            )
            .await
            .unwrap();
        assert_eq!(lineage.rows.len(), 1);
        assert_eq!(lineage.rows[0].packs.value(), 0);
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(result)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn reused_inactive_tag_keeps_distinct_acceptance_and_originals_through_later_handling() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let root = intake.original.position.group;
    let old = split(&db, root, 5, 2, "Reusable observed tag").await;
    accept(&db, &[old], 3).await;
    let paper = issue(&db, &[old]).await;
    let first = db
        .apply_hub_departure(owner(), operation(30), departing(&db, &paper).await, now())
        .await
        .unwrap();
    let new = split(&db, root, 3, 40, "Reusable observed tag").await;
    assert_ne!(old, new);
    let source = db.hub_group(owner(), root).await.unwrap();
    let duplicate = SplitCommand {
        expected_stock_revision: source.revision,
        expected_catalog_revision: source.catalog_revision,
        group: root,
        expected_group_revision: source.group.revision,
        packs: packs(1),
        identification: PhysicalIdentification {
            label: HubLabel::new("Reusable observed tag").unwrap(),
            observation: note("An active tag cannot identify another population"),
        },
        occurred: Occurrence::Unknown,
        observation: note("Duplicate active-tag attempt must leave stock unchanged"),
    };
    assert_eq!(
        db.apply_hub_split(owner(), operation(41), duplicate, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let scope = crate::HubTreatmentScope {
        trace: intake.original.trace.id,
        expected_revision: None,
        expected_catalog_revision: None,
    };
    let treatment = db.hub_treatment(owner(), scope.clone()).await.unwrap();
    assert_eq!(treatment.projection.summary.accepted.value(), 0);
    assert_eq!(
        treatment
            .projection
            .groups
            .iter()
            .find(|group| group.group == new)
            .unwrap()
            .acceptance,
        AcceptanceStatus::ReviewRequired
    );
    let second_paper = issue_to(&db, &[new], id(9), 50).await;
    assert_eq!(
        db.apply_hub_departure(
            owner(),
            operation(60),
            departing(&db, &second_paper).await,
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_departure_operation(owner(), operation(60))
            .await
            .unwrap()
            .is_none()
    );
    let current = db.hub_group(owner(), root).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(70),
        MovementCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group: root,
            expected_group_revision: current.group.revision,
            to: id(7),
            packs: current.group.packs,
            separated_portion: None,
            occurred: Occurrence::Unknown,
            observation: note("Observed the remaining twelve original crates arrive at A3"),
        },
        now(),
    )
    .await
    .unwrap();
    accept(&db, &[new], 71).await;
    let mut command = departing(&db, &second_paper).await;
    command.observation = note("Three newly accepted crates actually depart to recipient B");
    let second = db
        .apply_hub_departure(owner(), operation(80), command, now())
        .await
        .unwrap();
    let work = db
        .hub_work_page(
            owner(),
            HubHandlingQuery {
                open_only: false,
                ..HubHandlingQuery::default()
            },
        )
        .await
        .unwrap()
        .rows[0]
        .work
        .clone();
    assert_eq!(
        (
            work.quantity.required().value(),
            work.quantity.completed().value(),
            work.quantity.no_longer_required().value(),
            work.quantity.remaining().value()
        ),
        (20, 12, 8, 0)
    );
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(current.rows.len(), 1);
    assert_eq!(current.rows[0].position.packs.value(), 12);
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        db.hub_group(owner(), old).await.unwrap_err().code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_group(owner(), new).await.unwrap_err().code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(current, db.hub_stock_page(owner(), query()).await.unwrap());
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(second)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn corrupt_departure_original_is_never_disguised_as_unavailable_stock() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = open(&db).await;
    let group = intake.original.position.group;
    accept(&db, &[group], 2).await;
    let paper = issue(&db, &[group]).await;
    let other_paper = issue_to(&db, &[group], id(9), 40).await;
    let recorded = db
        .apply_hub_departure(owner(), operation(30), departing(&db, &paper).await, now())
        .await
        .unwrap();
    let mut raw = fixture.raw().await;
    assert!(
        sqlx::query("UPDATE hub_departures SET result_hash=zeroblob(32) WHERE operation_id=?")
            .bind(recorded.operation.as_str())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let trigger = schema::MIGRATION_SQL
        .split(";\n")
        .find(|sql| sql.starts_with("CREATE TRIGGER hub_departure_no_update "))
        .unwrap();
    sqlx::query("DROP TRIGGER hub_departure_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_departures SET result_hash=zeroblob(32) WHERE operation_id=?")
        .bind(recorded.operation.as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(trigger).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_group(owner(), group).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_departure_review(
            owner(),
            HubDepartureReviewInput {
                preparation: paper.record.id,
                expected_preparation_revision: paper.record.revision,
                identification: paper.record.versions[0].id
            },
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_departure_review(
            owner(),
            HubDepartureReviewInput {
                preparation: other_paper.record.id,
                expected_preparation_revision: other_paper.record.revision,
                identification: other_paper.record.versions[0].id
            },
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}

#[path = "review_tests.rs"]
mod review;
