//! HCAV-020: bounded public fixtures and the real governed storage boundary.

use super::*;
use std::fs;
use std::num::NonZeroU64;
use std::sync::Arc;

use harvestcircle_application::IdentityRepository;
use harvestcircle_domain::{
    AvailabilityEventVersion, AvailabilityObservation, AvailabilityUnsupportedReason,
    AvailabilityVersionView, EventId, IdentityCreatedAt, Kind0ProfileCandidate,
    LocalKeyringBinding, NostrIdentity, NostrIdentityReference, ProfileMetadata, SafeError,
    SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use radroots_event::envelope::event_head::EventHeadCoordinate;
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::wire::{DEFAULT_RAW_JSON_MAX_BYTES, Nip01EventWire};
use radroots_event_codec::verify::verify_nip01_event;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::{
    BackupCreatedAtUnixMs, MigrationAppliedAtUnixSeconds, MigrationBuildIdentity,
    ServiceDatabaseMetadata, ServiceSqliteConnectionOptions, ServiceSqliteHost,
    ServiceSqliteInitializer, ServiceSqliteInitializerFuture, ServiceSqliteTransactionErrorKind,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use sqlx::{Connection, Row, SqliteConnection};
use tempfile::{TempDir, tempdir_in};

use crate::availability_evidence::{
    SELECT_AVAILABILITY_VERSION_SQL, decode_availability_row, retain_availability_version_on,
};
use crate::{Database, verify_harvestcircle_backup};

const VERSION_CAPACITY: i64 = 4096;
const TOTAL_PAYLOAD_BYTES: i64 = 134_217_728;
const ORDINARY_PAYLOAD_BYTES: i64 = 125_829_120;
const RECOVERY_RESERVE_BYTES: i64 = 8_388_608;
const SOURCE: &str = "wss://evidence.example.invalid/public";
const OBSERVED_AT: i64 = 1_800_000_123;
const SNAPSHOT: &str = "SELECT \
    (SELECT group_concat(hex(public_key) || ':' || npub || ':' || label || ':' || created_at_unix_s, '|') FROM account_identities) AS accounts, \
    (SELECT group_concat(hex(account_public_key) || ':' || hex(binding_public_key) || ':' || availability, '|') FROM local_signer_bindings) AS bindings, \
    (SELECT group_concat(hex(subject_public_key) || ':' || hex(event_id) || ':' || name || ':' || refreshed_at_unix_s, '|') FROM profile_cache) AS profiles, \
    (SELECT group_concat(hex(owner_public_key) || ':' || preference_key || ':' || preference_value, '|') FROM account_preferences) AS preferences, \
    (SELECT group_concat(request_id || ':' || phase || ':' || updated_at_unix_s, '|') FROM durable_operations) AS journal, \
    (SELECT hex(installation_id) FROM installation_identity WHERE singleton = 1) AS installation, \
    (SELECT hex(selected_public_key) || ':' || hex(active_account_public_key) || ':' || session_generation FROM runtime_state WHERE singleton = 1) AS runtime";

struct Fixture {
    directory: TempDir,
    context: RuntimeContext,
    build: MigrationBuildIdentity,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempdir_in(std::env::temp_dir().canonicalize().expect("temporary root"))
            .expect("isolated directory");
        let context = RuntimeContext::resolve(
            &RadrootsPathResolver::new(
                RadrootsPlatform::current(),
                RadrootsHostEnvironment::default(),
            ),
            RuntimeContextBootstrap::new(
                RadrootsPathProfile::RepoLocal,
                Some(directory.path().canonicalize().expect("canonical root")),
                RuntimeContextSource::BootstrapCli,
                RuntimeContextSource::SafeDefault,
            )
            .expect("bootstrap"),
            ServiceId::new("harvestcircle").expect("service"),
            InstanceId::new("desktop").expect("instance"),
        )
        .expect("runtime context");
        fs::create_dir(directory.path().join("data")).expect("injected state root");
        let build = MigrationBuildIdentity::new(
            "0.1.0-alpha",
            "1111111111111111111111111111111111111111",
            "189c49b74b4bafc142b00b76b296477931139e72",
            "1.97.1",
            "test",
            "test",
            1,
            1,
            1,
            1,
            1,
        )
        .expect("migration evidence");
        Self {
            directory,
            context,
            build,
        }
    }

    async fn open(&self) -> Database {
        Database::open(&self.context, 99, 100, &self.build)
            .await
            .expect("governed database")
    }

    // This helper is called only after the sole governed host has closed.
    async fn closed_connection(&self) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(self.context.paths().state().join("state.sqlite"))
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .expect("closed-host test fault connection")
    }
}

#[derive(Clone, Copy)]
enum Recipe {
    Focused,
    Generic,
    Operational,
    Ambiguous,
    Rejected,
}

fn observation(source: &str, observed_at: i64) -> AvailabilityObservation {
    AvailabilityObservation::parse(
        source,
        UnixTimestamp::from_seconds(observed_at).expect("nonnegative observation"),
    )
    .expect("bounded named public provenance")
}

fn view_from_wire(json: &str, source: &str, observed_at: i64) -> AvailabilityVersionView {
    assert!(json.len() <= 262_144);
    let verified = verify_nip01_event(
        Nip01EventWire::parse_json_unverified(json)
            .expect("bounded public wire")
            .into_unverified_envelope()
            .expect("actual signed envelope"),
    )
    .expect("selected shared ID and Schnorr verification");
    AvailabilityVersionView::from_verified(verified, json, observation(source, observed_at))
        .expect("actual tolerant shared/domain view")
}

// No fixture secret escapes this builder or reaches a tested repository call.
fn public_view(
    identifier: &str,
    created_at: u64,
    publication: u64,
    recipe: Recipe,
    wire_bytes: Option<usize>,
) -> AvailabilityVersionView {
    let keys = Keys::generate();
    let json = public_wire(
        &keys,
        identifier,
        created_at,
        publication,
        recipe,
        wire_bytes,
    );
    drop(keys);
    view_from_wire(&json, SOURCE, OBSERVED_AT)
}

fn public_wire(
    keys: &Keys,
    identifier: &str,
    created_at: u64,
    publication: u64,
    recipe: Recipe,
    wire_bytes: Option<usize>,
) -> String {
    assert!(identifier.len() <= 4096);
    let mut tags: Vec<Vec<String>> = vec![
        vec!["d".into(), identifier.into()],
        vec!["title".into(), "Public storage listing".into()],
        vec!["summary".into(), "Public evidence fixture".into()],
        vec!["published_at".into(), publication.to_string()],
        vec!["location".into(), "Fixture location".into()],
        vec!["price".into(), "3.5".into(), "CAD".into()],
        vec!["status".into(), "active".into()],
    ];
    match recipe {
        Recipe::Focused | Recipe::Rejected => {
            tags.push(vec!["radroots:price_unit".into(), "lb".into()])
        }
        Recipe::Generic => {}
        Recipe::Operational => tags.push(vec!["radroots:primary_bin".into(), "public-bin".into()]),
        Recipe::Ambiguous => {
            tags.push(vec!["radroots:price_unit".into(), "lb".into()]);
            tags.push(vec!["radroots:primary_bin".into(), "public-bin".into()]);
        }
    }
    if matches!(recipe, Recipe::Rejected) {
        tags.retain(|tag| tag[0] != "price");
    }
    assert!(tags.len() <= 10 && tags.iter().map(Vec::len).sum::<usize>() <= 32);
    assert!(tags.iter().flatten().all(|value| value.len() <= 4096));
    let event = EventBuilder::new(
        Kind::from(30402_u16),
        "Public fixture; no custody or relay effects.",
    )
    .tags(
        tags.into_iter()
            .map(|tag| Tag::parse(tag).expect("bounded public tag")),
    )
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(keys)
    .expect("ephemeral public fixture signing");
    let compact = event.as_json();
    match wire_bytes {
        None => compact,
        Some(bytes) => {
            assert!(bytes <= 262_144 && bytes >= compact.len() + 2);
            format!("\n{}{}\n", compact, " ".repeat(bytes - compact.len() - 2))
        }
    }
}

