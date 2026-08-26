#![doc = "Integration-only native test bridge for HarvestCircle."]

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harvestcircle_application::{
    AppLifecycle, AppSnapshot, Clock, DurableRequestId, GeneratedKeyRecoveryHandle,
    InMemorySecretStore, RelayConfiguration, RemovalConfirmationToken, SecretStore, SessionState,
    SnapshotRevision,
};
use harvestcircle_domain::{
    PublicKey, RelayDestinationPolicy, RelayEndpoint, SafeError, SecretKeyInput, UnixTimestamp,
};
use harvestcircle_nostr::SdkNostrClient;
use harvestcircle_runtime::{
    InstallationIdentity, InstallationIdentitySource, RuntimeActorHandle,
    RuntimeChangeSubscription, RuntimeDependencies,
};
use nostr::{EventBuilder, Keys, Metadata};
use nostr_relay_builder::MockRelay;
use nostr_sdk::Client;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use tokio::runtime::{Builder, Runtime};

const ACTOR_CAPACITY: usize = 16;
const OBSERVER_CAPACITY: usize = 16;
const DEFAULT_TIMEOUT_MILLIS: u64 = 2_000;
const FIXED_TIME_SECONDS: i64 = 1_700_000_000;
const FIXED_INSTALLATION_ID: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct TestIdentity {
    pub public_key_hex: String,
    pub npub: String,
    pub display_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum TestLifecycle {
    Booting,
    Ready,
    Degraded,
    Fatal,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum TestSession {
    SignedOut,
    Activating,
    Active,
    SigningOut,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct TestSnapshot {
    pub revision: u64,
    pub lifecycle: TestLifecycle,
    pub identities: Vec<TestIdentity>,
    pub selected_public_key_hex: Option<String>,
    pub session: TestSession,
    pub profile_display_name: Option<String>,
}

#[derive(uniffi::Object)]
pub struct TestGeneratedRecoveryRequest {
    handle: GeneratedKeyRecoveryHandle,
    resolved: AtomicBool,
}

#[derive(uniffi::Object)]
pub struct TestRemovalRequest {
    token: Mutex<Option<RemovalConfirmationToken>>,
    public_key_hex: String,
    deletes_local_credential: bool,
    signs_out: bool,
    expires_at_seconds: i64,
}

#[uniffi::export]
impl TestRemovalRequest {
    pub fn public_key_hex(&self) -> String {
        self.public_key_hex.clone()
    }

    pub fn deletes_local_credential(&self) -> bool {
        self.deletes_local_credential
    }

    pub fn signs_out(&self) -> bool {
        self.signs_out
    }

    pub fn expires_at_seconds(&self) -> i64 {
        self.expires_at_seconds
    }
}

#[uniffi::export]
impl TestGeneratedRecoveryRequest {
    pub fn identity(&self) -> TestIdentity {
        to_identity(self.handle.view().identity())
    }

    pub fn expires_at_seconds(&self) -> i64 {
        self.handle.view().expires_at().as_seconds()
    }

    pub fn take_recovery_nsec(&self) -> Result<String, TestBridgeError> {
        self.handle
            .take_recovery_nsec()
            .map(|nsec| nsec.with_exposed_secret(ToOwned::to_owned))
            .map_err(TestBridgeError::from)
    }
}

#[derive(Debug, uniffi::Error)]
pub enum TestBridgeError {
    Failure { safe_message: String },
}

impl Display for TestBridgeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure { safe_message } => formatter.write_str(safe_message),
        }
    }
}

impl std::error::Error for TestBridgeError {}

impl From<SafeError> for TestBridgeError {
    fn from(error: SafeError) -> Self {
        Self::Failure {
            safe_message: error.message().as_str().to_owned(),
        }
    }
}

impl From<std::io::Error> for TestBridgeError {
    fn from(_error: std::io::Error) -> Self {
        Self::Failure {
            safe_message: "The integration test data directory is unavailable.".to_owned(),
        }
    }
}

#[derive(Default)]
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(FIXED_TIME_SECONDS).expect("fixed test timestamp")
    }
}

struct FixedInstallationIdentity;

impl InstallationIdentitySource for FixedInstallationIdentity {
    fn generate(&self) -> Result<InstallationIdentity, SafeError> {
        InstallationIdentity::parse(FIXED_INSTALLATION_ID)
    }
}

#[derive(uniffi::Object)]
pub struct HarvestCircleTestBridge {
    runtime: Runtime,
    actor: Mutex<Option<RuntimeActorHandle>>,
    observer: Mutex<Option<RuntimeChangeSubscription>>,
    secrets: Arc<InMemorySecretStore>,
    clock: Arc<FixedClock>,
    relay: Mutex<Option<MockRelay>>,
    relay_url: String,
    context: RuntimeContext,
    network_degraded: AtomicBool,
}

