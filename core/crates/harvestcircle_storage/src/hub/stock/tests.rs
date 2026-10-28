use super::*;
use crate::Database;
use crate::db::storage_unavailable;
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::reference::{
    HubLabel, LocationPurpose, ReferenceCatalog, ReferenceCommand, ReferenceMutation,
    ReferenceValue, SourceAttribution, SourceCertainty, SourceCode, SourceProvenance,
    SourceReference, SourceReferenceKind, WholePacks,
};
use harvestcircle_domain::hub::stock::{Occurrence, ReceiptCommand};
use harvestcircle_domain::hub::{HubId, HubOwnership};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    context: RuntimeContext,
    build: MigrationBuildIdentity,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let context = RuntimeContext::resolve(
            &RadrootsPathResolver::new(
                RadrootsPlatform::current(),
                RadrootsHostEnvironment::default(),
            ),
            RuntimeContextBootstrap::new(
                RadrootsPathProfile::RepoLocal,
                Some(directory.path().canonicalize().unwrap()),
                RuntimeContextSource::BootstrapCli,
                RuntimeContextSource::SafeDefault,
            )
            .unwrap(),
            ServiceId::new("harvestcircle").unwrap(),
            InstanceId::new("desktop").unwrap(),
        )
        .unwrap();
        std::fs::create_dir(directory.path().join("data")).unwrap();
        let build = MigrationBuildIdentity::new(
            "0.1.0-alpha",
            "1111111111111111111111111111111111111111",
            "189c49b74b4bafc142b00b76b296477931139e72",
            "1.99.0",
            "test",
            "test",
            1,
            1,
            1,
            1,
            1,
        )
        .unwrap();
        Self {
            _directory: directory,
            context,
            build,
        }
    }
    async fn open(&self) -> Database {
        Database::open(&self.context, 99, 100, &self.build)
            .await
            .unwrap()
    }
    async fn raw(&self) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(self.context.paths().state().join("state.sqlite"))
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .unwrap()
    }
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn other() -> PublicKey {
    PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5").unwrap()
}
fn now() -> UnixTimestamp {
    UnixTimestamp::from_seconds(100).unwrap()
}
fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn operation(n: u32) -> DurableRequestId {
    DurableRequestId::parse(format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}
fn query() -> HubStockQuery {
    HubStockQuery {
        expected_catalog_revision: None,
        expected_revision: None,
        offset: 0,
        limit: 64,
        location: None,
        trace: None,
        text: String::new(),
    }
}
async fn setup(db: &Database) -> ReferenceCatalog {
    db.initialize_hub(
        HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), owner()),
        &operation(65535),
        now(),
    )
    .await
    .unwrap();
    for (n, value) in [
        (
            1,
            ReferenceValue::Product {
                label: HubLabel::new("Fictional apples").unwrap(),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Original crate").unwrap(),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Party {
                label: HubLabel::new("Fictional supplier").unwrap(),
            },
        ),
        (
            4,
            ReferenceValue::Party {
                label: HubLabel::new("Fictional producer").unwrap(),
            },
        ),
        (
            5,
            ReferenceValue::Party {
                label: HubLabel::new("Fictional issuer").unwrap(),
            },
        ),
        (
            6,
            ReferenceValue::Location {
                label: HubLabel::new("Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            7,
            ReferenceValue::Location {
                label: HubLabel::new("A3").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(100 + n as u32),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create { id: id(n), value },
            },
            now(),
        )
        .await
        .unwrap();
    }
    db.hub_references(owner()).await.unwrap()
}
fn command(catalog: &ReferenceCatalog) -> ReceiptCommand {
    ReceiptCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        supplier: Some(id(3)),
        producer: Some(id(4)),
        source: SourceAttribution::Unknown,
        packs: WholePacks::from_value(20).unwrap(),
        receiving: id(6),
        planned_location: Some(id(7)),
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Counted twenty whole crates at Receiving").unwrap(),
    }
}
fn known_source(operation: &DurableRequestId, issuer: Option<HubRecordId>) -> SourceAttribution {
    SourceAttribution::Referenced {
        source: SourceReference {
            id: receipt_ids(operation).unwrap().source,
            issuer,
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("BATCH%_01").unwrap(),
            provenance: SourceProvenance::new("Original manually confirmed fictional label")
                .unwrap(),
        },
        certainty: SourceCertainty::Established,
    }
}

