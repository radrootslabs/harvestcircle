use radroots_studio_application::{AccountRepository, AppStateRepository};
use radroots_studio_domain::{
    AccountCreatedAt, AccountLabel, AccountSummary, KeyAvailability, Npub, PublicKey, SafeError,
    SafeErrorCode, SafeMessage, SignerKind, UnixTimestamp,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::Database;

impl AccountRepository for Database {
    fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT pubkey, npub, signer_kind, key_availability, label, created_at, \
                 last_used_at FROM accounts ORDER BY created_at ASC, pubkey ASC",
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
                "SELECT pubkey, npub, signer_kind, key_availability, label, created_at, \
                 last_used_at FROM accounts WHERE pubkey = ?1",
                [public_key.to_hex()],
                decode_account,
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn insert_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let encoded = EncodedAccount::from(account);
        let result = self.connection().execute(
            "INSERT INTO accounts (pubkey, npub, signer_kind, key_availability, label, \
             created_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                encoded.public_key,
                encoded.npub,
                encoded.signer_kind,
                encoded.key_availability,
                encoded.label,
                encoded.created_at,
                encoded.last_used_at,
            ],
        );
        match result {
            Ok(1) => Ok(()),
            Err(error) if is_constraint_violation(&error) => Err(account_exists()),
            Ok(_) | Err(_) => Err(storage_error()),
        }
    }

    fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let encoded = EncodedAccount::from(account);
        match self.connection().execute(
            "UPDATE accounts SET npub = ?2, signer_kind = ?3, key_availability = ?4, \
             label = ?5, created_at = ?6, last_used_at = ?7 WHERE pubkey = ?1",
            params![
                encoded.public_key,
                encoded.npub,
                encoded.signer_kind,
                encoded.key_availability,
                encoded.label,
                encoded.created_at,
                encoded.last_used_at,
            ],
        ) {
            Ok(1) => Ok(()),
            Ok(0) => Err(account_not_found()),
            Ok(_) | Err(_) => Err(storage_error()),
        }
    }

    fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
        self.connection()
            .execute(
                "DELETE FROM accounts WHERE pubkey = ?1",
                [public_key.to_hex()],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }
}

impl AppStateRepository for Database {
    fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
        let value = self
            .connection()
            .query_row(
                "SELECT selected_pubkey FROM app_state WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|_| corrupt_storage_error())?;
        value
            .map(|hex| PublicKey::from_hex(&hex).map_err(|_| corrupt_storage_error()))
            .transpose()
    }

    fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
        if let Some(public_key) = public_key
            && self.find_account(public_key)?.is_none()
        {
            return Err(account_not_found());
        }
        self.connection()
            .execute(
                "UPDATE app_state SET selected_pubkey = ?1 WHERE singleton = 1",
                [public_key.map(PublicKey::to_hex)],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
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
            signer_kind: encode_signer_kind(account.signer_kind()),
            key_availability: encode_key_availability(account.key_availability()),
            label: account.label().map(|label| label.as_str().to_owned()),
            created_at: account.created_at().timestamp().as_seconds(),
            last_used_at: account.last_used_at().map(UnixTimestamp::as_seconds),
        }
    }
}

fn decode_account(row: &Row<'_>) -> rusqlite::Result<AccountSummary> {
    let public_key =
        PublicKey::from_hex(row.get::<_, String>(0)?.as_str()).map_err(|_| invalid_column(0))?;
    let npub = Npub::from_encoded(row.get(1)?).map_err(|_| invalid_column(1))?;
    let signer_kind = decode_signer_kind(row.get::<_, String>(2)?.as_str())?;
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

    Ok(AccountSummary::new(
        public_key,
        npub,
        signer_kind,
        key_availability,
        label,
        AccountCreatedAt::new(created_at),
        last_used_at,
    ))
}

const fn encode_signer_kind(value: SignerKind) -> &'static str {
    match value {
        SignerKind::LocalSecret => "local_secret",
        SignerKind::WatchOnly => "watch_only",
        SignerKind::RemoteNip46 => "remote_nip46",
    }
}

fn decode_signer_kind(value: &str) -> rusqlite::Result<SignerKind> {
    match value {
        "local_secret" => Ok(SignerKind::LocalSecret),
        "watch_only" => Ok(SignerKind::WatchOnly),
        "remote_nip46" => Ok(SignerKind::RemoteNip46),
        _ => Err(invalid_column(2)),
    }
}

const fn encode_key_availability(value: KeyAvailability) -> &'static str {
    match value {
        KeyAvailability::Available => "available",
        KeyAvailability::CredentialMissing => "credential_missing",
        KeyAvailability::StoreUnavailable => "store_unavailable",
        KeyAvailability::NotRequired => "not_required",
    }
}

fn decode_key_availability(value: &str) -> rusqlite::Result<KeyAvailability> {
    match value {
        "available" => Ok(KeyAvailability::Available),
        "credential_missing" => Ok(KeyAvailability::CredentialMissing),
        "store_unavailable" => Ok(KeyAvailability::StoreUnavailable),
        "not_required" => Ok(KeyAvailability::NotRequired),
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
        AccountCreatedAt, AccountLabel, AccountSummary, KeyAvailability, Npub, PublicKey,
        SafeErrorCode, SignerKind, UnixTimestamp,
    };
    use tempfile::tempdir;

    use crate::Database;

    const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn account(key_byte: u8, created_at: i64) -> AccountSummary {
        AccountSummary::new(
            PublicKey::from_bytes([key_byte; 32]),
            Npub::from_encoded(NPUB.to_owned()).expect("valid npub"),
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            Some(AccountLabel::parse("Farm account").expect("valid label")),
            AccountCreatedAt::new(
                UnixTimestamp::from_seconds(created_at).expect("valid timestamp"),
            ),
            None,
        )
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