fn focused(identifier: &str) -> AvailabilityVersionView {
    public_view(
        identifier,
        1_800_000_100,
        1_800_000_000,
        Recipe::Focused,
        None,
    )
}

fn raw_d(view: &AvailabilityVersionView) -> &str {
    match view.raw_coordinate() {
        EventHeadCoordinate::Addressable { kind, d_tag, .. } => {
            assert_eq!(*kind, 30402);
            d_tag
        }
        _ => panic!("kind-30402 fixture has an addressable raw coordinate"),
    }
}

fn expected_charge(view: &AvailabilityVersionView, label: &str, reason: Option<&str>) -> i64 {
    // Independent frozen accounting; this never calls a production charge helper.
    i64::try_from(
        92 + view.original_json().len()
            + raw_d(view).len()
            + view.observation().source().as_str().len()
            + label.len()
            + reason.map_or(0, str::len),
    )
    .expect("bounded logical payload charge")
}

async fn rows(host: &ServiceSqliteHost, sql: &'static str) -> Vec<SqliteRow> {
    host.transaction(|transaction| {
        Box::pin(async move { sqlx::query(sql).fetch_all(&mut *transaction).await })
    })
    .await
    .expect("governed inspection")
}

async fn usage(database: &Database) -> (i64, i64) {
    let rows = rows(
        database.host(),
        "SELECT version_count, payload_bytes FROM public_payload_usage WHERE singleton = 1",
    )
    .await;
    assert_eq!(rows.len(), 1, "one global public payload meter");
    (rows[0].get("version_count"), rows[0].get("payload_bytes"))
}

async fn assert_usage(database: &Database, count: i64, bytes: i64) {
    assert_eq!(usage(database).await, (count, bytes));
    let rows = rows(database.host(), "SELECT count(*) AS versions, coalesce(sum(payload_bytes), 0) AS charged, coalesce(sum(92 + length(CAST(original_json AS BLOB)) + length(CAST(raw_d AS BLOB)) + length(CAST(source AS BLOB)) + length(CAST(admission_label AS BLOB)) + coalesce(length(CAST(rejection_code AS BLOB)), 0)), 0) AS actual_charge FROM availability_versions").await;
    assert_eq!(rows[0].get::<i64, _>("versions"), count);
    assert_eq!(rows[0].get::<i64, _>("charged"), bytes);
    assert_eq!(rows[0].get::<i64, _>("actual_charge"), bytes);
}

async fn stored_row(database: &Database, version: AvailabilityEventVersion) -> SqliteRow {
    database
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                sqlx::query("SELECT * FROM availability_versions WHERE event_id = ?")
                    .bind(version.event_id().as_bytes().as_slice())
                    .fetch_one(&mut *transaction)
                    .await
            })
        })
        .await
        .expect("small valid retained row inspection")
}

fn assert_round_trip(actual: &AvailabilityVersionView, expected: &AvailabilityVersionView) {
    assert_eq!(actual.version(), expected.version());
    assert_eq!(actual.publisher(), expected.publisher());
    assert_eq!(actual.created_at(), expected.created_at());
    assert_eq!(actual.raw_coordinate(), expected.raw_coordinate());
    assert_eq!(actual.listing_coordinate(), expected.listing_coordinate());
    assert_eq!(actual.original_json(), expected.original_json());
    assert_eq!(
        actual.observation().source().as_str(),
        expected.observation().source().as_str()
    );
    assert_eq!(
        actual.observation().observed_at(),
        expected.observation().observed_at()
    );
    assert_eq!(actual.focused(), expected.focused());
    assert_eq!(actual.unsupported_reason(), expected.unsupported_reason());
    assert!(actual.profile_metadata().is_none());
}

fn legacy_catalogs() -> (MigrationCatalog, SchemaCatalog) {
    let migration = MigrationDescriptor::sql(
        2,
        "bound_durable_operation_receipts",
        MIGRATE_DURABLE_OPERATIONS_V2_SQL,
        DURABLE_OPERATIONS_V2_MIGRATION_CHECKSUM,
    )
    .expect("historical migration");
    let migrations = MigrationCatalog::new([migration]).expect("historical catalog");
    let one = SchemaVersionCatalog::new(
        1,
        schema_objects(CREATE_DURABLE_OPERATIONS_SQL).expect("v1 objects"),
        SchemaDigest::from_bytes(VERSION_ONE_DIGEST),
    )
    .expect("v1 snapshot");
    let two = SchemaVersionCatalog::new(
        2,
        schema_objects(CREATE_DURABLE_OPERATIONS_V2_SQL).expect("v2 objects"),
        SchemaDigest::from_bytes(VERSION_TWO_DIGEST),
    )
    .expect("v2 snapshot");
    let schema = SchemaCatalog::new(&migrations, [one, two]).expect("historical schema");
    (migrations, schema)
}

fn legacy_initializer<'a>(
    initializer: &'a mut ServiceSqliteInitializer<'_>,
) -> ServiceSqliteInitializerFuture<'a, sqlx::Error> {
    Box::pin(async move {
        for statement in harvestcircle_initial_schema_sql() {
            sqlx::query(*statement).execute(&mut *initializer).await?;
        }
        sqlx::query("INSERT INTO runtime_state (singleton) VALUES (1)")
            .execute(&mut *initializer)
            .await?;
        Ok(())
    })
}

async fn legacy_host(fixture: &Fixture) -> (ServiceSqliteHost, ServiceDatabaseMetadata) {
    let contract =
        HarvestCircleStorageContract::from_runtime_context(&fixture.context).expect("paths");
    let (migrations, schema) = legacy_catalogs();
    fixture
        .context
        .state_directory_plan()
        .expect("state plan")
        .provision()
        .expect("exact service suffix");
    // A public source identity fixture is not a key, counter or admission proof.
    let generation =
        radroots_storage::event::SourceGeneration::new([71; 32]).expect("public source identity");
    let metadata = ServiceDatabaseMetadata::new(
        contract.paths(),
        generation,
        NonZeroU32::new(1).unwrap(),
        17,
        contract.application_id(),
    )
    .expect("historical metadata");
    let (opened, migration) = ServiceSqliteHost::open_or_initialize(
        contract.paths(),
        &metadata,
        &migrations,
        &schema,
        ServiceSqliteConnectionOptions::reviewed(),
        MigrationAppliedAtUnixSeconds::new(18).unwrap(),
        &fixture.build,
        &[],
        legacy_initializer,
    )
    .await
    .expect("real governed v1-to-v2 setup");
    assert_eq!(migration.final_version(), 2);
    opened.into_parts()
}

async fn seed_legacy_state(host: &ServiceSqliteHost) -> Vec<Option<String>> {
    let public = focused("legacy-public-author").publisher().public_key();
    let npub = NostrIdentityReference::derive(public)
        .expect("public reference")
        .npub()
        .as_str()
        .to_owned();
    host.transaction(|transaction| Box::pin(async move {
        sqlx::query("INSERT INTO account_identities (public_key, npub, label, created_at_unix_s) VALUES (?, ?, 'retained-account', 11)").bind(public.as_bytes().as_slice()).bind(npub).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO local_signer_bindings (account_public_key, binding_public_key, binding_kind, availability) VALUES (?, ?, 'local_secret', 'credential_missing')").bind(public.as_bytes().as_slice()).bind(public.as_bytes().as_slice()).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO profile_cache (subject_public_key, event_id, event_created_at_unix_s, name, refreshed_at_unix_s, refresh_status) VALUES (?, ?, 12, 'retained-profile', 13, 'success')").bind(public.as_bytes().as_slice()).bind([73_u8; 32].as_slice()).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO account_preferences (owner_public_key, preference_key, preference_value) VALUES (?, 'namespace_probe', 'retained-preference')").bind(public.as_bytes().as_slice()).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO durable_operations (request_id, operation_kind, account_public_key, binding_public_key, phase, updated_at_unix_s) VALUES ('01890f3e-7b1c-7000-8000-000000000011', 'import', ?, ?, 'intent_recorded', 14)").bind(public.as_bytes().as_slice()).bind(public.as_bytes().as_slice()).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO installation_identity (singleton, installation_id) VALUES (1, ?)").bind([72_u8; 16].as_slice()).execute(&mut *transaction).await?;
        sqlx::query("UPDATE runtime_state SET selected_public_key = ?, active_account_public_key = ?, active_binding_public_key = ?, session_generation = 17 WHERE singleton = 1").bind(public.as_bytes().as_slice()).bind(public.as_bytes().as_slice()).bind(public.as_bytes().as_slice()).execute(&mut *transaction).await?;
        Ok::<_, sqlx::Error>(())
    })).await.expect("legacy product state");
    snapshot(host).await
}

