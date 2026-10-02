use std::fs;
use std::path::PathBuf;

use harvestcircle_application::{
    AppSnapshot, AppStateRepository, Clock, DurableIdentityOperation, DurableOperationKind,
    DurableOperationPhase, DurableOperationRepository, DurableRequestId, FailureSecretStore,
    IdentityRepository, ImportIdentityReceipt, KeyMaterialProvider, OperationPriorState,
    RelayConfiguration, SecretStore, SecretStoreCall, SecretStoreOperation,
};
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
    SafeError, SafeErrorCode, SecretKeyInput, SignerAvailability, UnixTimestamp,
};
use harvestcircle_nostr::NostrKeyMaterialProvider;
use harvestcircle_runtime::PersistentAppCore;
use harvestcircle_storage::{
    HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS, HarvestCircleStorageContract,
};
use nostr::{Keys, ToBech32};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use tempfile::{TempDir, tempdir};

const NOW: i64 = 200;

struct TestClock(i64);

impl Clock for TestClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(self.0).expect("fixture timestamp")
    }
}

struct TestKey(Keys);

impl TestKey {
    fn generate() -> Self {
        Self(Keys::generate())
    }

    fn opposite(&self) -> Self {
        Self(Keys::new(nostr::SecretKey::from(
            (**self.0.secret_key()).negate(),
        )))
    }

    fn hex(&self) -> SecretKeyInput {
        SecretKeyInput::parse(self.0.secret_key().to_secret_hex()).expect("fixture hex")
    }

    fn nsec(&self) -> SecretKeyInput {
        SecretKeyInput::parse(
            self.0
                .secret_key()
                .to_bech32()
                .expect("fixture nsec encoding"),
        )
        .expect("fixture nsec")
    }

    fn invalid_nsec(&self) -> SecretKeyInput {
        let input = self.nsec();
        input.with_exposed_secret(|value| {
            let mut bytes = value.as_bytes().to_vec();
            let last = bytes.last_mut().expect("nonempty fixture nsec");
            *last = if *last == b'q' { b'p' } else { b'q' };
            SecretKeyInput::parse_bytes(bytes).expect("plausible nsec shape")
        })
    }

    fn public_key(&self) -> PublicKey {
        NostrKeyMaterialProvider
            .import(self.hex())
            .expect("real fixture key derivation")
            .into_parts()
            .0
    }
}

struct Fixture {
    _directory: TempDir,
    context: RuntimeContext,
    build: MigrationBuildIdentity,
    database_path: PathBuf,
    secrets: FailureSecretStore,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempdir().expect("isolated temporary root");
        let root = directory.path().canonicalize().expect("canonical root");
        let context = RuntimeContext::resolve(
            &RadrootsPathResolver::new(
                RadrootsPlatform::current(),
                RadrootsHostEnvironment::default(),
            ),
            RuntimeContextBootstrap::new(
                RadrootsPathProfile::RepoLocal,
                Some(root),
                RuntimeContextSource::BootstrapCli,
                RuntimeContextSource::SafeDefault,
            )
            .expect("bootstrap input"),
            ServiceId::new("harvestcircle").expect("service"),
            InstanceId::new("desktop").expect("instance"),
        )
        .expect("runtime context");
        fs::create_dir_all(directory.path().join("data")).expect("existing state root");
        let database_path = HarvestCircleStorageContract::from_runtime_context(&context)
            .expect("storage contract")
            .paths()
            .state_database()
            .to_path_buf();
        let build = MigrationBuildIdentity::new(
            "0.1.0-alpha",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "1.97.1",
            "test",
            "test",
            1,
            1,
            1,
            1,
            1,
        )
        .expect("fixture build identity");
        Self {
            _directory: directory,
            context,
            build,
            database_path,
            secrets: FailureSecretStore::default(),
        }
    }

    async fn open(&self) -> PersistentAppCore {
        let adapter = PersistentAppCore::open(
            &self.context,
            RelayConfiguration::default(),
            200_000,
            200,
            &self.build,
        )
        .await
        .expect("governed persistent core");
        adapter
            .bootstrap(&self.secrets, &TestClock(NOW))
            .await
            .expect("persistent bootstrap");
        adapter
    }

    async fn admit(&self, adapter: &PersistentAppCore, input: SecretKeyInput) -> Admitted {
        let request = DurableRequestId::new_v7();
        let revision = adapter.core().snapshot().revision().value();
        let receipt = adapter
            .import_secret_key_durable(&request, revision, input, &self.secrets, &TestClock(NOW))
            .await
            .expect("original admitted import");
        Admitted {
            request,
            revision,
            receipt,
        }
    }

    async fn observe(
        &self,
        adapter: &PersistentAppCore,
        request: &DurableRequestId,
    ) -> Observation {
        Observation {
            operation: adapter
                .database()
                .load_durable_operation(request)
                .await
                .expect("journal observation"),
            snapshot: adapter.core().snapshot(),
            identities: adapter
                .database()
                .list_identities()
                .await
                .expect("identity observation"),
            selected: adapter
                .database()
                .load_selected_identity()
                .await
                .expect("selection observation"),
            unfinished: adapter
                .database()
                .list_unfinished_durable_operations()
                .await
                .expect("unfinished observation"),
            mutations: self
                .secrets
                .calls()
                .into_iter()
                .filter(|call| {
                    matches!(
                        call.operation(),
                        SecretStoreOperation::Put | SecretStoreOperation::Delete
                    )
                })
                .collect(),
        }
    }

    async fn replay_and_close(
        &self,
        adapter: &PersistentAppCore,
        request: &DurableRequestId,
        revision: u64,
        input: SecretKeyInput,
    ) -> ReplayEvidence {
        let before = self.observe(adapter, request).await;
        let result = adapter
            .import_secret_key_durable(request, revision, input, &self.secrets, &TestClock(NOW))
            .await;
        let after = self.observe(adapter, request).await;
        adapter.close().await.expect("explicit governed host close");
        let database_bytes = fs::read(&self.database_path).expect("closed database bytes");
        ReplayEvidence {
            before,
            result,
            after,
            database_bytes,
        }
    }
}