#[tokio::test]
async fn actual_receipt_positions_work_and_originals_survive_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    let op = operation(1);
    let receipt = db
        .apply_hub_receipt(owner(), op.clone(), input.clone(), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.revision.value(), 2);
    assert_eq!(page.total, 1);
    assert_eq!(page.rows.len(), 1);
    let row = &page.rows[0];
    assert_eq!(row.position.packs.value(), 20);
    assert_eq!(row.position.location, id(6));
    assert_eq!(row.work_target.as_ref().unwrap().id, id(7));
    assert_eq!(row.put_away.required().value(), 20);
    assert_eq!(row.put_away.completed().value(), 0);
    assert_eq!(row.put_away.no_longer_required().value(), 0);
    assert_eq!(row.put_away.remaining().value(), 20);
    assert!(row.trace.source_needs_review());
    assert_eq!(row.trace.occurred, Occurrence::Unknown);
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                location: Some(id(7)),
                ..query()
            }
        )
        .await
        .unwrap()
        .total,
        0
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_receipt_operation(owner(), op.clone()).await.unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        db.apply_hub_receipt(
            owner(),
            op,
            input,
            UnixTimestamp::from_seconds(200).unwrap()
        )
        .await
        .unwrap(),
        receipt
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
}

#[tokio::test]
async fn replay_precedes_current_versions_and_changed_intent_cannot_add_stock() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut input = command(&catalog);
    let op = operation(1);
    input.source = known_source(&op, Some(id(5)));
    let receipt = db
        .apply_hub_receipt(owner(), op.clone(), input.clone(), now())
        .await
        .unwrap();
    db.apply_hub_reference(
        owner(),
        operation(200),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(1),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Product {
                    label: HubLabel::new("Renamed product").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.apply_hub_receipt(owner(), op.clone(), input.clone(), now())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        receipt.original.trace.product.value.label().as_str(),
        "Fictional apples"
    );
    input.packs = WholePacks::from_value(21).unwrap();
    assert_eq!(
        db.apply_hub_receipt(owner(), op.clone(), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        20
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), op).await.unwrap(),
        Some(receipt)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_stale_receipts_and_unauthorized_subject_leave_one_complete_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let input = command(&catalog);
    assert_eq!(
        db.apply_hub_receipt(other(), operation(9), input.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_stock_page(other(), query())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let (one, two) = tokio::join!(
        db.apply_hub_receipt(owner(), operation(1), input.clone(), now()),
        db.apply_hub_receipt(owner(), operation(2), input, now())
    );
    assert_ne!(one.is_ok(), two.is_ok());
    assert_eq!(
        one.err().or_else(|| two.err()).unwrap().code(),
        SafeErrorCode::HubConflict
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].position.packs.value(), 20);
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_transaction_after_all_production_inserts_rolls_back_every_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut input = command(&catalog);
    input.source = known_source(&operation(1), Some(id(5)));
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                super::write::apply_on(tx, owner(), operation(1), input, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(result.is_err());
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.revision, HubRevision::INITIAL);
    assert_eq!(page.total, 0);
    assert!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .is_none()
    );
    db.host().transaction(|tx|Box::pin(async move {
        let row=sqlx::query("SELECT (SELECT count(*) FROM hub_stock_operations) AS operations,(SELECT count(*) FROM hub_receipts) AS receipts,(SELECT count(*) FROM hub_stock_groups) AS groups,(SELECT count(*) FROM hub_putaway_work) AS work,(SELECT count(*) FROM hub_stock_events) AS events,(SELECT count(*) FROM hub_source_references) AS sources")
            .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        for field in ["operations","receipts","groups","work","events","sources"] { assert_eq!(row.get::<i64,_>(field),0); }
        Ok::<(),SafeError>(())
    })).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn same_code_and_unknown_issuers_never_merge_sources_and_reuse_is_exact() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut original = None;
    for (n, issuer) in [(1, Some(id(3))), (2, Some(id(5))), (3, None), (4, None)] {
        let op = operation(n);
        let mut input = command(&catalog);
        input.expected_stock_revision = HubRevision::from_value(n as u64).unwrap();
        input.source = known_source(&op, issuer);
        let receipt = db
            .apply_hub_receipt(owner(), op, input, now())
            .await
            .unwrap();
        if n == 1 {
            original = Some(receipt);
        }
    }
    let original = original.unwrap();
    let mut input = command(&catalog);
    input.expected_stock_revision = HubRevision::from_value(5).unwrap();
    input.source = original.original.trace.source.clone();
    db.apply_hub_receipt(owner(), operation(5), input.clone(), now())
        .await
        .unwrap();
    if let SourceAttribution::Referenced { source, .. } = &mut input.source {
        source.issuer = Some(id(5));
    }
    input.expected_stock_revision = HubRevision::from_value(6).unwrap();
    assert_eq!(
        db.apply_hub_receipt(owner(), operation(6), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.total, 5);
    let ids = page
        .rows
        .iter()
        .map(|row| match &row.trace.source {
            SourceAttribution::Referenced { source, .. } => source.id,
            _ => panic!(),
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                text: "%_".into(),
                ..query()
            }
        )
        .await
        .unwrap()
        .total,
        5
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn queries_are_bounded_and_continuations_bind_the_original_inventory_revision() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    for n in 1..=2 {
        let mut input = command(&catalog);
        input.expected_stock_revision = HubRevision::from_value(n).unwrap();
        db.apply_hub_receipt(owner(), operation(n as u32), input, now())
            .await
            .unwrap();
    }
    let first = db
        .hub_stock_page(
            owner(),
            HubStockQuery {
                limit: 1,
                ..query()
            },
        )
        .await
        .unwrap();
    assert!(first.more);
    let next = db
        .hub_stock_page(
            owner(),
            HubStockQuery {
                expected_catalog_revision: Some(first.catalog_revision),
                expected_revision: Some(first.revision),
                offset: 1,
                limit: 1,
                ..query()
            },
        )
        .await
        .unwrap();
    assert!(!next.more);
    assert_ne!(first.rows[0].position.group, next.rows[0].position.group);
    let mut input = command(&catalog);
    input.expected_stock_revision = first.revision;
    db.apply_hub_receipt(owner(), operation(3), input, now())
        .await
        .unwrap();
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                expected_catalog_revision: Some(first.catalog_revision),
                expected_revision: Some(first.revision),
                offset: 1,
                ..query()
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    for invalid in [
        HubStockQuery {
            limit: 0,
            ..query()
        },
        HubStockQuery {
            limit: 65,
            ..query()
        },
        HubStockQuery {
            text: "x".repeat(121),
            ..query()
        },
        HubStockQuery {
            offset: 4097,
            ..query()
        },
    ] {
        assert_eq!(
            db.hub_stock_page(owner(), invalid)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubInvalidInput
        );
    }
    db.close().await.unwrap();
}

#[tokio::test]
async fn oversized_original_observation_is_bounded_before_decoding() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("DROP TRIGGER hub_receipts_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("PRAGMA ignore_check_constraints=ON")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_receipts SET observation=?")
        .bind("x".repeat(1_000_000))
        .execute(&mut raw)
        .await
        .unwrap();
    let bounded: i64 = sqlx::query_scalar(
        "SELECT length(substr(CAST(observation AS BLOB),1,513)) FROM hub_receipts",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    assert_eq!(bounded, 513);
    sqlx::query("CREATE TRIGGER hub_receipts_no_update BEFORE UPDATE ON hub_receipts BEGIN SELECT RAISE(ABORT, 'original receipt is immutable'); END").execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn immutable_history_and_closed_occurrence_shapes_are_enforced_by_sql() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut input = command(&catalog);
    input.source = known_source(&operation(1), Some(id(5)));
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), input, now())
        .await
        .unwrap();
    db.host().transaction(|tx|Box::pin(async move {
        for sql in [
            "UPDATE hub_stock_operations SET kind=kind", "DELETE FROM hub_stock_operations",
            "UPDATE hub_source_references SET provenance=provenance", "DELETE FROM hub_source_references",
            "UPDATE hub_receipts SET original_packs=original_packs", "DELETE FROM hub_receipts",
            "UPDATE hub_stock_events SET packs=packs", "DELETE FROM hub_stock_events",
        ] { assert!(sqlx::query(sql).execute(&mut *tx).await.is_err(),"{sql}"); }
        assert!(sqlx::query("INSERT INTO hub_stock_events (sequence,operation_id,kind,trace_id,group_id,packs,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s) SELECT 2,operation_id,'receipt',trace_id,group_id,1,'known',90,NULL FROM hub_stock_events LIMIT 1")
            .execute(&mut *tx).await.is_err());
        Ok::<(),SafeError>(())
    })).await.unwrap();
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs
            .value(),
        20
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn exact_receipt_capacity_retains_original_replay_and_denies_growth_atomically() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let original_input = command(&catalog);
    let input = original_input.clone();
    let original = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut first = None;
                for n in 1..=HUB_TRACE_CAPACITY {
                    let mut current = input.clone();
                    current.expected_stock_revision = HubRevision::from_value(n as u64).unwrap();
                    let receipt =
                        super::write::apply_on(tx, owner(), operation(n as u32), current, now())
                            .await?;
                    if first.is_none() {
                        first = Some(receipt);
                    }
                }
                Ok::<_, SafeError>(first.unwrap())
            })
        })
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.total, HUB_TRACE_CAPACITY as u32);
    assert_eq!(page.rows.len(), 64);
    assert!(page.more);
    assert_eq!(
        db.apply_hub_receipt(owner(), operation(1), original_input, now())
            .await
            .unwrap(),
        original
    );
    let mut input = command(&catalog);
    input.expected_stock_revision = page.revision;
    let op = operation(HUB_TRACE_CAPACITY as u32 + 1);
    assert_eq!(
        db.apply_hub_receipt(owner(), op.clone(), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    assert!(
        db.hub_receipt_operation(owner(), op)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().revision,
        page.revision
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_stock_uses_current_names_while_original_receipt_versions_remain_immutable() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let original = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    for (n, record, value) in [
        (
            201,
            id(1),
            ReferenceValue::Product {
                label: HubLabel::new("Current product name").unwrap(),
            },
        ),
        (
            202,
            id(6),
            ReferenceValue::Location {
                label: HubLabel::new("Current Receiving name").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            203,
            id(7),
            ReferenceValue::Location {
                label: HubLabel::new("Current A3 name").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(n),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Revise {
                    id: record,
                    expected_record_revision: HubRevision::INITIAL,
                    value,
                },
            },
            now(),
        )
        .await
        .unwrap();
    }
    let page = db
        .hub_stock_page(
            owner(),
            HubStockQuery {
                text: "Current product name".into(),
                ..query()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let row = &page.rows[0];
    assert_eq!(row.product.value.label().as_str(), "Current product name");
    assert_eq!(
        row.location.value.label().as_str(),
        "Current Receiving name"
    );
    assert_eq!(
        row.work_target.as_ref().unwrap().value.label().as_str(),
        "Current A3 name"
    );
    assert_eq!(row.trace.product.value.label().as_str(), "Fictional apples");
    assert_eq!(row.trace.receiving.value.label().as_str(), "Receiving");
    assert_eq!(
        row.trace
            .planned_location
            .as_ref()
            .unwrap()
            .value
            .label()
            .as_str(),
        "A3"
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                text: "Fictional apples".into(),
                ..query()
            }
        )
        .await
        .unwrap()
        .total,
        1
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_reference_edits_invalidate_continuations_without_changing_physical_stock() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    for n in 1..=2 {
        let mut input = command(&catalog);
        input.expected_stock_revision = HubRevision::from_value(n).unwrap();
        db.apply_hub_receipt(owner(), operation(n as u32), input, now())
            .await
            .unwrap();
    }
    let first = db
        .hub_stock_page(
            owner(),
            HubStockQuery {
                limit: 1,
                ..query()
            },
        )
        .await
        .unwrap();
    assert!(first.more);
    db.apply_hub_reference(
        owner(),
        operation(201),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(1),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Product {
                    label: HubLabel::new("Current renamed apples").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let fresh = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(fresh.revision, first.revision);
    assert_ne!(fresh.catalog_revision, first.catalog_revision);
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                expected_revision: Some(first.revision),
                expected_catalog_revision: Some(first.catalog_revision),
                offset: 1,
                limit: 1,
                ..query()
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_stock_page(
            owner(),
            HubStockQuery {
                expected_revision: Some(first.revision),
                offset: 1,
                limit: 1,
                ..query()
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        fresh
            .rows
            .iter()
            .map(|row| row.position.packs.value())
            .collect::<Vec<_>>(),
        vec![20, 20]
    );
    db.close().await.unwrap();
}