async fn snapshot(host: &ServiceSqliteHost) -> Vec<Option<String>> {
    let rows = rows(host, SNAPSHOT).await;
    (0..7).map(|index| rows[0].get(index)).collect()
}

async fn bounded_fault_row(
    connection: &mut SqliteConnection,
    version: AvailabilityEventVersion,
) -> SqliteRow {
    sqlx::query(SELECT_AVAILABILITY_VERSION_SQL)
        .bind(version.event_id().as_bytes().as_slice())
        .fetch_one(connection)
        .await
        .expect("EXACT production bounded SELECT on a closed-host fault fixture")
}

async fn fill_count(database: &Database, count: usize) -> (AvailabilityVersionView, i64) {
    assert!(count <= 4096);
    let mut first = None;
    let mut bytes = 0;
    for index in 0..count {
        let view = focused(&format!("capacity-{index}"));
        bytes += expected_charge(&view, "focused", None);
        if index == 0 {
            first = Some(view.clone());
        }
        database
            .retain_availability_version(view)
            .await
            .expect("actual verified row admission");
    }
    (first.expect("nonzero streamed capacity setup"), bytes)
}

#[test]
fn historical_v1_v2_catalogs_preserve_exact_sql_and_digests() {
    let migrations = harvestcircle_migration_catalog().expect("current migrations");
    let schema = harvestcircle_schema_catalog().expect("current schemas");
    assert_eq!(migrations.current_version(), 3);
    assert_eq!(migrations.descriptors().len(), 2);
    let previous = &migrations.descriptors()[0];
    assert_eq!(previous.target_version(), 2);
    assert_eq!(previous.name().as_str(), "bound_durable_operation_receipts");
    assert_eq!(
        *previous.checksum().as_bytes(),
        [
            107, 95, 237, 250, 255, 0, 44, 110, 142, 194, 92, 163, 84, 27, 96, 31, 210, 37, 151,
            186, 210, 83, 137, 114, 251, 20, 30, 31, 11, 136, 207, 168
        ]
    );
    assert_eq!(
        MigrationChecksum::for_sql(MIGRATE_DURABLE_OPERATIONS_V2_SQL),
        previous.checksum()
    );
    assert_eq!(schema.versions().len(), 3);
    assert_eq!(schema.versions()[0].version(), 1);
    assert_eq!(
        *schema.versions()[0].digest().as_bytes(),
        [
            61, 122, 56, 39, 178, 126, 179, 157, 145, 167, 19, 2, 172, 134, 213, 107, 151, 196,
            212, 57, 17, 112, 163, 67, 240, 140, 61, 62, 5, 101, 14, 71
        ]
    );
    assert_eq!(schema.versions()[1].version(), 2);
    assert_eq!(
        *schema.versions()[1].digest().as_bytes(),
        [
            78, 151, 73, 238, 2, 15, 71, 52, 11, 111, 100, 95, 135, 11, 170, 138, 84, 105, 106,
            177, 27, 7, 134, 156, 53, 68, 22, 220, 116, 199, 147, 5
        ]
    );
    for (version, sql, service_count) in [
        (1, CREATE_DURABLE_OPERATIONS_SQL, 9),
        (2, CREATE_DURABLE_OPERATIONS_V2_SQL, 11),
    ] {
        let objects = schema_objects(sql).expect("historical objects");
        assert_eq!(objects.len(), service_count);
        for object in &objects {
            assert_eq!(
                object.digest(),
                SchemaObject::computed_digest(
                    object.kind(),
                    object.name(),
                    object.table_name(),
                    match object.name() {
                        "account_identities" => CREATE_ACCOUNT_IDENTITIES_SQL,
                        "local_signer_bindings" => CREATE_LOCAL_SIGNER_BINDINGS_SQL,
                        "runtime_state" => CREATE_RUNTIME_STATE_SQL,
                        "profile_cache" => CREATE_PROFILE_CACHE_SQL,
                        "account_preferences" => CREATE_ACCOUNT_PREFERENCES_SQL,
                        "durable_operations" => sql,
                        "installation_identity" => CREATE_INSTALLATION_IDENTITY_SQL,
                        "installation_identity_no_update" =>
                            CREATE_INSTALLATION_IDENTITY_NO_UPDATE_SQL,
                        "installation_identity_no_delete" =>
                            CREATE_INSTALLATION_IDENTITY_NO_DELETE_SQL,
                        "durable_operations_receipt_insert_guard" =>
                            CREATE_DURABLE_OPERATIONS_RECEIPT_INSERT_GUARD_SQL,
                        "durable_operations_receipt_update_guard" =>
                            CREATE_DURABLE_OPERATIONS_RECEIPT_UPDATE_GUARD_SQL,
                        _ => panic!("unallocated historical object"),
                    }
                )
                .expect("independent historical object digest")
            );
        }
        assert_eq!(
            SchemaVersionCatalog::computed_digest(version, objects).unwrap(),
            schema.versions()[version as usize - 1].digest()
        );
    }
    assert_eq!(harvestcircle_initial_schema_sql().len(), 9);
}

