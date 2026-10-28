use super::*;
use harvestcircle_domain::hub::reference::{ReferenceKind, ReferenceMutation};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
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

    async fn closed_connection(&self) -> SqliteConnection {
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
fn operation(n: u32) -> DurableRequestId {
    DurableRequestId::parse(format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}
fn id(n: u16) -> HubRecordId {
    let mut bytes = [0; 16];
    bytes[14..].copy_from_slice(&n.to_be_bytes());
    HubRecordId::from_bytes(bytes).unwrap()
}
fn party(label: &str) -> ReferenceValue {
    ReferenceValue::Party {
        label: HubLabel::new(label).unwrap(),
    }
}
fn command(revision: HubRevision, n: u16, value: ReferenceValue) -> ReferenceCommand {
    ReferenceCommand {
        expected_catalog_revision: revision,
        mutation: ReferenceMutation::Create { id: id(n), value },
    }
}
async fn setup(database: &Database) {
    database
        .initialize_hub(
            HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), owner()),
            &operation(65_535),
            now(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn current_references_keep_products_packs_parties_and_locations_distinct() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_references(owner()).await.unwrap_err().code(),
        SafeErrorCode::HubPermissionDenied
    );
    setup(&db).await;
    let mut catalog = db.hub_references(owner()).await.unwrap();
    assert_eq!(catalog.records().len(), 0);
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
                label: HubLabel::new("Original 10 kg crate").unwrap(),
                nominal_grams: Some(NominalGrams::from_value(10_000).unwrap()),
            },
        ),
        (3, party("Fictional orchard")),
        (
            4,
            ReferenceValue::Location {
                label: HubLabel::new("Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
    ] {
        db.apply_hub_reference(
            owner(),
            operation(u32::from(n)),
            command(catalog.revision(), n, value),
            now(),
        )
        .await
        .unwrap();
        catalog = db.hub_references(owner()).await.unwrap();
    }
    assert_eq!(catalog.records().len(), 4);
    assert_eq!(catalog.handling_basis(), SampleHandlingBasis::WHOLE_PACK_V1);
    assert_eq!(
        catalog
            .require_kind(id(2), ReferenceKind::Pack)
            .unwrap()
            .value
            .label()
            .as_str(),
        "Original 10 kg crate"
    );
    let generation = db.metadata().source_generation();
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(reopened.metadata().source_generation(), generation);
    assert_eq!(reopened.hub_references(owner()).await.unwrap(), catalog);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn original_receipts_survive_later_edits_replay_and_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let original = command(HubRevision::INITIAL, 1, party("Fictional orchard"));
    let first = db
        .apply_hub_reference(owner(), operation(1), original.clone(), now())
        .await
        .unwrap();
    let revise = ReferenceCommand {
        expected_catalog_revision: first.change.catalog_revision,
        mutation: ReferenceMutation::Revise {
            id: id(1),
            expected_record_revision: HubRevision::INITIAL,
            value: party("Fictional orchard cooperative"),
        },
    };
    let changed = db
        .apply_hub_reference(owner(), operation(2), revise, now())
        .await
        .unwrap();
    assert_eq!(changed.change.previous, Some(first.change.current.clone()));
    assert_eq!(
        db.apply_hub_reference(
            owner(),
            operation(1),
            original.clone(),
            UnixTimestamp::from_seconds(900).unwrap()
        )
        .await
        .unwrap(),
        first
    );
    let replacement = command(HubRevision::INITIAL, 1, party("Changed intent"));
    assert_eq!(
        db.apply_hub_reference(owner(), operation(1), replacement, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_references(other()).await.unwrap_err().code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_reference_operation(other(), operation(1))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let records = db.hub_references(owner()).await.unwrap();
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_reference_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(first.clone())
    );
    assert_eq!(db.hub_references(owner()).await.unwrap(), records);
    assert_eq!(
        db.hub_reference_history(owner(), id(1), 0, 64)
            .await
            .unwrap(),
        vec![first, changed]
    );
    assert!(
        db.hub_reference_operation(owner(), operation(99))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.hub_reference_history(owner(), id(1), 0, 0)
            .await
            .is_err()
    );
    assert!(
        db.hub_reference_history(owner(), id(1), 0, 65)
            .await
            .is_err()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn competing_current_forms_commit_exactly_one_reference() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let first = db.apply_hub_reference(
        owner(),
        operation(1),
        command(HubRevision::INITIAL, 1, party("A")),
        now(),
    );
    let second = db.apply_hub_reference(
        owner(),
        operation(2),
        command(HubRevision::INITIAL, 2, party("B")),
        now(),
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first.err().or_else(|| second.err()).unwrap().code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(db.hub_references(owner()).await.unwrap().records().len(), 1);
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_transaction_after_production_insert_rolls_back_version_and_receipt() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let failed = db
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                apply_on(
                    transaction,
                    owner(),
                    operation(1),
                    command(HubRevision::INITIAL, 1, party("Original")),
                    now(),
                )
                .await?;
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(
        db.hub_references(owner()).await.unwrap(),
        ReferenceCatalog::default()
    );
    assert!(
        db.hub_reference_operation(owner(), operation(1))
            .await
            .unwrap()
            .is_none()
    );
    let receipt = db
        .apply_hub_reference(
            owner(),
            operation(1),
            command(HubRevision::INITIAL, 1, party("Original")),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.change.catalog_revision.value(), 2);
    for query in [
        "UPDATE hub_reference_versions SET label='overwrite'",
        "DELETE FROM hub_reference_versions",
        "UPDATE hub_handling_context SET basis_version=2",
        "DELETE FROM hub_handling_context",
    ] {
        let result = db
            .host()
            .transaction(|transaction| {
                Box::pin(async move {
                    sqlx::query(query)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                    Ok::<(), SafeError>(())
                })
            })
            .await;
        assert!(result.is_err());
    }
    assert_eq!(
        db.hub_reference_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn full_history_retains_original_operation_without_allowing_more_growth() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let original = command(HubRevision::INITIAL, 1, party("Original"));
    let first = db
        .apply_hub_reference(owner(), operation(1), original.clone(), now())
        .await
        .unwrap();
    // Test-owned bulk history fixture; the production adapter owns all subsequent
    // capacity decisions, original receipt reads and replay behavior.
    db.host().transaction(|transaction| Box::pin(async move {
        sqlx::query("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<16384) INSERT INTO hub_reference_versions (record_id,record_revision,catalog_revision,hub_id,kind,label,actor_public_key,operation_id,intent_hash,recorded_at_unix_s) SELECT ?,x,x+1,?,'party',printf('Version %d',x),?,printf('01900000-0000-7000-8000-%012x',x),zeroblob(32),100 FROM n")
            .bind(id(1).as_bytes().as_slice()).bind([1u8;16].as_slice()).bind(owner().as_bytes().as_slice())
            .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
        Ok::<(), SafeError>(())
    })).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    assert_eq!(catalog.revision().value(), 16_385);
    let next = ReferenceCommand {
        expected_catalog_revision: catalog.revision(),
        mutation: ReferenceMutation::Revise {
            id: id(1),
            expected_record_revision: HubRevision::from_value(16_384).unwrap(),
            value: party("One too many"),
        },
    };
    assert_eq!(
        db.apply_hub_reference(owner(), operation(20_000), next, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    assert_eq!(
        db.apply_hub_reference(owner(), operation(1), original, now())
            .await
            .unwrap(),
        first
    );
    assert_eq!(db.hub_references(owner()).await.unwrap(), catalog);
    assert_eq!(
        db.hub_reference_history(owner(), id(1), 0, 64)
            .await
            .unwrap()
            .len(),
        64
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn oversize_closed_host_fault_is_bounded_before_domain_decoding() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    db.apply_hub_reference(
        owner(),
        operation(1),
        command(HubRevision::INITIAL, 1, party("Original")),
        now(),
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let mut connection = fixture.closed_connection().await;
    sqlx::query("PRAGMA ignore_check_constraints=ON")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER hub_reference_no_update")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_reference_versions SET label=?")
        .bind("x".repeat(1_000_000))
        .execute(&mut connection)
        .await
        .unwrap();
    let query = reference_select!(" FROM hub_reference_versions LIMIT 1");
    let row = sqlx::query(query).fetch_one(&mut connection).await.unwrap();
    assert_eq!(row.get::<Vec<u8>, _>("label").len(), 121);
    assert_eq!(
        decode_record(&row).unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn current_reference_capacity_denies_growth_and_never_truncates_a_corrupt_catalog() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    db.host().transaction(|transaction| Box::pin(async move {
        sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<512) INSERT INTO hub_reference_versions (record_id,record_revision,catalog_revision,hub_id,kind,label,actor_public_key,operation_id,intent_hash,recorded_at_unix_s) SELECT CAST(printf('%016d',x) AS BLOB),1,x+1,?,'party',printf('Fictional party %d',x),?,printf('01900000-0000-7000-8000-%012x',x),zeroblob(32),100 FROM n")
            .bind([1u8;16].as_slice()).bind(owner().as_bytes().as_slice()).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
        Ok::<(), SafeError>(())
    })).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    assert_eq!(catalog.records().len(), HUB_REFERENCE_CAPACITY);
    assert_eq!(
        db.apply_hub_reference(
            owner(),
            operation(999),
            command(catalog.revision(), 999, party("One too many")),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubCapacity
    );
    assert_eq!(db.hub_references(owner()).await.unwrap(), catalog);
    // A test-only valid-SQL/invalid-product-state fault must not be silently
    // truncated by the bounded current-record query.
    db.host().transaction(|transaction| Box::pin(async move {
        sqlx::query("INSERT INTO hub_reference_versions (record_id,record_revision,catalog_revision,hub_id,kind,label,actor_public_key,operation_id,intent_hash,recorded_at_unix_s) VALUES (CAST('0000000000000513' AS BLOB),1,514,?,'party','Fault',?,'01900000-0000-7000-8000-000000000201',zeroblob(32),100)")
            .bind([1u8;16].as_slice()).bind(owner().as_bytes().as_slice()).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
        Ok::<(), SafeError>(())
    })).await.unwrap();
    assert_eq!(
        db.hub_references(owner()).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}
