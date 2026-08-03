use radroots_studio_application::{AccountRepository, AppStateRepository};
use radroots_studio_domain::{
    AccountCreatedAt, AccountIdentity, AccountLabel, AccountSummary, BindingAvailability,
    LocalSignerBinding, PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::Database;

impl AccountRepository for Database {
    fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT identity.public_key, identity.npub, binding.binding_kind, \
                 binding.availability, identity.label, identity.created_at, identity.last_used_at \
                 FROM account_identities AS identity \
                 JOIN local_signer_bindings AS binding \
                 ON binding.account_public_key = identity.public_key \
                 ORDER BY identity.created_at ASC, identity.public_key ASC",
            )
            .map_err(|_| storage_error())?;
        let rows = statement
            .query_map([], decode_account)
            .map_err(|_| storage_error())?;
        rows.map(|row| row.map_err(|_| corrupt_storage_error()))
            .collect()
    }

    fn find_account(&self, public_key: PublicKey) -> Result<Option<AccountSummary>, SafeError> {
        self.connection()
            .query_row(
                "SELECT identity.public_key, identity.npub, binding.binding_kind, \
                 binding.availability, identity.label, identity.created_at, identity.last_used_at \
                 FROM account_identities AS identity \
                 JOIN local_signer_bindings AS binding \
                 ON binding.account_public_key = identity.public_key \
                 WHERE identity.public_key = ?1",
                [public_key.to_hex()],
                decode_account,
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn insert_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let encoded = EncodedAccount::from(account);
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        let result = transaction.execute(
            "INSERT INTO account_identities (public_key, npub, label, created_at, last_used_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                encoded.public_key,
                encoded.npub,
                encoded.label,
                encoded.created_at,
                encoded.last_used_at
            ],
        );
        match result {
            Ok(1) => {}
            Err(error) if is_constraint_violation(&error) => return Err(account_exists()),
            Ok(_) | Err(_) => return Err(storage_error()),
        }
        if transaction
            .execute(
                "INSERT INTO local_signer_bindings (account_public_key, binding_public_key, \
                 binding_kind, availability) VALUES (?1, ?1, ?2, ?3)",
                params![
                    encoded.public_key,
                    encoded.signer_kind,
                    encoded.key_availability
                ],
            )
            .map_err(|_| storage_error())?
            != 1
        {
            return Err(storage_error());
        }
        transaction.commit().map_err(|_| storage_error())
    }

    fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let encoded = EncodedAccount::from(account);
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        let identity_rows = transaction
            .execute(
                "UPDATE account_identities SET npub = ?2, label = ?5, created_at = ?6, \
             last_used_at = ?7 WHERE public_key = ?1",
                params![
                    encoded.public_key,
                    encoded.npub,
                    encoded.signer_kind,
                    encoded.key_availability,
                    encoded.label,
                    encoded.created_at,
                    encoded.last_used_at,
                ],
            )
            .map_err(|_| storage_error())?;
        if identity_rows == 0 {
            return Err(account_not_found());
        }
        if identity_rows != 1 {
            return Err(storage_error());
        }
        let binding_rows = transaction
            .execute(
                "UPDATE local_signer_bindings SET binding_kind = ?2, availability = ?3 \
                 WHERE account_public_key = ?1 AND binding_public_key = ?1",
                params![
                    encoded.public_key,
                    encoded.signer_kind,
                    encoded.key_availability
                ],
            )
            .map_err(|_| storage_error())?;
        if binding_rows != 1 {
            return Err(corrupt_storage_error());
        }
        transaction.commit().map_err(|_| storage_error())
    }

    fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
        match self.connection().execute(
            "DELETE FROM account_identities WHERE public_key = ?1",
            [public_key.to_hex()],
        ) {
            Ok(1) => Ok(()),
            Ok(0) => Err(account_not_found()),
            Ok(_) | Err(_) => Err(storage_error()),
        }
    }
}

impl AppStateRepository for Database {
    fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
        let value = self
            .connection()
            .query_row(
                "SELECT selected_public_key FROM runtime_state WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|_| corrupt_storage_error())?;
        value
            .map(|hex| PublicKey::from_hex(&hex).map_err(|_| corrupt_storage_error()))
            .transpose()
    }

    fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        if let Some(public_key) = public_key {
            let exists = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM account_identities WHERE public_key = ?1)",
                    [public_key.to_hex()],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|_| storage_error())?;
            if !exists {
                return Err(account_not_found());
            }
        }
        let rows = transaction
            .execute(
                "UPDATE runtime_state SET selected_public_key = ?1 WHERE singleton = 1",
                [public_key.map(PublicKey::to_hex)],
            )
            .map_err(|_| storage_error())?;
        if rows != 1 {
            return Err(corrupt_storage_error());
        }
        transaction.commit().map_err(|_| storage_error())
    }
}

