use harvestcircle_application::{AppStateRepository, IdentityRepository};
use harvestcircle_domain::{
    IdentityCreatedAt, IdentityLabel, LocalKeyringBinding, NostrIdentity, NostrIdentityReference,
    PublicKey, SafeError, SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::Database;

impl IdentityRepository for Database {
    fn list_identities(&self) -> Result<Vec<NostrIdentity>, SafeError> {
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
            .query_map([], decode_identity)
            .map_err(|_| storage_error())?;
        rows.map(|row| row.map_err(|_| corrupt_storage_error()))
            .collect()
    }

    fn find_identity(&self, public_key: PublicKey) -> Result<Option<NostrIdentity>, SafeError> {
        self.connection()
            .query_row(
                "SELECT identity.public_key, identity.npub, binding.binding_kind, \
                 binding.availability, identity.label, identity.created_at, identity.last_used_at \
                 FROM account_identities AS identity \
                 JOIN local_signer_bindings AS binding \
                 ON binding.account_public_key = identity.public_key \
                 WHERE identity.public_key = ?1",
                [public_key.to_hex()],
                decode_identity,
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn insert_identity(&self, identity: &NostrIdentity) -> Result<(), SafeError> {
        let encoded = EncodedIdentity::from(identity);
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
            Err(error) if is_constraint_violation(&error) => return Err(identity_exists()),
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

    fn update_identity(&self, identity: &NostrIdentity) -> Result<(), SafeError> {
        let encoded = EncodedIdentity::from(identity);
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
            return Err(identity_not_found());
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

    fn remove_identity(&self, public_key: PublicKey) -> Result<(), SafeError> {
        match self.connection().execute(
            "DELETE FROM account_identities WHERE public_key = ?1",
            [public_key.to_hex()],
        ) {
            Ok(1) => Ok(()),
            Ok(0) => Err(identity_not_found()),
            Ok(_) | Err(_) => Err(storage_error()),
        }
    }
}

impl AppStateRepository for Database {
    fn load_selected_identity(&self) -> Result<Option<PublicKey>, SafeError> {
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

    fn save_selected_identity(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
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
                return Err(identity_not_found());
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

struct EncodedIdentity {
    public_key: String,
    npub: String,
    signer_kind: &'static str,
    key_availability: &'static str,
    label: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl From<&NostrIdentity> for EncodedIdentity {
    fn from(identity: &NostrIdentity) -> Self {
        Self {
            public_key: identity.public_key().to_hex(),
            npub: identity.npub().as_str().to_owned(),
            signer_kind: "local_secret",
            key_availability: encode_key_availability(identity.signer_binding().availability()),
            label: identity.label().map(|label| label.as_str().to_owned()),
            created_at: identity.created_at().timestamp().as_seconds(),
            last_used_at: identity.last_used_at().map(UnixTimestamp::as_seconds),
        }
    }
}

fn decode_identity(row: &Row<'_>) -> rusqlite::Result<NostrIdentity> {
    let public_key =
        PublicKey::from_hex(row.get::<_, String>(0)?.as_str()).map_err(|_| invalid_column(0))?;
    let npub: String = row.get(1)?;
    if row.get::<_, String>(2)?.as_str() != "local_secret" {
        return Err(invalid_column(2));
    }
    let key_availability = decode_key_availability(row.get::<_, String>(3)?.as_str())?;
    let label = row
        .get::<_, Option<String>>(4)?
        .map(|value| IdentityLabel::parse(&value).map_err(|_| invalid_column(4)))
        .transpose()?;
    let created_at = UnixTimestamp::from_seconds(row.get(5)?).ok_or_else(|| invalid_column(5))?;
    let last_used_at = row
        .get::<_, Option<i64>>(6)?
        .map(|value| UnixTimestamp::from_seconds(value).ok_or_else(|| invalid_column(6)))
        .transpose()?;

    NostrIdentity::new(
        NostrIdentityReference::verify(public_key, npub).map_err(|_| invalid_column(1))?,
        LocalKeyringBinding::new(public_key, key_availability),
        label,
        IdentityCreatedAt::new(created_at),
        last_used_at,
    )
    .map_err(|_| invalid_column(0))
}

const fn encode_key_availability(value: SignerAvailability) -> &'static str {
    match value {
        SignerAvailability::Available => "available",
        SignerAvailability::CredentialMissing => "credential_missing",
        SignerAvailability::StoreUnavailable => "store_unavailable",
    }
}

fn decode_key_availability(value: &str) -> rusqlite::Result<SignerAvailability> {
    match value {
        "available" => Ok(SignerAvailability::Available),
        "credential_missing" => Ok(SignerAvailability::CredentialMissing),
        "store_unavailable" => Ok(SignerAvailability::StoreUnavailable),
        _ => Err(invalid_column(3)),
    }
}

fn invalid_column(index: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        index,
        "public identity metadata".to_owned(),
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

const fn identity_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityAlreadyExists,
        SafeMessage::new("The Nostr identity is already saved."),
    )
}

const fn identity_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityNotFound,
        SafeMessage::new("The identity was not found."),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use harvestcircle_application::{AppStateRepository, IdentityRepository};
    use harvestcircle_domain::{
        IdentityCreatedAt, IdentityLabel, LocalKeyringBinding, NostrIdentity,
        NostrIdentityReference, PublicKey, SafeErrorCode, SignerAvailability, UnixTimestamp,
    };
    use tempfile::{TempDir, tempdir_in};

    use crate::Database;

    fn tempdir() -> std::io::Result<TempDir> {
        tempdir_in(std::env::temp_dir().canonicalize()?)
    }

    fn public_key(key_byte: u8) -> PublicKey {
        let value = match key_byte {
            1 => "585591529da0bab31b3b1b1f986611cf5f435dca84f978c89ee8a40cca7103df",
            2 => "e0266e3cfb0d2886f91c73f5f868f3b98273713e5fcd97c081663f5518a4b3af",
            _ => "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
        };
        PublicKey::from_hex(value).expect("valid public key")
    }

    fn identity(key_byte: u8, created_at: i64) -> NostrIdentity {
        let public_key = public_key(key_byte);
        NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            Some(IdentityLabel::parse("Farm identity").expect("valid label")),
            IdentityCreatedAt::new(
                UnixTimestamp::from_seconds(created_at).expect("valid timestamp"),
            ),
            None,
        )
        .expect("identity")
    }

    #[test]
    fn identities_insert_list_update_and_reject_duplicates() {
        let database = Database::in_memory().expect("database");
        let first = identity(1, 20);
        let second = identity(2, 10);

        database.insert_identity(&first).expect("insert first");
        database.insert_identity(&second).expect("insert second");
        let duplicate = database.insert_identity(&first).expect_err("duplicate");

        assert_eq!(duplicate.code(), SafeErrorCode::IdentityAlreadyExists);
        assert_eq!(
            database.list_identities().expect("list"),
            vec![second, first.clone()]
        );
        assert_eq!(
            database.find_identity(first.public_key()).expect("find"),
            Some(first)
        );
    }

    #[test]
    fn identities_and_selection_survive_restart_without_secret_text() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("harvestcircle.sqlite3");
        let identity = identity(3, 30);

        {
            let database = Database::open(&path).expect("database");
            database.insert_identity(&identity).expect("insert");
            database
                .save_selected_identity(Some(identity.public_key()))
                .expect("select");
        }
        let reopened = Database::open(&path).expect("reopen");

        assert_eq!(
            reopened.list_identities().expect("list"),
            vec![identity.clone()]
        );
        assert_eq!(
            reopened.load_selected_identity().expect("selection"),
            Some(identity.public_key())
        );
        let bytes = fs::read(path).expect("database bytes");
        assert!(!String::from_utf8_lossy(&bytes).contains("nsec1known-test-secret"));
    }

    #[test]
    fn selection_requires_an_existing_identity_and_clears_on_delete() {
        let database = Database::in_memory().expect("database");
        let identity = identity(4, 40);

        let missing = database
            .save_selected_identity(Some(identity.public_key()))
            .expect_err("missing identity");
        assert_eq!(missing.code(), SafeErrorCode::IdentityNotFound);

        database.insert_identity(&identity).expect("insert");
        database
            .save_selected_identity(Some(identity.public_key()))
            .expect("select");
        database
            .remove_identity(identity.public_key())
            .expect("remove");

        assert_eq!(database.load_selected_identity().expect("selection"), None);
    }

    #[test]
    fn identity_mutations_reject_missing_and_corrupt_rows() {
        let database = Database::in_memory().expect("database");
        let missing = identity(3, 30);
        assert_eq!(
            database
                .update_identity(&missing)
                .expect_err("missing update")
                .code(),
            SafeErrorCode::IdentityNotFound
        );
        assert_eq!(
            database
                .remove_identity(missing.public_key())
                .expect_err("missing removal")
                .code(),
            SafeErrorCode::IdentityNotFound
        );
        assert_eq!(
            database.find_identity(missing.public_key()).expect("find"),
            None
        );

        database.insert_identity(&missing).expect("insert");
        database.update_identity(&missing).expect("update");
        database
            .connection()
            .execute(
                "DELETE FROM local_signer_bindings WHERE account_public_key = ?1",
                [missing.public_key().to_hex()],
            )
            .expect("delete binding");
        assert_eq!(
            database
                .update_identity(&missing)
                .expect_err("missing binding must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        database
            .connection()
            .execute(
                "INSERT INTO local_signer_bindings (account_public_key, binding_public_key, binding_kind, availability) VALUES (?1, ?1, 'local_secret', 'available')",
                [missing.public_key().to_hex()],
            )
            .expect("restore binding");
        database
            .connection()
            .pragma_update(None, "ignore_check_constraints", "ON")
            .expect("disable check constraints for corruption fixture");
        database
            .connection()
            .execute(
                "UPDATE local_signer_bindings SET binding_kind = 'remote' WHERE account_public_key = ?1",
                [missing.public_key().to_hex()],
            )
            .expect("corrupt binding kind");
        assert_eq!(
            database
                .list_identities()
                .expect_err("corrupt binding must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );

        let database = Database::in_memory().expect("database");
        database.insert_identity(&missing).expect("insert");
        database
            .connection()
            .pragma_update(None, "ignore_check_constraints", "ON")
            .expect("disable check constraints for corruption fixture");
        database
            .connection()
            .execute(
                "UPDATE local_signer_bindings SET availability = 'invalid' WHERE account_public_key = ?1",
                [missing.public_key().to_hex()],
            )
            .expect("corrupt availability");
        assert_eq!(
            database
                .find_identity(missing.public_key())
                .expect_err("corrupt availability must fail")
                .code(),
            SafeErrorCode::StorageUnavailable
        );

        let database = Database::in_memory().expect("database");
        database
            .connection()
            .execute("DELETE FROM runtime_state", [])
            .expect("delete runtime singleton");
        assert_eq!(
            database
                .save_selected_identity(None)
                .expect_err("missing runtime singleton must fail")
                .code(),
            SafeErrorCode::StorageCorrupt
        );

        let read_only = Database::in_memory().expect("read-only database");
        read_only
            .connection()
            .pragma_update(None, "query_only", "ON")
            .expect("enable query-only mode");
        assert_eq!(
            read_only
                .insert_identity(&missing)
                .expect_err("non-constraint insertion failure must fail closed")
                .code(),
            SafeErrorCode::StorageUnavailable
        );
    }
}