struct Admitted {
    request: DurableRequestId,
    revision: u64,
    receipt: ImportIdentityReceipt,
}

#[derive(Debug, Eq, PartialEq)]
struct Observation {
    operation: Option<DurableIdentityOperation>,
    snapshot: AppSnapshot,
    identities: Vec<NostrIdentity>,
    selected: Option<PublicKey>,
    unfinished: Vec<DurableIdentityOperation>,
    mutations: Vec<SecretStoreCall>,
}

struct ReplayEvidence {
    before: Observation,
    result: Result<ImportIdentityReceipt, SafeError>,
    after: Observation,
    database_bytes: Vec<u8>,
}

impl ReplayEvidence {
    fn assert_unchanged(&self) {
        assert_eq!(self.before, self.after);
    }

    fn assert_original(&self, admitted: &Admitted, kind: DurableOperationKind) {
        let operation = self.before.operation.as_ref().expect("original operation");
        assert_eq!(operation.request_id(), &admitted.request);
        assert_eq!(operation.kind(), kind);
        assert_eq!(operation.expected_revision(), Some(admitted.revision));
        assert_eq!(
            operation.identity(),
            admitted.receipt.identity().public_key()
        );
        assert_eq!(operation.phase(), DurableOperationPhase::Finalized);
        assert!(operation.terminal().is_some());
    }

    fn assert_success(&self, admitted: &Admitted, kind: DurableOperationKind) {
        self.assert_unchanged();
        self.assert_original(admitted, kind);
        assert_eq!(
            self.result.as_ref().expect("exact replay"),
            &admitted.receipt
        );
    }

    fn assert_error(&self, code: SafeErrorCode) {
        self.assert_unchanged();
        assert_eq!(
            self.result.as_ref().expect_err("replay must fail").code(),
            code
        );
    }

    fn assert_secret_absent(&self, input: &SecretKeyInput) {
        let public = format!("{:?}{:?}{:?}", self.before, self.result, self.after);
        assert!(!input.with_exposed_secret(|value| public.contains(value)));
        assert!(!input.with_exposed_secret(|value| {
            self.database_bytes
                .windows(value.len())
                .any(|bytes| bytes == value.as_bytes())
        }));
        assert!(
            !self
                .database_bytes
                .windows(5)
                .any(|bytes| bytes == b"nsec1")
        );
    }

    fn assert_key_redacted(&self, key: &TestKey) {
        self.assert_secret_absent(&key.hex());
        self.assert_secret_absent(&key.nsec());
    }
}

#[tokio::test]
async fn completed_import_exact_replay_preserves_original_kind_receipt_and_custody() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    evidence.assert_success(&admitted, DurableOperationKind::Import);
    evidence.assert_key_redacted(&key);
}

#[tokio::test]
async fn completed_import_exact_replay_survives_unrelated_revision_advance() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let unrelated = fixture.admit(&adapter, other.hex()).await;
    adapter
        .select_identity(unrelated.receipt.identity().public_key())
        .await
        .expect("unrelated selection advance");
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    assert!(evidence.before.snapshot.revision().value() > admitted.revision);
    assert_eq!(
        evidence.before.selected,
        Some(unrelated.receipt.identity().public_key())
    );
    evidence.assert_success(&admitted, DurableOperationKind::Import);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}