struct EncodedAccount {
    public_key: String,
    npub: String,
    signer_kind: &'static str,
    key_availability: &'static str,
    label: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl From<&AccountSummary> for EncodedAccount {
    fn from(account: &AccountSummary) -> Self {
        Self {
            public_key: account.public_key().to_hex(),
            npub: account.npub().as_str().to_owned(),
            signer_kind: "local_secret",
            key_availability: encode_key_availability(account.signer().availability()),
            label: account.label().map(|label| label.as_str().to_owned()),
            created_at: account.created_at().timestamp().as_seconds(),
            last_used_at: account.last_used_at().map(UnixTimestamp::as_seconds),
        }
    }
}

fn decode_account(row: &Row<'_>) -> rusqlite::Result<AccountSummary> {
    let public_key =
        PublicKey::from_hex(row.get::<_, String>(0)?.as_str()).map_err(|_| invalid_column(0))?;
    let npub: String = row.get(1)?;
    if row.get::<_, String>(2)?.as_str() != "local_secret" {
        return Err(invalid_column(2));
    }
    let key_availability = decode_key_availability(row.get::<_, String>(3)?.as_str())?;
    let label = row
        .get::<_, Option<String>>(4)?
        .map(|value| AccountLabel::parse(&value).map_err(|_| invalid_column(4)))
        .transpose()?;
    let created_at = UnixTimestamp::from_seconds(row.get(5)?).ok_or_else(|| invalid_column(5))?;
    let last_used_at = row
        .get::<_, Option<i64>>(6)?
        .map(|value| UnixTimestamp::from_seconds(value).ok_or_else(|| invalid_column(6)))
        .transpose()?;

    AccountSummary::new(
        AccountIdentity::verify(public_key, npub).map_err(|_| invalid_column(1))?,
        LocalSignerBinding::new(public_key, key_availability),
        label,
        AccountCreatedAt::new(created_at),
        last_used_at,
    )
    .map_err(|_| invalid_column(0))
}

const fn encode_key_availability(value: BindingAvailability) -> &'static str {
    match value {
        BindingAvailability::Available => "available",
        BindingAvailability::CredentialMissing => "credential_missing",
        BindingAvailability::StoreUnavailable => "store_unavailable",
    }
}

fn decode_key_availability(value: &str) -> rusqlite::Result<BindingAvailability> {
    match value {
        "available" => Ok(BindingAvailability::Available),
        "credential_missing" => Ok(BindingAvailability::CredentialMissing),
        "store_unavailable" => Ok(BindingAvailability::StoreUnavailable),
        _ => Err(invalid_column(3)),
    }
}

fn invalid_column(index: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        index,
        "public account metadata".to_owned(),
        rusqlite::types::Type::Text,
    )
}

fn is_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    )
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The application database is unavailable."),
    )
}

const fn corrupt_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The application database could not be read."),
    )
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
    use std::fs;

    use radroots_studio_application::{AccountRepository, AppStateRepository};
    use radroots_studio_domain::{
        AccountCreatedAt, AccountIdentity, AccountLabel, AccountSummary, BindingAvailability,
        LocalSignerBinding, PublicKey, SafeErrorCode, UnixTimestamp,
    };
    use tempfile::tempdir;

    use crate::Database;

    fn account(key_byte: u8, created_at: i64) -> AccountSummary {
        let public_key = PublicKey::from_bytes([key_byte; 32]);
        AccountSummary::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            Some(AccountLabel::parse("Farm account").expect("valid label")),
            AccountCreatedAt::new(
                UnixTimestamp::from_seconds(created_at).expect("valid timestamp"),
            ),
            None,
        )
        .expect("account")
    }

    #[test]
    fn accounts_insert_list_update_and_reject_duplicates() {
        let database = Database::in_memory().expect("database");
        let first = account(1, 20);
        let second = account(2, 10);

        database.insert_account(&first).expect("insert first");
        database.insert_account(&second).expect("insert second");
        let duplicate = database.insert_account(&first).expect_err("duplicate");

        assert_eq!(duplicate.code(), SafeErrorCode::AccountAlreadyExists);
        assert_eq!(
            database.list_accounts().expect("list"),
            vec![second, first.clone()]
        );
        assert_eq!(
            database.find_account(first.public_key()).expect("find"),
            Some(first)
        );
    }

    #[test]
    fn accounts_and_selection_survive_restart_without_secret_text() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let account = account(3, 30);

        {
            let database = Database::open(&path).expect("database");
            database.insert_account(&account).expect("insert");
            database
                .save_selected_account(Some(account.public_key()))
                .expect("select");
        }
        let reopened = Database::open(&path).expect("reopen");

        assert_eq!(
            reopened.list_accounts().expect("list"),
            vec![account.clone()]
        );
        assert_eq!(
            reopened.load_selected_account().expect("selection"),
            Some(account.public_key())
        );
        let bytes = fs::read(path).expect("database bytes");
        assert!(!String::from_utf8_lossy(&bytes).contains("nsec1known-test-secret"));
    }

    #[test]
    fn selection_requires_an_existing_account_and_clears_on_delete() {
        let database = Database::in_memory().expect("database");
        let account = account(4, 40);

        let missing = database
            .save_selected_account(Some(account.public_key()))
            .expect_err("missing account");
        assert_eq!(missing.code(), SafeErrorCode::AccountNotFound);

        database.insert_account(&account).expect("insert");
        database
            .save_selected_account(Some(account.public_key()))
            .expect("select");
        database
            .remove_account(account.public_key())
            .expect("remove");

        assert_eq!(database.load_selected_account().expect("selection"), None);
    }
}
