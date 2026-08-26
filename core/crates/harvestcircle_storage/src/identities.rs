use harvestcircle_application::{AppStateRepository, BoxFuture, IdentityRepository};
use harvestcircle_domain::{
    IdentityCreatedAt, IdentityLabel, LocalKeyringBinding, NostrIdentity, NostrIdentityReference,
    PublicKey, SafeError, SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
};
use sqlx::Row;

use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::{Database, HARVESTCIRCLE_IDENTITY_CAPACITY};

const IDENTITY_PROJECTION: &str = "SELECT substr(identity.public_key, 1, 33) AS public_key, length(identity.public_key) AS public_key_bytes, \
     substr(CAST(identity.npub AS BLOB), 1, 64) AS npub, length(CAST(identity.npub AS BLOB)) AS npub_bytes, \
     substr(CAST(binding.binding_kind AS BLOB), 1, 17) AS binding_kind, \
     length(CAST(binding.binding_kind AS BLOB)) AS binding_kind_bytes, \
     substr(CAST(binding.availability AS BLOB), 1, 19) AS availability, \
     length(CAST(binding.availability AS BLOB)) AS availability_bytes, \
     CASE WHEN identity.label IS NULL THEN NULL ELSE substr(CAST(identity.label AS BLOB), 1, 81) END AS label, \
     CASE WHEN identity.label IS NULL THEN NULL ELSE length(CAST(identity.label AS BLOB)) END AS label_bytes, \
     identity.created_at_unix_s, identity.last_used_at_unix_s \
     FROM account_identities AS identity JOIN local_signer_bindings AS binding \
     ON binding.account_public_key = identity.public_key";

impl IdentityRepository for Database {
    fn list_identities(&self) -> BoxFuture<'_, Result<Vec<NostrIdentity>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let sql = format!(
                            "{IDENTITY_PROJECTION} ORDER BY identity.created_at_unix_s, identity.public_key LIMIT {}",
                            HARVESTCIRCLE_IDENTITY_CAPACITY + 1
                        );
                        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                            .fetch_all(&mut *transaction)
                            .await
                            .map_err(|_| corrupt_storage())?;
                        if rows.len() > HARVESTCIRCLE_IDENTITY_CAPACITY {
                            return Err(corrupt_storage());
                        }
                        rows.iter().map(decode_identity).collect()
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn find_identity(
        &self,
        public_key: PublicKey,
    ) -> BoxFuture<'_, Result<Option<NostrIdentity>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let sql =
                            format!("{IDENTITY_PROJECTION} WHERE identity.public_key = ? LIMIT 2");
                        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                            .bind(public_key.as_bytes().as_slice())
                            .fetch_all(&mut *transaction)
                            .await
                            .map_err(|_| corrupt_storage())?;
                        match rows.as_slice() {
                            [] => Ok(None),
                            [row] => decode_identity(row).map(Some),
                            _ => Err(corrupt_storage()),
                        }
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn insert_identity<'a>(
        &'a self,
        identity: &'a NostrIdentity,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let encoded = EncodedIdentity::try_from(identity)?;
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let existing: i64 = sqlx::query_scalar(
                            "SELECT EXISTS(SELECT 1 FROM account_identities WHERE public_key = ?)",
                        )
                        .bind(encoded.public_key.as_slice())
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        match existing {
                            0 => {}
                            1 => return Err(identity_exists()),
                            _ => return Err(corrupt_storage()),
                        }
                        let admitted: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM (SELECT 1 FROM account_identities LIMIT 257)",
                        )
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if usize::try_from(admitted)
                            .ok()
                            .is_none_or(|count| count >= HARVESTCIRCLE_IDENTITY_CAPACITY)
                        {
                            return Err(identity_capacity_exhausted());
                        }
                        let result = sqlx::query(
                            "INSERT INTO account_identities \
                             (public_key, npub, label, created_at_unix_s, last_used_at_unix_s) \
                             VALUES (?, ?, ?, ?, ?)",
                        )
                        .bind(encoded.public_key.as_slice())
                        .bind(&encoded.npub)
                        .bind(&encoded.label)
                        .bind(encoded.created_at)
                        .bind(encoded.last_used_at)
                        .execute(&mut *transaction)
                        .await;
                        match result {
                            Ok(result) if result.rows_affected() == 1 => {}
                            Err(error) if is_unique_violation(&error) => {
                                return Err(identity_exists());
                            }
                            Ok(_) | Err(_) => return Err(storage_unavailable()),
                        }
                        let result = sqlx::query(
                            "INSERT INTO local_signer_bindings \
                             (account_public_key, binding_public_key, binding_kind, availability) \
                             VALUES (?, ?, 'local_secret', ?)",
                        )
                        .bind(encoded.public_key.as_slice())
                        .bind(encoded.public_key.as_slice())
                        .bind(encoded.key_availability)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(storage_unavailable());
                        }
                        Ok(())
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn update_identity<'a>(
        &'a self,
        identity: &'a NostrIdentity,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let encoded = EncodedIdentity::try_from(identity)?;
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let result = sqlx::query(
                            "UPDATE account_identities SET npub = ?, label = ?, \
                             created_at_unix_s = ?, last_used_at_unix_s = ? WHERE public_key = ?",
                        )
                        .bind(&encoded.npub)
                        .bind(&encoded.label)
                        .bind(encoded.created_at)
                        .bind(encoded.last_used_at)
                        .bind(encoded.public_key.as_slice())
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() == 0 {
                            return Err(identity_not_found());
                        }
                        if result.rows_affected() != 1 {
                            return Err(corrupt_storage());
                        }
                        let result = sqlx::query(
                            "UPDATE local_signer_bindings SET availability = ? \
                             WHERE account_public_key = ? AND binding_public_key = ? \
                             AND binding_kind = 'local_secret'",
                        )
                        .bind(encoded.key_availability)
                        .bind(encoded.public_key.as_slice())
                        .bind(encoded.public_key.as_slice())
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(corrupt_storage());
                        }
                        Ok(())
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn remove_identity(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let result =
                            sqlx::query("DELETE FROM account_identities WHERE public_key = ?")
                                .bind(public_key.as_bytes().as_slice())
                                .execute(&mut *transaction)
                                .await
                                .map_err(|_| storage_unavailable())?;
                        match result.rows_affected() {
                            1 => Ok(()),
                            0 => Err(identity_not_found()),
                            _ => Err(corrupt_storage()),
                        }
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }
}