#[uniffi::export]
impl HarvestCircleTestBridge {
    #[uniffi::constructor]
    pub fn open(data_directory: String) -> Result<Arc<Self>, TestBridgeError> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|_| TestBridgeError::Failure {
                safe_message: "The integration test runtime could not start.".to_owned(),
            })?;
        let data_root = prepare_data_root(Path::new(&data_directory))?;
        let context = runtime_context(&data_root)?;
        let relay = runtime
            .block_on(MockRelay::run())
            .map_err(|_| TestBridgeError::Failure {
                safe_message: "The local integration relay could not start.".to_owned(),
            })?;
        let relay_url = runtime.block_on(relay.url()).to_string();
        let secrets = Arc::new(InMemorySecretStore::default());
        let clock = Arc::new(FixedClock);
        let actor = runtime.block_on(open_actor(
            &context,
            &relay_url,
            Arc::clone(&secrets),
            Arc::clone(&clock),
            runtime.handle(),
        ))?;
        Ok(Arc::new(Self {
            runtime,
            actor: Mutex::new(Some(actor)),
            observer: Mutex::new(None),
            secrets,
            clock,
            relay: Mutex::new(Some(relay)),
            relay_url,
            context,
            network_degraded: AtomicBool::new(false),
        }))
    }

    pub fn bootstrap(&self) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(self.to_test_snapshot(self.runtime.block_on(actor.bootstrap())?))
    }

    pub fn snapshot(&self) -> Result<TestSnapshot, TestBridgeError> {
        Ok(self.to_test_snapshot(self.actor()?.snapshot()))
    }

    pub fn begin_generated_identity(
        &self,
    ) -> Result<Arc<TestGeneratedRecoveryRequest>, TestBridgeError> {
        let actor = self.actor()?;
        let handle = self.runtime.block_on(actor.begin_generated_key_stage())?;
        Ok(Arc::new(TestGeneratedRecoveryRequest {
            handle,
            resolved: AtomicBool::new(false),
        }))
    }

    pub fn acknowledge_generated_identity(
        &self,
        request_id: String,
        expected_revision: u64,
        timeout_millis: u64,
        request: Arc<TestGeneratedRecoveryRequest>,
    ) -> Result<TestSnapshot, TestBridgeError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Err(request_unavailable());
        }
        let actor = self.actor()?;
        let snapshot = self
            .runtime
            .block_on(actor.acknowledge_generated_key_stage(
                request.handle.id(),
                DurableRequestId::parse(request_id)?,
                SnapshotRevision::from_value(expected_revision),
                Duration::from_millis(timeout_millis),
            ))?;
        Ok(self.to_test_snapshot(snapshot))
    }

    pub fn cancel_generated_identity(
        &self,
        request: Arc<TestGeneratedRecoveryRequest>,
    ) -> Result<bool, TestBridgeError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        let actor = self.actor()?;
        Ok(self.runtime.block_on(actor.cancel_generated_key_stage())?)
    }

    pub fn import_identity(
        &self,
        request_id: String,
        expected_revision: u64,
        mut secret: Vec<u8>,
        timeout_millis: u64,
    ) -> Result<TestSnapshot, TestBridgeError> {
        let input = SecretKeyInput::parse_bytes(std::mem::take(&mut secret))?;
        secret.fill(0);
        let actor = self.actor()?;
        let receipt = self.runtime.block_on(actor.import_secret_key(
            DurableRequestId::parse(request_id)?,
            SnapshotRevision::from_value(expected_revision),
            input,
            Duration::from_millis(timeout_millis),
        ))?;
        let _ = receipt.identity();
        Ok(self.to_test_snapshot(actor.snapshot()))
    }

    pub fn select_identity(&self, public_key_hex: String) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(self.to_test_snapshot(
            self.runtime
                .block_on(actor.select_identity(PublicKey::from_hex(&public_key_hex)?))?,
        ))
    }

    pub fn activate_identity(
        &self,
        public_key_hex: String,
    ) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(self.to_test_snapshot(
            self.runtime
                .block_on(actor.activate_identity(PublicKey::from_hex(&public_key_hex)?))?,
        ))
    }

    pub fn sign_out(&self) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(self.to_test_snapshot(self.runtime.block_on(actor.sign_out())?))
    }

    pub fn seed_selected_profile(&self, display_name: String) -> Result<(), TestBridgeError> {
        let selected = self
            .actor()?
            .snapshot()
            .selected_identity()
            .ok_or_else(request_unavailable)?;
        let secret = self.secrets.load(selected)?;
        self.runtime.block_on(async {
            let keys = secret
                .with_exposed_secret(Keys::parse)
                .map_err(|_| invalid_secret())?;
            let publisher = Client::new(keys);
            publisher
                .add_relay(&self.relay_url)
                .await
                .map_err(|_| relay_failed())?;
            publisher.connect().await;
            publisher
                .wait_for_connection(Duration::from_millis(DEFAULT_TIMEOUT_MILLIS))
                .await;
            publisher
                .send_event_builder(EventBuilder::metadata(
                    &Metadata::new().display_name(display_name),
                ))
                .await
                .map_err(|_| relay_failed())?;
            publisher.shutdown().await;
            Ok(())
        })
    }

    pub fn refresh_active_profile(&self) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(self.to_test_snapshot(self.runtime.block_on(actor.refresh_active_profile())?))
    }

    pub fn request_identity_removal(
        &self,
        public_key_hex: String,
    ) -> Result<Arc<TestRemovalRequest>, TestBridgeError> {
        let token = self.runtime.block_on(
            self.actor()?
                .request_identity_removal(PublicKey::from_hex(&public_key_hex)?),
        )?;
        let impact = token.impact();
        let expires_at_seconds = token.expires_at().as_seconds();
        Ok(Arc::new(TestRemovalRequest {
            token: Mutex::new(Some(token)),
            public_key_hex,
            deletes_local_credential: impact.deletes_local_credential(),
            signs_out: impact.signs_out(),
            expires_at_seconds,
        }))
    }

    pub fn cancel_identity_removal(&self, request: Arc<TestRemovalRequest>) -> bool {
        request
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .is_some()
    }

    pub fn confirm_identity_removal(
        &self,
        request_id: String,
        expected_revision: u64,
        timeout_millis: u64,
        request: Arc<TestRemovalRequest>,
    ) -> Result<TestSnapshot, TestBridgeError> {
        let token = request
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(request_unavailable)?;
        let snapshot = self
            .runtime
            .block_on(self.actor()?.confirm_identity_removal(
                token,
                DurableRequestId::parse(request_id)?,
                SnapshotRevision::from_value(expected_revision),
                Duration::from_millis(timeout_millis),
            ))?;
        Ok(self.to_test_snapshot(snapshot))
    }

    pub fn set_network_degraded(&self, degraded: bool) {
        self.network_degraded.store(degraded, Ordering::Release);
    }

    pub fn start_observer(&self) -> Result<(), TestBridgeError> {
        let actor = self.actor()?;
        let subscription =
            self.runtime.block_on(actor.subscribe_changes(
                NonZeroUsize::new(OBSERVER_CAPACITY).expect("observer capacity"),
            ))?;
        *self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(subscription);
        Ok(())
    }

    pub fn next_observed_snapshot(
        &self,
        timeout_millis: u64,
    ) -> Result<Option<TestSnapshot>, TestBridgeError> {
        let mut observer = self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let subscription = observer.as_mut().ok_or_else(request_unavailable)?;
        let change = self.runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_millis(timeout_millis),
                subscription.receive(),
            )
            .await
        });
        match change {
            Ok(Some(change)) => Ok(Some(self.to_test_snapshot(change.snapshot().clone()))),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    pub fn stop_observer(&self) -> Result<bool, TestBridgeError> {
        let subscription = self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(subscription) = subscription else {
            return Ok(false);
        };
        let actor = self.actor()?;
        Ok(self
            .runtime
            .block_on(actor.unsubscribe_changes(subscription.id()))?)
    }

    pub fn restart(&self) -> Result<TestSnapshot, TestBridgeError> {
        let _ = self.stop_observer();
        self.close_actor()?;
        let actor = self.runtime.block_on(open_actor(
            &self.context,
            &self.relay_url,
            Arc::clone(&self.secrets),
            Arc::clone(&self.clock),
            self.runtime.handle(),
        ))?;
        let snapshot = actor.snapshot();
        *self
            .actor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(actor);
        Ok(self.to_test_snapshot(snapshot))
    }

    pub fn shutdown(&self) -> Result<TestSnapshot, TestBridgeError> {
        let snapshot = self.snapshot()?;
        let _ = self.stop_observer();
        self.close_actor()?;
        Ok(TestSnapshot {
            lifecycle: TestLifecycle::Closed,
            ..snapshot
        })
    }
}

