use radroots_studio_application::{AccountNamespaceRepository, AccountPreferenceKey};
use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage};
use rusqlite::{OptionalExtension, params};

use crate::Database;

const MAX_VALUE_CHARS: usize = 4_096;

impl AccountNamespaceRepository for Database {
    fn get_value(
        &self,
        owner: PublicKey,
        key: AccountPreferenceKey,
    ) -> Result<Option<String>, SafeError> {
        self.connection()
            .query_row(
                "SELECT preference_value FROM account_namespace \
                 WHERE owner_pubkey = ?1 AND preference_key = ?2",
                params![owner.to_hex(), encode_key(key)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn set_value(
        &self,
        owner: PublicKey,
        key: AccountPreferenceKey,
        value: &str,
    ) -> Result<(), SafeError> {
        if value.chars().count() > MAX_VALUE_CHARS || value.chars().any(char::is_control) {
            return Err(invalid_preference());
        }
        self.connection()
            .execute(
                "INSERT INTO account_namespace (owner_pubkey, preference_key, preference_value) \
                 VALUES (?1, ?2, ?3) ON CONFLICT(owner_pubkey, preference_key) DO UPDATE SET \
                 preference_value = excluded.preference_value",
                params![owner.to_hex(), encode_key(key), value],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    fn clear_owner(&self, owner: PublicKey) -> Result<(), SafeError> {
        self.connection()
            .execute(
                "DELETE FROM account_namespace WHERE owner_pubkey = ?1",
                [owner.to_hex()],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }
}

const fn encode_key(key: AccountPreferenceKey) -> &'static str {
    match key {
        AccountPreferenceKey::NamespaceProbe => "namespace_probe",
    }
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The account preference is unavailable."),
    )
}

const fn invalid_preference() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidAccountMetadata,
        SafeMessage::new("The account preference is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_application::{
        AccountNamespaceRepository, AccountPreferenceKey, AccountRepository, AppStateRepository,
    };
    use radroots_studio_domain::{
        AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, LocalSignerBinding,
        PublicKey, UnixTimestamp,
    };

    use crate::Database;

    fn account(byte: u8) -> AccountSummary {
        let public_key = PublicKey::from_bytes([byte; 32]);
        AccountSummary::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(i64::from(byte)).expect("time")),
            None,
        )
        .expect("account")
    }

    #[test]
    fn namespace_partitions_same_typed_key_by_owner_and_selection() {
        let database = Database::in_memory().expect("database");
        let owner_a = PublicKey::from_bytes([1; 32]);
        let owner_b = PublicKey::from_bytes([2; 32]);
        database.insert_account(&account(1)).expect("account a");
        database.insert_account(&account(2)).expect("account b");
        database
            .set_value(owner_a, AccountPreferenceKey::NamespaceProbe, "A")
            .expect("set a");
        database
            .set_value(owner_b, AccountPreferenceKey::NamespaceProbe, "B")
            .expect("set b");

        database
            .save_selected_account(Some(owner_b))
            .expect("select b");
        let selected = database
            .load_selected_account()
            .expect("selection")
            .expect("selected owner");
        assert_eq!(
            database
                .get_value(selected, AccountPreferenceKey::NamespaceProbe)
                .expect("selected value"),
            Some("B".to_owned())
        );
        assert_eq!(
            database
                .get_value(owner_a, AccountPreferenceKey::NamespaceProbe)
                .expect("owner a value"),
            Some("A".to_owned())
        );
    }

    #[test]
    fn namespace_updates_and_cascades_with_owner_removal() {
        let database = Database::in_memory().expect("database");
        let owner = PublicKey::from_bytes([3; 32]);
        database.insert_account(&account(3)).expect("account");
        database
            .set_value(owner, AccountPreferenceKey::NamespaceProbe, "before")
            .expect("set");
        database
            .set_value(owner, AccountPreferenceKey::NamespaceProbe, "after")
            .expect("update");
        assert_eq!(
            database
                .get_value(owner, AccountPreferenceKey::NamespaceProbe)
                .expect("value"),
            Some("after".to_owned())
        );

        database.remove_account(owner).expect("remove");
        assert_eq!(
            database
                .get_value(owner, AccountPreferenceKey::NamespaceProbe)
                .expect("deleted value"),
            None
        );
    }
}