impl AppStateRepository for Database {
    fn load_selected_identity(&self) -> BoxFuture<'_, Result<Option<PublicKey>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let rows = sqlx::query(
                            "SELECT CASE WHEN selected_public_key IS NULL THEN NULL \
                             ELSE substr(selected_public_key, 1, 33) END AS selected_public_key, \
                             CASE WHEN selected_public_key IS NULL THEN NULL \
                             ELSE length(selected_public_key) END AS selected_bytes \
                             FROM runtime_state WHERE singleton = 1 LIMIT 2",
                        )
                        .fetch_all(&mut *transaction)
                        .await
                        .map_err(|_| corrupt_storage())?;
                        let [row] = rows.as_slice() else {
                            return Err(corrupt_storage());
                        };
                        let value = row
                            .try_get::<Option<Vec<u8>>, _>("selected_public_key")
                            .map_err(|_| corrupt_storage())?;
                        let length = row
                            .try_get::<Option<i64>, _>("selected_bytes")
                            .map_err(|_| corrupt_storage())?;
                        match (value, length) {
                            (None, None) => Ok(None),
                            (Some(value), Some(32)) => public_key_from_bytes(&value).map(Some),
                            _ => Err(corrupt_storage()),
                        }
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn save_selected_identity(
        &self,
        public_key: Option<PublicKey>,
    ) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        if let Some(public_key) = public_key {
                            let exists: i64 = sqlx::query_scalar(
                                "SELECT EXISTS(SELECT 1 FROM account_identities WHERE public_key = ?)",
                            )
                            .bind(public_key.as_bytes().as_slice())
                            .fetch_one(&mut *transaction)
                            .await
                            .map_err(|_| storage_unavailable())?;
                            if exists != 1 {
                                return Err(identity_not_found());
                            }
                        }
                        let selected = public_key.map(|key| key.as_bytes().to_vec());
                        let result = sqlx::query(
                            "UPDATE runtime_state SET selected_public_key = ? WHERE singleton = 1",
                        )
                        .bind(selected)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|_| storage_unavailable())?;
                        if result.rows_affected() != 1 {
                            return Err(corrupt_storage());
                        }
                        Ok(())
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }
}