#[tokio::test]
async fn fresh_governed_database_installs_allocated_evidence_schema() {
    let fixture = Fixture::new();
    let database = fixture.open().await;
    assert_eq!(database.metadata().state_schema_version().get(), 3);
    assert_eq!(database.metadata().service().as_str(), "harvestcircle");
    assert_eq!(database.metadata().instance().as_str(), "desktop");
    assert_eq!(database.metadata().created_at_unix_ms(), 99);
    assert_usage(&database, 0, 0).await;
    let ledger = rows(
        database.host(),
        "SELECT version, name, checksum FROM schema_migrations ORDER BY version",
    )
    .await;
    assert_eq!(ledger.len(), 2);
    assert_eq!(ledger[0].get::<i64, _>("version"), 2);
    assert_eq!(ledger[1].get::<i64, _>("version"), 3);
    assert_eq!(
        ledger[1].get::<String, _>("name"),
        "add_verified_listing_evidence"
    );
    assert_eq!(
        ledger[1].get::<Vec<u8>, _>("checksum"),
        MigrationChecksum::for_sql(MIGRATE_AVAILABILITY_EVIDENCE_V3_SQL).as_bytes()
    );
    let schema = harvestcircle_schema_catalog().unwrap();
    let objects = schema_objects_v3().expect("real new catalog assembly");
    let actual = rows(
        database.host(),
        "SELECT name, tbl_name, sql FROM sqlite_schema WHERE sql IS NOT NULL",
    )
    .await;
    for object in &objects {
        let row = actual
            .iter()
            .find(|row| row.get::<String, _>("name") == object.name())
            .expect("actual schema object");
        let sql: String = row.get("sql");
        assert_eq!(row.get::<String, _>("tbl_name"), object.table_name());
        assert_eq!(
            SchemaObject::computed_digest(object.kind(), object.name(), object.table_name(), &sql)
                .unwrap(),
            object.digest()
        );
    }
    assert_eq!(
        SchemaVersionCatalog::computed_digest(3, objects).unwrap(),
        schema.versions()[2].digest()
    );
    assert_ne!(schema.versions()[2].digest(), schema.versions()[1].digest());
    let constrained = focused("sql-constraints");
    database
        .retain_availability_version(constrained)
        .await
        .unwrap();
    database.close().await.unwrap();
    let mut connection = fixture.closed_connection().await;
    assert!(sqlx::query("INSERT INTO public_payload_usage (singleton, version_count, payload_bytes) VALUES (2, 0, 0)").execute(&mut connection).await.is_err());
    for sql in [
        "UPDATE public_payload_usage SET version_count = -1",
        "UPDATE public_payload_usage SET version_count = 4097",
        "UPDATE public_payload_usage SET payload_bytes = -1",
        "UPDATE public_payload_usage SET payload_bytes = 134217729",
        "UPDATE availability_versions SET event_id = zeroblob(31)",
        "UPDATE availability_versions SET author = zeroblob(31)",
        "UPDATE availability_versions SET kind = 30403",
        "UPDATE availability_versions SET signed_at = zeroblob(7)",
        "UPDATE availability_versions SET published_at = zeroblob(9)",
        "UPDATE availability_versions SET observed_at_unix_s = -1",
        "UPDATE availability_versions SET admission_label = 'unknown'",
        "UPDATE availability_versions SET rejection_code = 'unexpected'",
        "UPDATE availability_versions SET original_json = zeroblob(10)",
    ] {
        assert!(
            sqlx::query(sql).execute(&mut connection).await.is_err(),
            "global bounds must be SQL constraints"
        );
    }
    for (sql, value) in [
        (
            "UPDATE availability_versions SET original_json = ?",
            "x".repeat(262_145),
        ),
        (
            "UPDATE availability_versions SET raw_d = ?",
            "é".repeat(2049),
        ),
        (
            "UPDATE availability_versions SET source = ?",
            "x".repeat(2049),
        ),
    ] {
        assert!(
            sqlx::query(sql)
                .bind(value)
                .execute(&mut connection)
                .await
                .is_err(),
            "UTF-8 limits use BLOB byte length"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pragma_foreign_key_list('availability_versions')"
        )
        .fetch_one(&mut connection)
        .await
        .unwrap(),
        0
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn governed_v2_upgrade_preserves_original_state_and_source_identity() {
    let fixture = Fixture::new();
    let (host, before) = legacy_host(&fixture).await;
    let state = seed_legacy_state(&host).await;
    assert!(state.iter().all(Option::is_some));
    host.close().await.unwrap();
    let database = fixture.open().await;
    assert_eq!(database.metadata().state_schema_version().get(), 3);
    assert_eq!(
        database.metadata().source_generation(),
        before.source_generation()
    );
    assert_eq!(database.metadata().created_at_unix_ms(), 17);
    assert_eq!(
        database.metadata().application_id(),
        before.application_id()
    );
    assert_eq!(snapshot(database.host()).await, state);
    assert_usage(&database, 0, 0).await;
    database.close().await.unwrap();
}

#[tokio::test]
async fn invalid_v1_receipt_migration_rolls_back_without_partial_schema() {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for sql in harvestcircle_initial_schema_sql() {
        sqlx::query(*sql).execute(&mut connection).await.unwrap();
    }
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO durable_operations (request_id, operation_kind, account_public_key, binding_public_key, phase, updated_at_unix_s) VALUES ('01890f3e-7b1c-7000-8000-000000000013', 'create', ?, ?, 'finalized', 12)")
        .bind([9_u8;32].as_slice()).bind([9_u8;32].as_slice()).execute(&mut connection).await.unwrap();
    sqlx::query("PRAGMA ignore_check_constraints = OFF")
        .execute(&mut connection)
        .await
        .unwrap();
    let mut transaction = connection.begin().await.unwrap();
    assert!(
        sqlx::raw_sql(MIGRATE_DURABLE_OPERATIONS_V2_SQL)
            .execute(&mut *transaction)
            .await
            .is_err()
    );
    transaction.rollback().await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pragma_table_info('durable_operations') WHERE name = 'completed_at_unix_s'").fetch_one(&mut connection).await.unwrap(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'durable_operations_receipt_%'"
        )
        .fetch_one(&mut connection)
        .await
        .unwrap(),
        0
    );
    let row = sqlx::query("SELECT phase, updated_at_unix_s FROM durable_operations")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("phase"), "finalized");
    assert_eq!(row.get::<i64, _>("updated_at_unix_s"), 12);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn invalid_v2_database_is_refused_before_migration_without_replacement() {
    let fixture = Fixture::new();
    let (host, metadata) = legacy_host(&fixture).await;
    seed_legacy_state(&host).await;
    host.close().await.unwrap();
    let mut connection = fixture.closed_connection().await;
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("UPDATE account_preferences SET preference_value = ''")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("PRAGMA ignore_check_constraints = OFF")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let error = match Database::open(&fixture.context, 900, 901, &fixture.build).await {
        Ok(_) => panic!("invalid v2 must be refused"),
        Err(error) => error,
    };
    assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
    // This is observed preflight refusal, not a claimed migration rollback.
    let mut connection = fixture.closed_connection().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>("SELECT source_generation FROM radroots_service_metadata")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        metadata.source_generation().as_bytes()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT preference_value FROM account_preferences")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        ""
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_identities")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        1
    );
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sqlite_schema WHERE name IN ('availability_versions', 'public_payload_usage')").fetch_one(&mut connection).await.unwrap(), 0);
    connection.close().await.unwrap();
}

async fn rollback_evidence_creation(fixture: &Fixture) -> Vec<Option<String>> {
    let (host, metadata) = legacy_host(fixture).await;
    let before = seed_legacy_state(&host).await;
    let injected = SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The test operation was refused."),
    );
    let error = host.transaction(|transaction| Box::pin(async move {
        sqlx::raw_sql(MIGRATE_AVAILABILITY_EVIDENCE_V3_SQL).execute(&mut *transaction).await.map_err(|_| injected)?;
        let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name IN ('availability_versions', 'public_payload_usage')").fetch_one(&mut *transaction).await.map_err(|_| injected)?;
        assert_eq!(objects, 2, "v3 DDL was actually executed before the fault");
        let meter: (i64, i64) = sqlx::query_as("SELECT version_count, payload_bytes FROM public_payload_usage").fetch_one(&mut *transaction).await.map_err(|_| injected)?;
        assert_eq!(meter, (0, 0), "allocated singleton was visible within transaction");
        Err::<(), _>(injected)
    })).await.expect_err("deliberate operation failure after real DDL");
    assert_eq!(
        error.kind(),
        ServiceSqliteTransactionErrorKind::OperationRolledBack
    );
    assert_eq!(error.operation_error(), Some(&injected));
    assert_eq!(snapshot(&host).await, before);
    let absent = rows(&host, "SELECT count(*) AS objects FROM sqlite_schema WHERE name IN ('availability_versions', 'public_payload_usage')").await;
    assert_eq!(absent[0].get::<i64, _>("objects"), 0);
    let retained = rows(
        &host,
        "SELECT state_schema_version, source_generation FROM radroots_service_metadata",
    )
    .await;
    assert_eq!(retained[0].get::<i64, _>("state_schema_version"), 2);
    assert_eq!(
        retained[0].get::<Vec<u8>, _>("source_generation"),
        metadata.source_generation().as_bytes()
    );
    host.close().await.unwrap();
    before
}

