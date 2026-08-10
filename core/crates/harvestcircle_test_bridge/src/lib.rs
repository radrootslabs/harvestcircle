#![doc = "Integration-only native test bridge for HarvestCircle."]

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harvestcircle_application::{
    AppLifecycle, AppSnapshot, Clock, DurableRequestId, GeneratedKeyRecoveryHandle,
    InMemorySecretStore, RelayConfiguration, SessionState, SnapshotRevision,
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

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct TestSnapshot {
    pub revision: u64,
    pub lifecycle: String,
    pub identities: Vec<TestIdentity>,
    pub selected_public_key_hex: Option<String>,
    pub session: String,
    pub profile_display_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct TestGeneratedRecovery {
    pub stage_id: u64,
    pub identity: TestIdentity,
    pub recovery_nsec: String,
    pub expires_at_seconds: i64,
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
    pending_generation: Mutex<Option<GeneratedKeyRecoveryHandle>>,
    secrets: Arc<InMemorySecretStore>,
    clock: Arc<FixedClock>,
    relay: Mutex<Option<MockRelay>>,
    relay_url: String,
    database_path: PathBuf,
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
        let database_path = data_root.join("harvestcircle-integration.sqlite3");
        let relay = runtime
            .block_on(MockRelay::run())
            .map_err(|_| TestBridgeError::Failure {
                safe_message: "The local integration relay could not start.".to_owned(),
            })?;
        let relay_url = runtime.block_on(relay.url()).to_string();
        let secrets = Arc::new(InMemorySecretStore::default());
        let clock = Arc::new(FixedClock);
        let actor = runtime.block_on(open_actor(
            &database_path,
            &relay_url,
            Arc::clone(&secrets),
            Arc::clone(&clock),
            runtime.handle(),
        ))?;
        Ok(Arc::new(Self {
            runtime,
            actor: Mutex::new(Some(actor)),
            observer: Mutex::new(None),
            pending_generation: Mutex::new(None),
            secrets,
            clock,
            relay: Mutex::new(Some(relay)),
            relay_url,
            database_path,
        }))
    }

    pub fn bootstrap(&self) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(to_snapshot(self.runtime.block_on(actor.bootstrap())?))
    }

    pub fn snapshot(&self) -> Result<TestSnapshot, TestBridgeError> {
        Ok(to_snapshot(self.actor()?.snapshot()))
    }

    pub fn begin_generated_identity(&self) -> Result<TestGeneratedRecovery, TestBridgeError> {
        let actor = self.actor()?;
        let handle = self.runtime.block_on(actor.begin_generated_key_stage())?;
        let recovery_nsec = handle
            .take_recovery_nsec()?
            .with_exposed_secret(ToOwned::to_owned);
        let recovery = TestGeneratedRecovery {
            stage_id: handle.id().value(),
            identity: to_identity(handle.view().identity()),
            recovery_nsec,
            expires_at_seconds: handle.view().expires_at().as_seconds(),
        };
        *self
            .pending_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
        Ok(recovery)
    }

    pub fn acknowledge_generated_identity(
        &self,
        request_id: String,
        expected_revision: u64,
        timeout_millis: u64,
    ) -> Result<TestSnapshot, TestBridgeError> {
        let handle = self
            .pending_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(request_unavailable)?;
        let actor = self.actor()?;
        let snapshot = self
            .runtime
            .block_on(actor.acknowledge_generated_key_stage(
                handle.id(),
                DurableRequestId::parse(request_id)?,
                SnapshotRevision::from_value(expected_revision),
                Duration::from_millis(timeout_millis),
            ))?;
        Ok(to_snapshot(snapshot))
    }

    pub fn cancel_generated_identity(&self) -> Result<bool, TestBridgeError> {
        self.pending_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
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
        Ok(to_snapshot(actor.snapshot()))
    }

    pub fn select_identity(&self, public_key_hex: String) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(to_snapshot(self.runtime.block_on(
            actor.select_identity(PublicKey::from_hex(&public_key_hex)?),
        )?))
    }

    pub fn activate_identity(
        &self,
        public_key_hex: String,
    ) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(to_snapshot(self.runtime.block_on(
            actor.activate_identity(PublicKey::from_hex(&public_key_hex)?),
        )?))
    }

    pub fn sign_out(&self) -> Result<TestSnapshot, TestBridgeError> {
        let actor = self.actor()?;
        Ok(to_snapshot(self.runtime.block_on(actor.sign_out())?))
    }

    pub fn seed_profile(
        &self,
        secret_hex: String,
        display_name: String,
    ) -> Result<(), TestBridgeError> {
        self.runtime.block_on(async {
            let keys = Keys::parse(&secret_hex).map_err(|_| invalid_secret())?;
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
        Ok(to_snapshot(
            self.runtime.block_on(actor.refresh_active_profile())?,
        ))
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
            Ok(Some(change)) => Ok(Some(to_snapshot(change.snapshot().clone()))),
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
            &self.database_path,
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
        Ok(to_snapshot(snapshot))
    }

    pub fn shutdown(&self) -> Result<TestSnapshot, TestBridgeError> {
        let snapshot = self.snapshot()?;
        let _ = self.stop_observer();
        self.close_actor()?;
        if let Some(relay) = self
            .relay
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            relay.shutdown();
        }
        Ok(TestSnapshot {
            lifecycle: "closed".to_owned(),
            ..snapshot
        })
    }
}

impl HarvestCircleTestBridge {
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
    database_path: &Path,
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
    Ok(RuntimeActorHandle::open(
        database_path,
        RelayConfiguration::new(vec![relay])?,
        dependencies,
        NonZeroUsize::new(ACTOR_CAPACITY).expect("actor capacity"),
        runtime,
    )
    .await?)
}

fn prepare_data_root(path: &Path) -> Result<PathBuf, TestBridgeError> {
    fs::create_dir_all(path)?;
    Ok(path.canonicalize()?)
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
        AppLifecycle::Booting => "booting",
        AppLifecycle::Ready => "ready",
        AppLifecycle::Fatal(_) => "fatal",
    };
    let session = match snapshot.session() {
        SessionState::SignedOut => "signed_out",
        SessionState::Activating(_) => "activating",
        SessionState::Active => "active",
        SessionState::SigningOut => "signing_out",
        SessionState::Failed(_) => "failed",
    };
    let profile_display_name = snapshot
        .active_identity()
        .and_then(|active| active.profile())
        .and_then(|profile| profile.display_name())
        .map(ToOwned::to_owned);
    TestSnapshot {
        revision: snapshot.revision().value(),
        lifecycle: lifecycle.to_owned(),
        identities: snapshot.identities().iter().map(to_identity).collect(),
        selected_public_key_hex: snapshot.selected_identity().map(PublicKey::to_hex),
        session: session.to_owned(),
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