struct EncodedIdentity {
    public_key: [u8; 32],
    npub: String,
    key_availability: &'static str,
    label: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl TryFrom<&NostrIdentity> for EncodedIdentity {
    type Error = SafeError;

    fn try_from(identity: &NostrIdentity) -> Result<Self, Self::Error> {
        let binding = identity
            .signer_binding()
            .as_local_keyring()
            .ok_or_else(storage_unavailable)?;
        Ok(Self {
            public_key: *identity.public_key().as_bytes(),
            npub: identity.npub().as_str().to_owned(),
            key_availability: encode_key_availability(binding.availability()),
            label: identity.label().map(|label| label.as_str().to_owned()),
            created_at: identity.created_at().timestamp().as_seconds(),
            last_used_at: identity.last_used_at().map(UnixTimestamp::as_seconds),
        })
    }
}

fn decode_identity(row: &sqlx::sqlite::SqliteRow) -> Result<NostrIdentity, SafeError> {
    let public_key_bytes = row
        .try_get::<Vec<u8>, _>("public_key")
        .map_err(|_| corrupt_storage())?;
    if row
        .try_get::<i64, _>("public_key_bytes")
        .map_err(|_| corrupt_storage())?
        != 32
    {
        return Err(corrupt_storage());
    }
    let public_key = public_key_from_bytes(&public_key_bytes)?;
    let npub = bounded_utf8(row, "npub", "npub_bytes", 63)?.ok_or_else(corrupt_storage)?;
    let binding_kind =
        bounded_utf8(row, "binding_kind", "binding_kind_bytes", 16)?.ok_or_else(corrupt_storage)?;
    if binding_kind != "local_secret" {
        return Err(corrupt_storage());
    }
    let availability =
        bounded_utf8(row, "availability", "availability_bytes", 18)?.ok_or_else(corrupt_storage)?;
    let availability = decode_key_availability(&availability)?;
    let label = bounded_utf8(row, "label", "label_bytes", 80)?
        .map(|value| IdentityLabel::parse(&value).map_err(|_| corrupt_storage()))
        .transpose()?;
    let created_at = UnixTimestamp::from_seconds(
        row.try_get("created_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    let last_used_at = row
        .try_get::<Option<i64>, _>("last_used_at_unix_s")
        .map_err(|_| corrupt_storage())?
        .map(|value| UnixTimestamp::from_seconds(value).ok_or_else(corrupt_storage))
        .transpose()?;
    NostrIdentity::new(
        NostrIdentityReference::verify(public_key, npub).map_err(|_| corrupt_storage())?,
        LocalKeyringBinding::new(public_key, availability),
        label,
        IdentityCreatedAt::new(created_at),
        last_used_at,
    )
    .map_err(|_| corrupt_storage())
}

fn bounded_utf8(
    row: &sqlx::sqlite::SqliteRow,
    value_column: &str,
    length_column: &str,
    maximum: usize,
) -> Result<Option<String>, SafeError> {
    let value = row
        .try_get::<Option<Vec<u8>>, _>(value_column)
        .map_err(|_| corrupt_storage())?;
    let length = row
        .try_get::<Option<i64>, _>(length_column)
        .map_err(|_| corrupt_storage())?;
    match (value, length) {
        (None, None) => Ok(None),
        (Some(value), Some(length))
            if usize::try_from(length)
                .ok()
                .is_some_and(|length| length <= maximum && length == value.len()) =>
        {
            String::from_utf8(value)
                .map(Some)
                .map_err(|_| corrupt_storage())
        }
        _ => Err(corrupt_storage()),
    }
}

fn public_key_from_bytes(bytes: &[u8]) -> Result<PublicKey, SafeError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| corrupt_storage())?;
    PublicKey::from_bytes(bytes).map_err(|_| corrupt_storage())
}

const fn encode_key_availability(value: SignerAvailability) -> &'static str {
    match value {
        SignerAvailability::Available => "available",
        SignerAvailability::CredentialMissing => "credential_missing",
        SignerAvailability::StoreUnavailable => "store_unavailable",
    }
}

fn decode_key_availability(value: &str) -> Result<SignerAvailability, SafeError> {
    match value {
        "available" => Ok(SignerAvailability::Available),
        "credential_missing" => Ok(SignerAvailability::CredentialMissing),
        "store_unavailable" => Ok(SignerAvailability::StoreUnavailable),
        _ => Err(corrupt_storage()),
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

const fn identity_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityAlreadyExists,
        SafeMessage::new("That identity is already saved."),
    )
}

const fn identity_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityNotFound,
        SafeMessage::new("That identity is not saved."),
    )
}

const fn identity_capacity_exhausted() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The saved identity capacity is exhausted."),
    )
}
