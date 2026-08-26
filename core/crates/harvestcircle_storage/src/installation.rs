use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};
use sqlx::Row;

use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};

impl Database {
    pub async fn load_installation_id(&self) -> Result<Option<String>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let rows = sqlx::query(
                        "SELECT substr(installation_id, 1, 17) AS installation_id, \
                         length(installation_id) AS installation_id_bytes \
                         FROM installation_identity WHERE singleton = 1 LIMIT 2",
                    )
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(|_| corrupt_storage())?;
                    match rows.as_slice() {
                        [] => Ok(None),
                        [row] => decode_installation_id(row).map(Some),
                        _ => Err(corrupt_storage()),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn initialize_installation_id(&self, candidate: &str) -> Result<String, SafeError> {
        let candidate = decode_hex(candidate)?;
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let result = sqlx::query(
                        "INSERT INTO installation_identity (singleton, installation_id) \
                         VALUES (1, ?) ON CONFLICT(singleton) DO NOTHING",
                    )
                    .bind(candidate.as_slice())
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| storage_unavailable())?;
                    if result.rows_affected() > 1 {
                        return Err(corrupt_storage());
                    }
                    let rows = sqlx::query(
                        "SELECT substr(installation_id, 1, 17) AS installation_id, \
                         length(installation_id) AS installation_id_bytes \
                         FROM installation_identity WHERE singleton = 1 LIMIT 2",
                    )
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(|_| corrupt_storage())?;
                    let [row] = rows.as_slice() else {
                        return Err(corrupt_storage());
                    };
                    decode_installation_id(row)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

fn decode_installation_id(row: &sqlx::sqlite::SqliteRow) -> Result<String, SafeError> {
    let value = row
        .try_get::<Vec<u8>, _>("installation_id")
        .map_err(|_| corrupt_storage())?;
    if row
        .try_get::<i64, _>("installation_id_bytes")
        .map_err(|_| corrupt_storage())?
        != 16
        || value.len() != 16
    {
        return Err(corrupt_storage());
    }
    Ok(encode_hex(&value))
}

fn decode_hex(value: &str) -> Result<[u8; 16], SafeError> {
    if value.len() != 32 {
        return Err(invalid_installation_identity());
    }
    let mut output = [0_u8; 16];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let high = hex_nibble(pair[0]).ok_or_else(invalid_installation_identity)?;
        let low = hex_nibble(pair[1]).ok_or_else(invalid_installation_identity)?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn encode_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

const fn invalid_installation_identity() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The installation identity is invalid."),
    )
}