impl Drop for HarvestCircleTestBridge {
    fn drop(&mut self) {
        if let Some(relay) = self
            .relay
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            relay.shutdown();
        }
    }
}

impl HarvestCircleTestBridge {
    fn to_test_snapshot(&self, snapshot: AppSnapshot) -> TestSnapshot {
        let mut snapshot = to_snapshot(snapshot);
        if self.network_degraded.load(Ordering::Acquire)
            && !matches!(
                snapshot.lifecycle,
                TestLifecycle::Closed | TestLifecycle::Fatal
            )
        {
            snapshot.lifecycle = TestLifecycle::Degraded;
        }
        snapshot
    }

    fn actor(&self) -> Result<RuntimeActorHandle, TestBridgeError> {
        self.actor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(request_unavailable)
    }

    fn close_actor(&self) -> Result<(), TestBridgeError> {
        let actor = self
            .actor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(actor) = actor {
            self.runtime.block_on(
                actor.close_with_timeout(Duration::from_millis(DEFAULT_TIMEOUT_MILLIS)),
            )?;
        }
        Ok(())
    }
}

async fn open_actor(
    context: &RuntimeContext,
    relay_url: &str,
    secrets: Arc<InMemorySecretStore>,
    clock: Arc<FixedClock>,
    runtime: &tokio::runtime::Handle,
) -> Result<RuntimeActorHandle, TestBridgeError> {
    let relay = RelayEndpoint::parse(relay_url, RelayDestinationPolicy::Local, true, true)?;
    let dependencies = RuntimeDependencies::new(
        secrets,
        clock,
        Arc::new(SdkNostrClient::new(Duration::from_millis(
            DEFAULT_TIMEOUT_MILLIS,
        ))),
        Arc::new(FixedInstallationIdentity),
    );
    let build = migration_build_identity()?;
    Ok(RuntimeActorHandle::open(
        context,
        RelayConfiguration::new(vec![relay])?,
        dependencies,
        &build,
        NonZeroUsize::new(ACTOR_CAPACITY).expect("actor capacity"),
        runtime,
    )
    .await?)
}

