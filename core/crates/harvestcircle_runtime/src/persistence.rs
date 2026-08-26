use std::sync::Arc;

use harvestcircle_application::{
    AppCore, AppSnapshot, Clock, DurableRequestId, GenerateIdentityReceipt, ImportIdentityReceipt,
    KeyMaterialProvider, RelayConfiguration, RemovalConfirmationToken, SecretStore,
    StagedGeneratedKey,
};
use harvestcircle_domain::{PublicKey, SafeError, SecretKeyInput};
use harvestcircle_nostr::NostrKeyMaterialProvider;
use harvestcircle_storage::Database;
use radroots_runtime_paths::RuntimeContext;
use radroots_service_sqlite::MigrationBuildIdentity;

use crate::{InstallationIdentity, InstallationIdentitySource};

pub struct PersistentAppCore {
    core: AppCore,
    database: Database,
    key_material: Arc<dyn KeyMaterialProvider>,
}

impl PersistentAppCore {
    pub(crate) async fn initialize_installation_identity(
        &self,
        source: &dyn InstallationIdentitySource,
    ) -> Result<InstallationIdentity, SafeError> {
        if let Some(existing) = self.database.load_installation_id().await? {
            return InstallationIdentity::parse(existing);
        }
        let candidate = source.generate()?;
        InstallationIdentity::parse(
            self.database
                .initialize_installation_id(candidate.as_str())
                .await?,
        )
    }

    /// Opens the canonical application database without accessing credentials or relays.
    pub async fn open(
        context: &RuntimeContext,
        relay_configuration: RelayConfiguration,
        created_at_unix_ms: u64,
        applied_at_unix_s: u64,
        build: &MigrationBuildIdentity,
    ) -> Result<Self, SafeError> {
        let key_material: Arc<dyn KeyMaterialProvider> = Arc::new(NostrKeyMaterialProvider);
        Ok(Self {
            core: AppCore::new(relay_configuration, Arc::clone(&key_material)),
            database: Database::open(context, created_at_unix_ms, applied_at_unix_s, build).await?,
            key_material,
        })
    }

    pub(crate) fn key_material(&self) -> &dyn KeyMaterialProvider {
        self.key_material.as_ref()
    }

    /// Reconciles the UUID ledger and restores public state without activating a session.
    pub async fn bootstrap(
        &self,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core
            .recover_durable_operations(
                &self.database,
                &self.database,
                secrets,
                &self.database,
                clock,
            )
            .await?;
        self.core
            .bootstrap_from(&self.database, &self.database)
            .await
    }

    pub async fn commit_staged_generated_key(
        &self,
        request_id: &DurableRequestId,
        staged: StagedGeneratedKey,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        self.core
            .commit_staged_generated_key(
                request_id,
                staged,
                &self.database,
                &self.database,
                secrets,
                &self.database,
                clock,
            )
            .await
    }

    pub async fn generate_identity_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateIdentityReceipt, SafeError> {
        self.core
            .generate_identity_durable(
                request_id,
                expected_revision,
                &self.database,
                &self.database,
                secrets,
                &self.database,
                clock,
            )
            .await
    }

    pub async fn import_secret_key_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        input: SecretKeyInput,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        self.core
            .import_secret_key_durable(
                request_id,
                expected_revision,
                input,
                &self.database,
                &self.database,
                secrets,
                &self.database,
                clock,
            )
            .await
    }

    pub async fn select_identity(&self, public_key: PublicKey) -> Result<AppSnapshot, SafeError> {
        self.core
            .select_identity(public_key, &self.database, &self.database)
            .await
    }

    pub async fn activate_identity(
        &self,
        public_key: PublicKey,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core
            .activate_identity(
                public_key,
                &self.database,
                &self.database,
                &self.database,
                secrets,
                clock,
            )
            .await
    }

    pub fn sign_out(&self) -> Result<AppSnapshot, SafeError> {
        self.core.sign_out()
    }

    pub fn request_identity_removal(
        &self,
        public_key: PublicKey,
        clock: &(impl Clock + ?Sized),
    ) -> Result<RemovalConfirmationToken, SafeError> {
        self.core.request_identity_removal(public_key, clock)
    }

    pub async fn confirm_identity_removal_durable(
        &self,
        request_id: &DurableRequestId,
        token: RemovalConfirmationToken,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core
            .confirm_identity_removal_durable(
                request_id,
                token,
                &self.database,
                &self.database,
                secrets,
                &self.database,
                clock,
            )
            .await
    }

    pub async fn close(&self) -> Result<(), SafeError> {
        self.database.close().await
    }

    #[must_use]
    pub const fn core(&self) -> &AppCore {
        &self.core
    }

    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }
}