#[tokio::test]
async fn completed_import_exact_replay_survives_database_reopen() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    fixture.admit(&adapter, other.hex()).await;
    let original_operation = adapter
        .database()
        .load_durable_operation(&admitted.request)
        .await
        .expect("original durable receipt");
    adapter.close().await.expect("close before actual reopen");
    let reopened = fixture.open().await;
    let evidence = fixture
        .replay_and_close(&reopened, &admitted.request, admitted.revision, key.hex())
        .await;
    assert_eq!(evidence.before.operation, original_operation);
    evidence.assert_success(&admitted, DurableOperationKind::Import);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}

#[tokio::test]
async fn completed_repair_exact_replay_preserves_original_kind_after_availability_restored() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let public_key = key.public_key();
    let missing = NostrIdentity::new(
        NostrIdentityReference::derive(public_key).expect("canonical identity"),
        LocalKeyringBinding::new(public_key, SignerAvailability::CredentialMissing),
        None,
        IdentityCreatedAt::new(TestClock(NOW).now()),
        None,
    )
    .expect("missing credential identity");
    adapter
        .database()
        .insert_identity(&missing)
        .await
        .expect("governed repair metadata");
    adapter
        .database()
        .save_selected_identity(Some(public_key))
        .await
        .expect("governed repair selection");
    adapter
        .close()
        .await
        .expect("close before repair bootstrap");
    let adapter = fixture.open().await;
    let admitted = fixture.admit(&adapter, key.hex()).await;
    fixture.admit(&adapter, other.hex()).await;
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    assert_eq!(
        admitted.receipt.identity().signer_binding().availability(),
        SignerAvailability::Available
    );
    evidence.assert_success(&admitted, DurableOperationKind::Repair);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}

#[tokio::test]
async fn completed_import_replay_accepts_equivalent_nsec_and_hex() {
    let mut observations = Vec::new();
    for first_nsec in [true, false] {
        let fixture = Fixture::new();
        let adapter = fixture.open().await;
        let key = TestKey::generate();
        let first = if first_nsec { key.nsec() } else { key.hex() };
        let replay = if first_nsec { key.hex() } else { key.nsec() };
        let admitted = fixture.admit(&adapter, first).await;
        let evidence = fixture
            .replay_and_close(&adapter, &admitted.request, admitted.revision, replay)
            .await;
        observations.push((admitted, evidence, key));
    }
    for (admitted, evidence, key) in observations {
        evidence.assert_success(&admitted, DurableOperationKind::Import);
        evidence.assert_key_redacted(&key);
    }
}

#[tokio::test]
async fn completed_import_replay_rejects_changed_identity() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let distinct = key.public_key() != other.public_key();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, other.hex())
        .await;
    assert!(distinct, "fixture identities must differ");
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}

#[tokio::test]
async fn completed_import_replay_rejects_changed_expected_revision() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let evidence = fixture
        .replay_and_close(
            &adapter,
            &admitted.request,
            admitted.revision + 1,
            key.hex(),
        )
        .await;
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
}

#[tokio::test]
async fn completed_import_replay_rejects_completed_create_request() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let request = DurableRequestId::new_v7();
    let revision = adapter.core().snapshot().revision().value();
    let generated = adapter
        .generate_identity_durable(&request, revision, &fixture.secrets, &TestClock(NOW))
        .await
        .expect("real durable creation");
    let input = generated
        .generated_nsec()
        .with_exposed_secret(|value| SecretKeyInput::parse(value.to_owned()))
        .expect("generated input");
    let canonical = NostrKeyMaterialProvider
        .import(input)
        .expect("canonical generated key")
        .into_parts()
        .2;
    let replay = canonical
        .with_exposed_secret(|value| SecretKeyInput::parse(value.to_owned()))
        .expect("replay input");
    let evidence = fixture
        .replay_and_close(&adapter, &request, revision, replay)
        .await;
    assert_eq!(
        evidence
            .before
            .operation
            .as_ref()
            .expect("create receipt")
            .kind(),
        DurableOperationKind::Create
    );
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_secret_absent(&canonical);
}

#[tokio::test]
async fn completed_import_replay_rejects_completed_remove_request() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let original = fixture.admit(&adapter, key.hex()).await;
    fixture.admit(&adapter, other.hex()).await;
    let request = DurableRequestId::new_v7();
    let revision = adapter.core().snapshot().revision().value();
    let token = adapter
        .request_identity_removal(original.receipt.identity().public_key(), &TestClock(NOW))
        .expect("explicit removal authority");
    adapter
        .confirm_identity_removal_durable(&request, token, &fixture.secrets, &TestClock(NOW))
        .await
        .expect("completed removal");
    fixture.admit(&adapter, key.hex()).await;
    let evidence = fixture
        .replay_and_close(&adapter, &request, revision, key.hex())
        .await;
    assert_eq!(
        evidence
            .before
            .operation
            .as_ref()
            .expect("remove receipt")
            .kind(),
        DurableOperationKind::Remove
    );
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}