fn prepare_data_root(path: &Path) -> Result<PathBuf, TestBridgeError> {
    fs::create_dir_all(path)?;
    Ok(path.canonicalize()?)
}

fn runtime_context(root: &Path) -> Result<RuntimeContext, TestBridgeError> {
    let resolver = RadrootsPathResolver::new(
        RadrootsPlatform::current(),
        RadrootsHostEnvironment::default(),
    );
    let bootstrap = RuntimeContextBootstrap::new(
        RadrootsPathProfile::RepoLocal,
        Some(root.to_path_buf()),
        RuntimeContextSource::BootstrapCli,
        RuntimeContextSource::SafeDefault,
    )
    .map_err(|_| invalid_runtime_evidence())?;
    RuntimeContext::resolve(
        &resolver,
        bootstrap,
        ServiceId::new("harvestcircle").map_err(|_| invalid_runtime_evidence())?,
        InstanceId::new("desktop").map_err(|_| invalid_runtime_evidence())?,
    )
    .map_err(|_| invalid_runtime_evidence())
}

fn migration_build_identity() -> Result<MigrationBuildIdentity, TestBridgeError> {
    MigrationBuildIdentity::new(
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
    .map_err(|_| invalid_runtime_evidence())
}

fn invalid_runtime_evidence() -> TestBridgeError {
    TestBridgeError::Failure {
        safe_message: "The integration test runtime evidence is invalid.".to_owned(),
    }
}

fn to_identity(identity: &harvestcircle_domain::NostrIdentity) -> TestIdentity {
    TestIdentity {
        public_key_hex: identity.public_key().to_hex(),
        npub: identity.npub().as_str().to_owned(),
        display_label: identity.display_label(),
    }
}

fn to_snapshot(snapshot: AppSnapshot) -> TestSnapshot {
    let lifecycle = match snapshot.lifecycle() {
        AppLifecycle::Booting => TestLifecycle::Booting,
        AppLifecycle::Ready => TestLifecycle::Ready,
        AppLifecycle::Fatal(_) => TestLifecycle::Fatal,
    };
    let session = match snapshot.session() {
        SessionState::SignedOut => TestSession::SignedOut,
        SessionState::Activating(_) => TestSession::Activating,
        SessionState::Active => TestSession::Active,
        SessionState::SigningOut => TestSession::SigningOut,
        SessionState::Failed(_) => TestSession::Failed,
    };
    let profile_display_name = snapshot
        .active_identity()
        .and_then(|active| active.profile())
        .and_then(|profile| profile.display_name())
        .map(ToOwned::to_owned);
    TestSnapshot {
        revision: snapshot.revision().value(),
        lifecycle,
        identities: snapshot.identities().iter().map(to_identity).collect(),
        selected_public_key_hex: snapshot.selected_identity().map(PublicKey::to_hex),
        session,
        profile_display_name,
    }
}

fn request_unavailable() -> TestBridgeError {
    TestBridgeError::Failure {
        safe_message: "The integration test request is no longer available.".to_owned(),
    }
}

fn invalid_secret() -> TestBridgeError {
    TestBridgeError::Failure {
        safe_message: "The integration test secret key is invalid.".to_owned(),
    }
}

fn relay_failed() -> TestBridgeError {
    TestBridgeError::Failure {
        safe_message: "The local integration relay operation failed.".to_owned(),
    }
}

uniffi::setup_scaffolding!();