#[tokio::test]
async fn failed_governed_evidence_creation_rolls_back_objects_and_counters() {
    let fixture = Fixture::new();
    rollback_evidence_creation(&fixture).await;
    let mut connection = fixture.closed_connection().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM schema_migrations WHERE version = 3")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        0
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn real_migration_succeeds_after_rolled_back_creation() {
    let fixture = Fixture::new();
    let before = rollback_evidence_creation(&fixture).await;
    let database = fixture.open().await;
    assert_eq!(database.metadata().state_schema_version().get(), 3);
    assert_eq!(snapshot(database.host()).await, before);
    assert_usage(&database, 0, 0).await;
    let public = focused("after-real-retry");
    database
        .retain_availability_version(public.clone())
        .await
        .unwrap();
    assert_round_trip(
        &database
            .load_availability_version(public.version())
            .await
            .unwrap()
            .unwrap(),
        &public,
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn future_schema_and_integrity_failure_preserve_original_state() {
    for future in [false, true] {
        let fixture = Fixture::new();
        let database = fixture.open().await;
        let public = focused("preserved-on-refusal");
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        let source = database.metadata().source_generation();
        database.close().await.unwrap();
        let mut connection = fixture.closed_connection().await;
        if future {
            sqlx::query("UPDATE radroots_service_metadata SET state_schema_version = 4")
                .execute(&mut connection)
                .await
                .unwrap();
        } else {
            sqlx::query("CREATE TABLE unexpected_evidence (value TEXT)")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let error = match Database::open(&fixture.context, 700, 701, &fixture.build).await {
            Ok(_) => panic!("future or foreign schema must be refused"),
            Err(error) => error,
        };
        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
        let mut connection = fixture.closed_connection().await;
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT source_generation FROM radroots_service_metadata"
            )
            .fetch_one(&mut connection)
            .await
            .unwrap(),
            source.as_bytes()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT original_json FROM availability_versions")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            public.original_json()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT version_count FROM public_payload_usage")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            1
        );
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn remote_version_retention_creates_no_account_or_signer() {
    let public = focused("remote-public-only");
    let fixture = Fixture::new();
    let database = fixture.open().await;
    assert_eq!(
        database
            .load_availability_version(public.version())
            .await
            .unwrap(),
        None
    );
    database
        .retain_availability_version(public.clone())
        .await
        .unwrap();
    assert_round_trip(
        &database
            .load_availability_version(public.version())
            .await
            .unwrap()
            .unwrap(),
        &public,
    );
    let rows = rows(database.host(), "SELECT (SELECT count(*) FROM account_identities) AS accounts, (SELECT count(*) FROM local_signer_bindings) AS signers, (SELECT count(*) FROM profile_cache) AS profiles, (SELECT count(*) FROM installation_identity) AS installations, (SELECT count(*) FROM durable_operations) AS operations").await;
    for column in [
        "accounts",
        "signers",
        "profiles",
        "installations",
        "operations",
    ] {
        assert_eq!(rows[0].get::<i64, _>(column), 0);
    }
    assert_usage(&database, 1, expected_charge(&public, "focused", None)).await;
    database.close().await.unwrap();
    assert_eq!(
        database
            .load_availability_version(public.version())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageUnavailable
    );
    assert_eq!(
        database
            .retain_availability_version(public)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageUnavailable
    );
}

#[tokio::test]
async fn focused_version_round_trips_exact_wire_admission_and_provenance() {
    let public = focused("exact-wire");
    let original = format!(
        "\n  {},\"unauthenticated_extra\":\"public observed bytes\"}}  \n",
        public.original_json().strip_suffix('}').unwrap()
    );
    let long_source = format!(
        "wss://evidence.example.invalid/{}",
        "x".repeat(2048 - "wss://evidence.example.invalid/".len())
    );
    assert_eq!(long_source.len(), 2048);
    let expected = view_from_wire(&original, &long_source, i64::MAX);
    assert!(expected.focused().is_some());
    let fixture = Fixture::new();
    let database = fixture.open().await;
    database
        .retain_availability_version(expected.clone())
        .await
        .unwrap();
    let row = stored_row(&database, expected.version()).await;
    assert_eq!(row.get::<String, _>("original_json"), original);
    assert_eq!(row.get::<String, _>("raw_d"), "exact-wire");
    assert_eq!(row.get::<String, _>("source"), long_source);
    assert_eq!(row.get::<i64, _>("observed_at_unix_s"), i64::MAX);
    assert_eq!(row.get::<String, _>("admission_label"), "focused");
    assert_eq!(row.get::<Option<String>, _>("rejection_code"), None);
    assert_eq!(row.get::<i64, _>("kind"), 30402);
    assert_eq!(
        row.get::<Vec<u8>, _>("event_id"),
        expected.version().event_id().as_bytes()
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("author"),
        expected.publisher().public_key().as_bytes()
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("signed_at"),
        1_800_000_100_u64.to_be_bytes()
    );
    assert_eq!(
        row.get::<Option<Vec<u8>>, _>("published_at"),
        Some(1_800_000_000_u64.to_be_bytes().to_vec())
    );
    assert_usage(&database, 1, expected_charge(&expected, "focused", None)).await;
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_round_trip(
        &reopened
            .load_availability_version(expected.version())
            .await
            .unwrap()
            .unwrap(),
        &expected,
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_empty_and_overlong_raw_coordinates_remain_evidence() {
    let identifiers = [
        String::new(),
        "d".repeat(4026),
        "d".repeat(4096),
        "é".repeat(2048),
        " opaque:\0:é ".into(),
    ];
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let mut bytes = 0;
    for identifier in &identifiers {
        let public = public_view(identifier, 100, 90, Recipe::Focused, None);
        assert_eq!(raw_d(&public), identifier);
        assert!(public.focused().is_none());
        let Some(AvailabilityUnsupportedReason::ProjectionRejected(reason)) =
            public.unsupported_reason()
        else {
            panic!("shared strict projection rejects broader raw evidence")
        };
        assert!(!reason.is_empty() && reason.len() <= 64 && reason.is_ascii());
        if identifier.is_empty() || identifier.len() > 4025 {
            assert!(public.listing_coordinate().is_none());
        }
        bytes += expected_charge(&public, "projection_rejected", Some(reason));
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        let row = stored_row(&database, public.version()).await;
        assert_eq!(row.get::<String, _>("raw_d"), *identifier);
        assert_eq!(
            row.get::<String, _>("admission_label"),
            "projection_rejected"
        );
        assert_eq!(
            row.get::<Option<String>, _>("rejection_code").as_deref(),
            Some(*reason)
        );
        assert_eq!(row.get::<Option<Vec<u8>>, _>("published_at"), None);
        assert_round_trip(
            &database
                .load_availability_version(public.version())
                .await
                .unwrap()
                .unwrap(),
            &public,
        );
    }
    assert_usage(&database, 5, bytes).await;
    database.close().await.unwrap();
}

#[tokio::test]
async fn signed_and_publication_timestamps_preserve_full_u64_range() {
    let values = [0, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX];
    let fixture = Fixture::new();
    let database = fixture.open().await;
    for (index, value) in values.into_iter().enumerate() {
        let public = public_view(
            &format!("unsigned-{index}"),
            value,
            value.max(1),
            if value == 0 {
                Recipe::Generic
            } else {
                Recipe::Focused
            },
            None,
        );
        assert_eq!(public.created_at().as_u64(), value);
        if value != 0 {
            assert_eq!(public.focused().unwrap().published_at().as_u64(), value);
        }
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        let row = stored_row(&database, public.version()).await;
        assert_eq!(row.get::<Vec<u8>, _>("signed_at"), value.to_be_bytes());
        assert_eq!(
            row.get::<Option<Vec<u8>>, _>("published_at"),
            (value != 0).then(|| value.to_be_bytes().to_vec())
        );
        assert_round_trip(
            &database
                .load_availability_version(public.version())
                .await
                .unwrap()
                .unwrap(),
            &public,
        );
    }
    let rows = rows(
        database.host(),
        "SELECT signed_at FROM availability_versions ORDER BY signed_at",
    )
    .await;
    assert_eq!(
        rows.iter()
            .map(|row| u64::from_be_bytes(row.get::<Vec<u8>, _>("signed_at").try_into().unwrap()))
            .collect::<Vec<_>>(),
        values
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn exact_id_duplicates_preserve_first_wire_and_provenance() {
    let original = focused("same-signed-id");
    let first_wire = format!("  {}\n", original.original_json());
    let first = view_from_wire(&first_wire, SOURCE, 10);
    let second_wire = format!(
        "{},\"extra\":\"different unauthenticated observation\"}}",
        original.original_json().strip_suffix('}').unwrap()
    );
    let later = view_from_wire(&second_wire, "wss://later.example.invalid/public", 20);
    assert_eq!(first.version(), later.version());
    assert_ne!(first.original_json(), later.original_json());
    let fixture = Fixture::new();
    let database = fixture.open().await;
    database
        .retain_availability_version(first.clone())
        .await
        .unwrap();
    database.retain_availability_version(later).await.unwrap();
    database
        .retain_availability_version(original)
        .await
        .unwrap();
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    assert_usage(&database, 1, expected_charge(&first, "focused", None)).await;
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_round_trip(
        &reopened
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn distinct_versions_of_one_coordinate_preserve_history() {
    let keys = Keys::generate();
    let first_wire = public_wire(&keys, "history", 100, 90, Recipe::Focused, None);
    let later_wire = public_wire(&keys, "history", 101, 90, Recipe::Generic, None);
    drop(keys);
    let first = view_from_wire(&first_wire, SOURCE, 1);
    let later = view_from_wire(&later_wire, SOURCE, 2);
    assert_eq!(first.raw_coordinate(), later.raw_coordinate());
    assert_ne!(first.version(), later.version());
    assert!(first.focused().is_some());
    assert_eq!(
        later.unsupported_reason(),
        Some(&AvailabilityUnsupportedReason::Excluded(
            ClassifiedListingPartition::GenericNip99
        ))
    );
    let fixture = Fixture::new();
    let database = fixture.open().await;
    database
        .retain_availability_version(later.clone())
        .await
        .unwrap();
    database
        .retain_availability_version(first.clone())
        .await
        .unwrap();
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    assert_round_trip(
        &database
            .load_availability_version(later.version())
            .await
            .unwrap()
            .unwrap(),
        &later,
    );
    assert_usage(
        &database,
        2,
        expected_charge(&first, "focused", None)
            + expected_charge(&later, "excluded_generic", None),
    )
    .await;
    let heads = rows(
        database.host(),
        "SELECT count(*) AS heads FROM sqlite_schema WHERE name = 'availability_heads'",
    )
    .await;
    assert_eq!(
        heads[0].get::<i64, _>("heads"),
        0,
        "head selection remains a later owner"
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn attached_display_profile_is_not_retained_as_verified_evidence() {
    let public = focused("unverified-display");
    let metadata = ProfileMetadata::new(
        Some("Unverified caller display".into()),
        None,
        None,
        Some("Not signed profile evidence.".into()),
        None,
    )
    .unwrap();
    let profile = Kind0ProfileCandidate::new(
        EventId::from_bytes([74; 32]),
        public.publisher().public_key(),
        UnixTimestamp::from_seconds(1).unwrap(),
        metadata,
    );
    let decorated = public.with_profile(Some(&profile)).unwrap();
    assert!(decorated.profile_metadata().is_some());
    let fixture = Fixture::new();
    let database = fixture.open().await;
    database
        .retain_availability_version(decorated)
        .await
        .unwrap();
    assert_round_trip(
        &database
            .load_availability_version(public.version())
            .await
            .unwrap()
            .unwrap(),
        &public,
    );
    let rows = rows(database.host(), "SELECT (SELECT count(*) FROM profile_cache) AS profiles, (SELECT count(*) FROM account_identities) AS accounts").await;
    assert_eq!(rows[0].get::<i64, _>("profiles"), 0);
    assert_eq!(rows[0].get::<i64, _>("accounts"), 0);
    assert_usage(&database, 1, expected_charge(&public, "focused", None)).await;
    database.close().await.unwrap();
}

#[tokio::test]
async fn corrupt_wire_or_signed_metadata_fails_closed_on_load() {
    let public = focused("corrupt-evidence");
    let mut malformed = public.original_json().to_owned();
    let signature_start =
        malformed.find("\"sig\":\"").expect("signature field") + "\"sig\":\"".len();
    let replacement = if &malformed[signature_start..signature_start + 1] == "0" {
        "1"
    } else {
        "0"
    };
    malformed.replace_range(signature_start..signature_start + 1, replacement);
    let changes: [(&str, Option<String>); 9] = [
        (
            "UPDATE availability_versions SET original_json = ?",
            Some(malformed),
        ),
        (
            "UPDATE availability_versions SET author = zeroblob(32)",
            None,
        ),
        (
            "UPDATE availability_versions SET signed_at = x'0000000000000001'",
            None,
        ),
        (
            "UPDATE availability_versions SET published_at = x'0000000000000001'",
            None,
        ),
        (
            "UPDATE availability_versions SET raw_d = 'other-coordinate'",
            None,
        ),
        (
            "UPDATE availability_versions SET source = 'invalid-source'",
            None,
        ),
        (
            "UPDATE availability_versions SET original_json = '{}'",
            None,
        ),
        (
            "UPDATE availability_versions SET admission_label = 'excluded_generic', published_at = NULL",
            None,
        ),
        (
            "UPDATE availability_versions SET event_id = zeroblob(32)",
            None,
        ),
    ];
    for (sql, text) in changes {
        let fixture = Fixture::new();
        let database = fixture.open().await;
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        database.close().await.unwrap();
        let mut connection = fixture.closed_connection().await;
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await
            .unwrap();
        let mut query = sqlx::query(sql);
        if let Some(text) = text {
            query = query.bind(text);
        }
        query.execute(&mut connection).await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut connection)
            .await
            .unwrap();
        let version = if sql.contains("SET event_id") {
            AvailabilityEventVersion::from_hex(&"00".repeat(32)).unwrap()
        } else {
            public.version()
        };
        let row = bounded_fault_row(&mut connection, version).await;
        assert_eq!(
            decode_availability_row(row).unwrap_err().code(),
            SafeErrorCode::StorageCorrupt,
            "actual product decoder must reject signed/metadata mismatch"
        );
        connection.close().await.unwrap();
        match Database::open(&fixture.context, 200, 201, &fixture.build).await {
            Ok(database) => {
                assert_eq!(
                    database
                        .load_availability_version(version)
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                if version == public.version() {
                    assert_eq!(
                        database
                            .retain_availability_version(public.clone())
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt,
                        "duplicate does not bless corrupt first evidence"
                    );
                }
                database.close().await.unwrap();
            }
            Err(error) => assert_eq!(
                error.code(),
                SafeErrorCode::StorageCorrupt,
                "shared preflight refusal is distinct from decoder execution above"
            ),
        }
    }
}

#[tokio::test]
async fn oversize_or_malformed_fields_are_bounded_before_hydration() {
    let public = focused("bounded-corruption");
    let fields = [
        (
            "original_json",
            262_144_usize,
            "UPDATE availability_versions SET original_json = ?",
        ),
        ("raw_d", 4096, "UPDATE availability_versions SET raw_d = ?"),
        (
            "source",
            2048,
            "UPDATE availability_versions SET source = ?",
        ),
        (
            "admission_label",
            64,
            "UPDATE availability_versions SET admission_label = ?",
        ),
        (
            "rejection_code",
            64,
            "UPDATE availability_versions SET rejection_code = ?",
        ),
        ("author", 32, "UPDATE availability_versions SET author = ?"),
        (
            "signed_at",
            8,
            "UPDATE availability_versions SET signed_at = ?",
        ),
        (
            "published_at",
            8,
            "UPDATE availability_versions SET published_at = ?",
        ),
    ];
    for (field, bound, sql) in fields {
        let fixture = Fixture::new();
        let database = fixture.open().await;
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        database.close().await.unwrap();
        let mut connection = fixture.closed_connection().await;
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await
            .unwrap();
        // The identifier is exclusively this fixed test-owned inventory.
        let query = sqlx::query(sql);
        if matches!(field, "author" | "signed_at" | "published_at") {
            query
                .bind(vec![1_u8; bound + 2])
                .execute(&mut connection)
                .await
                .unwrap();
        } else {
            query
                .bind("x".repeat(bound + 2))
                .execute(&mut connection)
                .await
                .unwrap();
        }
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut connection)
            .await
            .unwrap();
        let row = bounded_fault_row(&mut connection, public.version()).await;
        let bytes: Vec<u8> = row.get(field);
        assert_eq!(
            bytes.len(),
            bound + 1,
            "real SQL projects only the bounded prefix"
        );
        assert_eq!(
            row.get::<i64, _>(format!("{field}_bytes").as_str()),
            i64::try_from(bound + 2).unwrap(),
            "full original byte length remains available without hydration"
        );
        assert_eq!(
            decode_availability_row(row).unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
        connection.close().await.unwrap();
        let error = match Database::open(&fixture.context, 300, 301, &fixture.build).await {
            Ok(database) => {
                let error = database
                    .load_availability_version(public.version())
                    .await
                    .unwrap_err();
                database.close().await.unwrap();
                error
            }
            Err(error) => error,
        };
        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
    }
    for sql in [
        "UPDATE availability_versions SET original_json = CAST(x'FF' AS TEXT)",
        "UPDATE availability_versions SET source = CAST(x'FF' AS TEXT)",
        "UPDATE availability_versions SET kind = 30403",
        "UPDATE availability_versions SET observed_at_unix_s = -1",
        "UPDATE availability_versions SET admission_label = 'unknown-private-message'",
        "UPDATE availability_versions SET rejection_code = 'caller-owned-message'",
    ] {
        let fixture = Fixture::new();
        let database = fixture.open().await;
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        database.close().await.unwrap();
        let mut connection = fixture.closed_connection().await;
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query(sql).execute(&mut connection).await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut connection)
            .await
            .unwrap();
        let error =
            decode_availability_row(bounded_fault_row(&mut connection, public.version()).await)
                .unwrap_err();
        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
        let diagnostic = format!("{error:?}");
        assert!(
            !diagnostic.contains("caller-owned-message")
                && !diagnostic.contains("unknown-private-message")
        );
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn logical_payload_charge_matches_the_frozen_formula() {
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let cases = [
        (Recipe::Focused, "focused", None),
        (Recipe::Generic, "excluded_generic", None),
        (Recipe::Operational, "excluded_operational", None),
        (
            Recipe::Ambiguous,
            "projection_rejected",
            Some("food_profile_ambiguous"),
        ),
        (
            Recipe::Rejected,
            "projection_rejected",
            Some("price_invalid"),
        ),
    ];
    let mut total = 0;
    for (index, (recipe, label, reason)) in cases.into_iter().enumerate() {
        let public = public_view(&format!("charge-{index}"), 100, 90, recipe, None);
        if let Some(reason) = reason {
            assert_eq!(
                public.unsupported_reason(),
                Some(&AvailabilityUnsupportedReason::ProjectionRejected(reason))
            );
        }
        let charge = expected_charge(&public, label, reason);
        total += charge;
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        let row = stored_row(&database, public.version()).await;
        assert_eq!(row.get::<String, _>("admission_label"), label);
        assert_eq!(
            row.get::<Option<String>, _>("rejection_code").as_deref(),
            reason
        );
        assert_eq!(row.get::<i64, _>("payload_bytes"), charge);
        assert_round_trip(
            &database
                .load_availability_version(public.version())
                .await
                .unwrap()
                .unwrap(),
            &public,
        );
        assert_usage(&database, index as i64 + 1, total).await;
    }
    assert_eq!(
        TOTAL_PAYLOAD_BYTES - ORDINARY_PAYLOAD_BYTES,
        RECOVERY_RESERVE_BYTES
    );
    database.close().await.unwrap();
    // Plausible in-range values remain corrupt when they disagree with retained facts.
    let public = focused("meter-corruption");
    for sql in [
        "UPDATE public_payload_usage SET version_count = 0",
        "UPDATE public_payload_usage SET payload_bytes = payload_bytes + 1",
        "DELETE FROM public_payload_usage",
        "UPDATE availability_versions SET payload_bytes = payload_bytes + 1",
        "UPDATE availability_versions SET payload_bytes = payload_bytes + 1; UPDATE public_payload_usage SET payload_bytes = payload_bytes + 1",
    ] {
        let fixture = Fixture::new();
        let database = fixture.open().await;
        database
            .retain_availability_version(public.clone())
            .await
            .unwrap();
        database.close().await.unwrap();
        let mut connection = fixture.closed_connection().await;
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::raw_sql(sql).execute(&mut connection).await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        match Database::open(&fixture.context, 400, 401, &fixture.build).await {
            Ok(database) => {
                assert_eq!(
                    database
                        .load_availability_version(public.version())
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                assert_eq!(
                    database
                        .retain_availability_version(public.clone())
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                database.close().await.unwrap();
            }
            Err(error) => assert_eq!(error.code(), SafeErrorCode::StorageCorrupt),
        }
        let mut connection = fixture.closed_connection().await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM availability_versions")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            1,
            "no reset, deletion or repair"
        );
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn exact_version_capacity_and_plus_one_preserve_existing_rows() {
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let (first, bytes) = fill_count(&database, 4096).await;
    assert_usage(&database, VERSION_CAPACITY, bytes).await;
    let excess = focused("capacity-plus-one");
    assert_eq!(
        database
            .retain_availability_version(excess.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::AvailabilityCapacity
    );
    assert_eq!(
        database
            .load_availability_version(excess.version())
            .await
            .unwrap(),
        None
    );
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    assert_usage(&database, 4096, bytes).await;
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_usage(&reopened, 4096, bytes).await;
    assert_round_trip(
        &reopened
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn ordinary_byte_capacity_preserves_the_recovery_reserve() {
    assert_eq!(DEFAULT_RAW_JSON_MAX_BYTES, 262_144);
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let mut bytes = 0;
    let mut count = 0;
    let mut first = None;
    while bytes < ORDINARY_PAYLOAD_BYTES {
        assert!(count < 600, "bounded actual-capacity fixture stream");
        let identifier = format!("byte-capacity-{count}");
        let overhead = 92 + identifier.len() + SOURCE.len() + "focused".len();
        let remaining = usize::try_from(ORDINARY_PAYLOAD_BYTES - bytes).unwrap();
        let wire_bytes = 262_144.min(remaining - overhead);
        assert!(
            wire_bytes >= 1024,
            "last admitted row remains a genuine bounded wire recipe"
        );
        let public = public_view(&identifier, 100, 90, Recipe::Focused, Some(wire_bytes));
        assert_eq!(public.original_json().len(), wire_bytes);
        if first.is_none() {
            first = Some(public.clone());
        }
        bytes += expected_charge(&public, "focused", None);
        assert!(bytes <= ORDINARY_PAYLOAD_BYTES);
        database.retain_availability_version(public).await.unwrap();
        count += 1;
    }
    assert_eq!(
        bytes, 125_829_120,
        "actual original-wire payloads fill the complete ordinary budget"
    );
    assert!(count < VERSION_CAPACITY);
    assert_usage(&database, count, bytes).await;
    let refused = focused("ordinary-byte-plus-one");
    assert_eq!(
        database
            .retain_availability_version(refused.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::AvailabilityCapacity
    );
    assert_eq!(
        database
            .load_availability_version(refused.version())
            .await
            .unwrap(),
        None
    );
    assert_usage(&database, count, bytes).await;
    assert_eq!(
        TOTAL_PAYLOAD_BYTES - bytes,
        8_388_608,
        "ordinary callers cannot consume recovery reserve"
    );
    let first = first.unwrap();
    let alternate = view_from_wire(
        first.original_json().trim(),
        "wss://another.example.invalid/public",
        OBSERVED_AT + 1,
    );
    database
        .retain_availability_version(alternate)
        .await
        .unwrap();
    assert_usage(&database, count, bytes).await;
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_usage(&reopened, count, bytes).await;
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn duplicates_at_capacity_do_not_grow_global_accounting() {
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let (first, bytes) = fill_count(&database, 4096).await;
    let alternate_wire = format!(
        "{},\"later_extra\":\"first evidence remains authoritative\"}}",
        first.original_json().strip_suffix('}').unwrap()
    );
    let alternate = view_from_wire(
        &alternate_wire,
        "wss://another.example.invalid/public",
        OBSERVED_AT + 20,
    );
    assert_eq!(alternate.version(), first.version());
    database
        .retain_availability_version(alternate)
        .await
        .expect("zero-growth duplicate at actual row cap");
    database
        .retain_availability_version(first.clone())
        .await
        .unwrap();
    assert_usage(&database, 4096, bytes).await;
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    database.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_and_failed_transactions_preserve_exact_global_accounting() {
    let fixture = Fixture::new();
    let database = fixture.open().await;
    let (first, before_bytes) = fill_count(&database, 4095).await;
    let left = focused("last-slot-left");
    let right = focused("last-slot-right");
    let left_charge = expected_charge(&left, "focused", None);
    let right_charge = expected_charge(&right, "focused", None);
    let injected = SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The test operation was refused."),
    );
    let rollback_view = left.clone();
    let error = database
        .host()
        .transaction(|transaction| {
            Box::pin(async move {
                retain_availability_version_on(transaction, rollback_view).await?;
                let meter: (i64, i64) =
                    sqlx::query_as("SELECT version_count, payload_bytes FROM public_payload_usage")
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(|_| injected)?;
                assert_eq!(
                    meter,
                    (4096, before_bytes + left_charge),
                    "real insertion and reservation precede failure"
                );
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM availability_versions")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(|_| injected)?;
                assert_eq!(count, 4096);
                Err::<(), _>(injected)
            })
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.kind(),
        ServiceSqliteTransactionErrorKind::OperationRolledBack
    );
    assert_eq!(error.operation_error(), Some(&injected));
    assert_usage(&database, 4095, before_bytes).await;
    assert_eq!(
        database
            .load_availability_version(left.version())
            .await
            .unwrap(),
        None
    );
    let duplicate_fixture = Fixture::new();
    let duplicate_database = duplicate_fixture.open().await;
    assert_usage(&duplicate_database, 0, 0).await;
    let duplicate_barrier = Arc::new(tokio::sync::Barrier::new(3));
    let (first_duplicate, second_duplicate, _) = tokio::join!(
        async {
            duplicate_barrier.wait().await;
            duplicate_database
                .retain_availability_version(first.clone())
                .await
        },
        async {
            duplicate_barrier.wait().await;
            duplicate_database
                .retain_availability_version(first.clone())
                .await
        },
        duplicate_barrier.wait(),
    );
    assert!(
        first_duplicate.is_ok(),
        "first concurrent new exact-ID retention succeeds"
    );
    assert!(
        second_duplicate.is_ok(),
        "second concurrent exact-ID retention succeeds without growth"
    );
    assert_usage(
        &duplicate_database,
        1,
        expected_charge(&first, "focused", None),
    )
    .await;
    assert_round_trip(
        &duplicate_database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    duplicate_database.close().await.unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let (left_result, right_result, _) = tokio::join!(
        async {
            barrier.wait().await;
            database.retain_availability_version(left.clone()).await
        },
        async {
            barrier.wait().await;
            database.retain_availability_version(right.clone()).await
        },
        barrier.wait(),
    );
    assert_ne!(
        left_result.is_ok(),
        right_result.is_ok(),
        "one real last-slot winner"
    );
    let (winner, loser, added) = match (left_result, right_result) {
        (Ok(()), Err(error)) => {
            assert_eq!(error.code(), SafeErrorCode::AvailabilityCapacity);
            (&left, &right, left_charge)
        }
        (Err(error), Ok(())) => {
            assert_eq!(error.code(), SafeErrorCode::AvailabilityCapacity);
            (&right, &left, right_charge)
        }
        _ => panic!("exactly one last-slot result"),
    };
    assert_usage(&database, 4096, before_bytes + added).await;
    assert_round_trip(
        &database
            .load_availability_version(winner.version())
            .await
            .unwrap()
            .unwrap(),
        winner,
    );
    assert_eq!(
        database
            .load_availability_version(loser.version())
            .await
            .unwrap(),
        None
    );
    assert_round_trip(
        &database
            .load_availability_version(first.version())
            .await
            .unwrap()
            .unwrap(),
        &first,
    );
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_usage(&reopened, 4096, before_bytes + added).await;
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn reopen_and_account_removal_preserve_public_evidence() {
    let public = focused("independent-of-local-account");
    let key = public.publisher().public_key();
    let identity = NostrIdentity::new(
        NostrIdentityReference::derive(key).unwrap(),
        LocalKeyringBinding::new(key, SignerAvailability::CredentialMissing),
        None,
        IdentityCreatedAt::new(UnixTimestamp::from_seconds(1).unwrap()),
        None,
    )
    .unwrap();
    let fixture = Fixture::new();
    let database = fixture.open().await;
    database.insert_identity(&identity).await.unwrap();
    database
        .retain_availability_version(public.clone())
        .await
        .unwrap();
    let source = database.metadata().source_generation();
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(reopened.metadata().source_generation(), source);
    reopened.remove_identity(key).await.unwrap();
    assert!(reopened.list_identities().await.unwrap().is_empty());
    assert_round_trip(
        &reopened
            .load_availability_version(public.version())
            .await
            .unwrap()
            .unwrap(),
        &public,
    );
    assert_usage(&reopened, 1, expected_charge(&public, "focused", None)).await;
    reopened.close().await.unwrap();
    let again = fixture.open().await;
    assert_round_trip(
        &again
            .load_availability_version(public.version())
            .await
            .unwrap()
            .unwrap(),
        &public,
    );
    assert_usage(&again, 1, expected_charge(&public, "focused", None)).await;
    again.close().await.unwrap();
}

#[tokio::test]
async fn governed_backup_restore_preserves_evidence_and_accounting() {
    let retained = focused("captured-public-version");
    let later = public_view("post-backup-public-version", 100, 90, Recipe::Generic, None);
    let fixture = Fixture::new();
    let mut database = fixture.open().await;
    database
        .retain_availability_version(retained.clone())
        .await
        .unwrap();
    let expected_identity = database.metadata().identity();
    let parent = fixture.directory.path().join("backup-output");
    fs::create_dir(&parent).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let bundle = parent.join("public-evidence");
    let manifest = database
        .capture_online_backup(
            &bundle,
            BackupCreatedAtUnixMs::new(1_800_000_000_000).unwrap(),
        )
        .await
        .unwrap();
    assert!(!manifest.protected_material_included());
    let verified = verify_harvestcircle_backup(
        manifest.canonical_bytes(),
        manifest.digest(),
        &bundle,
        &expected_identity,
        NonZeroU64::new(manifest.members()[0].byte_length()).unwrap(),
    )
    .unwrap();
    database
        .retain_availability_version(later.clone())
        .await
        .unwrap();
    assert_usage(
        &database,
        2,
        expected_charge(&retained, "focused", None)
            + expected_charge(&later, "excluded_generic", None),
    )
    .await;
    database
        .restore_verified_backup(&fixture.context, verified, 200, &fixture.build)
        .await
        .unwrap();
    assert_eq!(database.metadata().identity(), expected_identity);
    assert_round_trip(
        &database
            .load_availability_version(retained.version())
            .await
            .unwrap()
            .unwrap(),
        &retained,
    );
    assert_eq!(
        database
            .load_availability_version(later.version())
            .await
            .unwrap(),
        None
    );
    assert_usage(&database, 1, expected_charge(&retained, "focused", None)).await;
    database.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_round_trip(
        &reopened
            .load_availability_version(retained.version())
            .await
            .unwrap()
            .unwrap(),
        &retained,
    );
    assert_usage(&reopened, 1, expected_charge(&retained, "focused", None)).await;
    reopened.close().await.unwrap();
}
