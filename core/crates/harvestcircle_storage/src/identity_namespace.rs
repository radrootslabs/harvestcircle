use harvestcircle_application::{BoxFuture, IdentityNamespaceRepository, IdentityPreferenceKey};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage};
use sqlx::Row;

use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::{Database, HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES};

impl IdentityNamespaceRepository for Database {
    fn get_value<'a>(
        &'a self,
        owner: PublicKey,
        key: IdentityPreferenceKey,
    ) -> BoxFuture<'a, Result<Option<String>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let rows = sqlx::query(
                            "SELECT substr(CAST(preference_value AS BLOB), 1, 4097) AS value, \
                             length(CAST(preference_value AS BLOB)) AS value_bytes \
                             FROM account_preferences WHERE owner_public_key = ? \
                             AND preference_key = ? LIMIT 2",
                        )
                        .bind(owner.as_bytes().as_slice())
                        .bind(encode_key(key))
                        .fetch_all(&mut *transaction)
                        .await
                        .map_err(|_| corrupt_storage())?;
                        match rows.as_slice() {
                            [] => Ok(None),
                            [row] => {
                                let value = row
                                    .try_get::<Vec<u8>, _>("value")
                                    .map_err(|_| corrupt_storage())?;
                                let length = row
                                    .try_get::<i64, _>("value_bytes")
                                    .map_err(|_| corrupt_storage())?;
                                let length =
                                    usize::try_from(length).map_err(|_| corrupt_storage())?;
                                if length == 0
                                    || length > HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES
                                    || length != value.len()
                                {
                                    return Err(corrupt_storage());
                                }
                                String::from_utf8(value)
                                    .map(Some)
                                    .map_err(|_| corrupt_storage())
                            }
                            _ => Err(corrupt_storage()),
                        }
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn set_value<'a>(
        &'a self,
        owner: PublicKey,
        key: IdentityPreferenceKey,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            if value.is_empty()
                || value.len() > HARVESTCIRCLE_PREFERENCE_VALUE_UTF8_BYTES
                || value.chars().any(char::is_control)
            {
                return Err(invalid_preference());
            }
            let value = value.to_owned();
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let result = sqlx::query(
                            "INSERT INTO account_preferences \
                             (owner_public_key, preference_key, preference_value) VALUES (?, ?, ?) \
                             ON CONFLICT(owner_public_key, preference_key) DO UPDATE SET \
                             preference_value = excluded.preference_value",
                        )
                        .bind(owner.as_bytes().as_slice())
                        .bind(encode_key(key))
                        .bind(&value)
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

    fn clear_owner(&self, owner: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        sqlx::query("DELETE FROM account_preferences WHERE owner_public_key = ?")
                            .bind(owner.as_bytes().as_slice())
                            .execute(&mut *transaction)
                            .await
                            .map(|_| ())
                            .map_err(|_| storage_unavailable())
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }
}

const fn encode_key(key: IdentityPreferenceKey) -> &'static str {
    match key {
        IdentityPreferenceKey::NamespaceProbe => "namespace_probe",
    }
}

const fn invalid_preference() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidIdentityMetadata,
        SafeMessage::new("The identity preference is invalid."),
    )
}