#[tokio::test]
async fn completed_import_replay_rejects_opposite_scalar_with_same_x_only_identity() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let opposite = key.opposite();
    let same_identity = key.public_key() == opposite.public_key();
    let distinct_input = key
        .hex()
        .with_exposed_secret(|first| opposite.hex().with_exposed_secret(|second| first != second));
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let evidence = fixture
        .replay_and_close(
            &adapter,
            &admitted.request,
            admitted.revision,
            opposite.hex(),
        )
        .await;
    assert!(
        same_identity,
        "real opposite scalars must share x-only identity"
    );
    assert!(distinct_input, "the complete scalar inputs must differ");
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&opposite);
}

#[tokio::test]
async fn completed_import_replay_rejects_invalid_nsec_checksum() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let invalid = key.invalid_nsec();
    let evidence = fixture
        .replay_and_close(
            &adapter,
            &admitted.request,
            admitted.revision,
            key.invalid_nsec(),
        )
        .await;
    evidence.assert_error(SafeErrorCode::InvalidSecretKey);
    evidence.assert_key_redacted(&key);
    evidence.assert_secret_absent(&invalid);
}

#[tokio::test]
async fn completed_import_replay_rejects_missing_credential() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    fixture
        .secrets
        .delete(&admitted.request, admitted.receipt.identity().public_key())
        .await
        .expect("isolated missing credential");
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    evidence.assert_error(SafeErrorCode::CredentialMissing);
    evidence.assert_key_redacted(&key);
}

#[tokio::test]
async fn completed_import_replay_rejects_credential_replaced_under_another_request() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let public_key = admitted.receipt.identity().public_key();
    fixture
        .secrets
        .delete(&admitted.request, public_key)
        .await
        .expect("remove isolated original credential");
    let replacement = DurableRequestId::new_v7();
    fixture
        .secrets
        .put(&replacement, public_key, key.hex())
        .await
        .expect("replacement with another request");
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    assert_ne!(replacement, admitted.request);
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
}

#[tokio::test]
async fn completed_import_replay_rejects_replaced_secret_with_same_x_only_identity() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let opposite = key.opposite();
    let same_identity = key.public_key() == opposite.public_key();
    let distinct_input = key
        .hex()
        .with_exposed_secret(|first| opposite.hex().with_exposed_secret(|second| first != second));
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let public_key = admitted.receipt.identity().public_key();
    fixture
        .secrets
        .delete(&admitted.request, public_key)
        .await
        .expect("remove isolated original credential");
    fixture
        .secrets
        .put(&admitted.request, public_key, opposite.hex())
        .await
        .expect("replace full scalar under the original request");
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    assert!(
        same_identity,
        "real opposite scalars must share x-only identity"
    );
    assert!(distinct_input, "the complete scalar inputs must differ");
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&opposite);
}

#[tokio::test]
async fn durable_import_replay_requires_terminal_receipt() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let request = DurableRequestId::new_v7();
    let revision = adapter.core().snapshot().revision().value();
    adapter
        .database()
        .begin_durable_operation(
            &request,
            DurableOperationKind::Import,
            key.public_key(),
            Some(revision),
            OperationPriorState::new(None, None),
            TestClock(NOW).now(),
        )
        .await
        .expect("unfinished governed intent");
    let evidence = fixture
        .replay_and_close(&adapter, &request, revision, key.hex())
        .await;
    assert!(
        evidence
            .before
            .operation
            .as_ref()
            .expect("intent")
            .terminal()
            .is_none()
    );
    evidence.assert_error(SafeErrorCode::PendingOperationRecoveryRequired);
    evidence.assert_key_redacted(&key);
}

#[tokio::test]
async fn expired_import_receipt_is_not_recreated_as_success() {
    let fixture = Fixture::new();
    let adapter = fixture.open().await;
    let key = TestKey::generate();
    let other = TestKey::generate();
    let admitted = fixture.admit(&adapter, key.hex()).await;
    let cleanup_time = NOW + HARVESTCIRCLE_TERMINAL_RECEIPT_RETENTION_SECONDS + 1;
    adapter
        .import_secret_key_durable(
            &DurableRequestId::new_v7(),
            adapter.core().snapshot().revision().value(),
            other.hex(),
            &fixture.secrets,
            &TestClock(cleanup_time),
        )
        .await
        .expect("unrelated admission triggers existing expired cleanup");
    let evidence = fixture
        .replay_and_close(&adapter, &admitted.request, admitted.revision, key.hex())
        .await;
    assert!(evidence.before.operation.is_none());
    evidence.assert_error(SafeErrorCode::InvalidApplicationState);
    evidence.assert_key_redacted(&key);
    evidence.assert_key_redacted(&other);
}
