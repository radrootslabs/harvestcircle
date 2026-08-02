use std::sync::{Mutex, MutexGuard};

use radroots_studio_domain::{
    AccountCreatedAt, AccountSummary, KeyAvailability, Nsec, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, SignerKind,
};
use radroots_studio_nostr::generate_local_keypair;

use crate::{AccountRepository, AppCore, AppStateRepository, Clock, SecretStore, StateTransition};

pub struct GenerateAccountReceipt {
    account: AccountSummary,
    generated_nsec: Nsec,
}

impl GenerateAccountReceipt {
    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }

    #[must_use]
    pub const fn generated_nsec(&self) -> &Nsec {
        &self.generated_nsec
    }
}

impl AppCore {
    /// Generates, stores, and selects one local Nostr account without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe key, credential, persistence, or application-state error.
    pub fn generate_account(
        &self,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateAccountReceipt, SafeError> {
        let generated = generate_local_keypair()?;
        let (public_key, npub, secret, nsec) = generated.into_parts();
        let account = AccountSummary::new(
            public_key,
            npub,
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            None,
            AccountCreatedAt::new(clock.now()),
            None,
        );
        secrets.put(public_key, secret)?;
        accounts.insert_account(&account)?;
        app_state.save_selected_account(Some(public_key))?;
        let registry = accounts.list_accounts()?;
        self.apply_transition(StateTransition::ReplaceRegistry {
            accounts: registry,
            selected: Some(public_key),
        })?;
        Ok(GenerateAccountReceipt {
            account,
            generated_nsec: nsec,
        })
    }
}

#[derive(Default)]
pub struct InMemoryAccountRepository {
    state: Mutex<InMemoryAccountState>,
}

#[derive(Default)]
struct InMemoryAccountState {
    accounts: Vec<AccountSummary>,
    selected: Option<PublicKey>,
}

impl InMemoryAccountRepository {
    fn state(&self) -> MutexGuard<'_, InMemoryAccountState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl AccountRepository for InMemoryAccountRepository {
    fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
        Ok(self.state().accounts.clone())
    }

    fn find_account(&self, public_key: PublicKey) -> Result<Option<AccountSummary>, SafeError> {
        Ok(self
            .state()
            .accounts
            .iter()
            .find(|account| account.public_key() == public_key)
            .cloned())
    }

    fn insert_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let mut state = self.state();
        if state
            .accounts
            .iter()
            .any(|saved| saved.public_key() == account.public_key())
        {
            return Err(account_exists());
        }
        state.accounts.push(account.clone());
        state
            .accounts
            .sort_by_key(|saved| (saved.created_at().timestamp(), saved.public_key()));
        Ok(())
    }

    fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let mut state = self.state();
        let saved = state
            .accounts
            .iter_mut()
            .find(|saved| saved.public_key() == account.public_key())
            .ok_or_else(account_not_found)?;
        *saved = account.clone();
        Ok(())
    }

    fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
        let mut state = self.state();
        state
            .accounts
            .retain(|account| account.public_key() != public_key);
        if state.selected == Some(public_key) {
            state.selected = None;
        }
        Ok(())
    }
}

impl AppStateRepository for InMemoryAccountRepository {
    fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
        Ok(self.state().selected)
    }

    fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
        let mut state = self.state();
        if public_key.is_some_and(|key| {
            !state
                .accounts
                .iter()
                .any(|account| account.public_key() == key)
        }) {
            return Err(account_not_found());
        }
        state.selected = public_key;
        Ok(())
    }
}

const fn account_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountAlreadyExists,
        SafeMessage::new("The Nostr account is already saved."),
    )
}

const fn account_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountNotFound,
        SafeMessage::new("The account was not found."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_domain::UnixTimestamp;

    use super::InMemoryAccountRepository;
    use crate::{
        AppCore, AppStateRepository, Clock, InMemorySecretStore, RelayConfiguration, SecretStore,
        SessionState,
    };

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(10).expect("time")
        }
    }

    #[test]
    fn generate_account_stores_selects_and_returns_one_time_nsec_without_activation() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        core.bootstrap().expect("bootstrap");

        let receipt = core
            .generate_account(&accounts, &accounts, &secrets, &FixedClock)
            .expect("generate");
        let public_key = receipt.account().public_key();
        assert_eq!(public_key.to_hex().len(), 64);
        assert!(secrets.contains(public_key).expect("credential"));
        assert_eq!(
            accounts.load_selected_account().expect("selection"),
            Some(public_key)
        );
        assert_eq!(core.snapshot().selected_account(), Some(public_key));
        assert_eq!(core.snapshot().session(), SessionState::SignedOut);
        assert!(core.snapshot().active_account().is_none());
        assert_eq!(receipt.generated_nsec().with_exposed_secret(str::len), 63);
        assert!(!format!("{:?}", core.snapshot()).contains("nsec1"));
    }
}
