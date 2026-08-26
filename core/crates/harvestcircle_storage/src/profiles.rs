use harvestcircle_application::{
    BoxFuture, CachedProfile, ProfileRefreshStatus, ProfileRepository,
};
use harvestcircle_domain::{
    EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError, UnixTimestamp,
};
use sqlx::Row;

use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};

const PROFILE_PROJECTION: &str = "SELECT substr(event_id, 1, 33) AS event_id, \
    length(event_id) AS event_id_bytes, event_created_at_unix_s, \
    CASE WHEN name IS NULL THEN NULL ELSE substr(CAST(name AS BLOB), 1, 129) END AS name, \
    CASE WHEN name IS NULL THEN NULL ELSE length(CAST(name AS BLOB)) END AS name_bytes, \
    CASE WHEN display_name IS NULL THEN NULL ELSE substr(CAST(display_name AS BLOB), 1, 129) END AS display_name, \
    CASE WHEN display_name IS NULL THEN NULL ELSE length(CAST(display_name AS BLOB)) END AS display_name_bytes, \
    CASE WHEN nip05 IS NULL THEN NULL ELSE substr(CAST(nip05 AS BLOB), 1, 321) END AS nip05, \
    CASE WHEN nip05 IS NULL THEN NULL ELSE length(CAST(nip05 AS BLOB)) END AS nip05_bytes, \
    CASE WHEN about IS NULL THEN NULL ELSE substr(CAST(about AS BLOB), 1, 4097) END AS about, \
    CASE WHEN about IS NULL THEN NULL ELSE length(CAST(about AS BLOB)) END AS about_bytes, \
    CASE WHEN picture IS NULL THEN NULL ELSE substr(CAST(picture AS BLOB), 1, 2049) END AS picture, \
    CASE WHEN picture IS NULL THEN NULL ELSE length(CAST(picture AS BLOB)) END AS picture_bytes, \
    refreshed_at_unix_s, substr(CAST(refresh_status AS BLOB), 1, 13) AS refresh_status, \
    length(CAST(refresh_status AS BLOB)) AS refresh_status_bytes FROM profile_cache";

impl ProfileRepository for Database {
    fn load_profile(
        &self,
        public_key: PublicKey,
    ) -> BoxFuture<'_, Result<Option<CachedProfile>, SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        let sql =
                            format!("{PROFILE_PROJECTION} WHERE subject_public_key = ? LIMIT 2");
                        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                            .bind(public_key.as_bytes().as_slice())
                            .fetch_all(&mut *transaction)
                            .await
                            .map_err(|_| corrupt_storage())?;
                        match rows.as_slice() {
                            [] => Ok(None),
                            [row] => decode_profile(row, public_key).map(Some),
                            _ => Err(corrupt_storage()),
                        }
                    })
                })
                .await
                .map_err(map_transaction_error)
        })
    }

    fn save_profile<'a>(
        &'a self,
        profile: &'a CachedProfile,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let candidate = profile.candidate();
            let metadata = candidate.metadata();
            let author = *candidate.author().as_bytes();
            let event_id = candidate.event_id().as_bytes();
            let event_created_at = candidate.created_at().as_seconds();
            let name = metadata.name().map(str::to_owned);
            let display_name = metadata.display_name().map(str::to_owned);
            let nip05 = metadata.nip05().map(str::to_owned);
            let about = metadata.about().map(str::to_owned);
            let picture = metadata.picture().map(str::to_owned);
            let refreshed_at = profile.refreshed_at().as_seconds();
            let refresh_status = encode_refresh_status(profile.refresh_status());
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        sqlx::query(
                            "INSERT INTO profile_cache (subject_public_key, event_id, \
                             event_created_at_unix_s, name, display_name, nip05, about, picture, \
                             refreshed_at_unix_s, refresh_status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                             ON CONFLICT(subject_public_key) DO UPDATE SET event_id = excluded.event_id, \
                             event_created_at_unix_s = excluded.event_created_at_unix_s, name = excluded.name, \
                             display_name = excluded.display_name, nip05 = excluded.nip05, about = excluded.about, \
                             picture = excluded.picture, refreshed_at_unix_s = excluded.refreshed_at_unix_s, \
                             refresh_status = excluded.refresh_status WHERE \
                             excluded.event_created_at_unix_s > profile_cache.event_created_at_unix_s \
                             OR (excluded.event_created_at_unix_s = profile_cache.event_created_at_unix_s \
                             AND excluded.event_id < profile_cache.event_id)",
                        )
                        .bind(author.as_slice())
                        .bind(event_id.as_slice())
                        .bind(event_created_at)
                        .bind(&name)
                        .bind(&display_name)
                        .bind(&nip05)
                        .bind(&about)
                        .bind(&picture)
                        .bind(refreshed_at)
                        .bind(refresh_status)
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

    fn record_refresh_status<'a>(
        &'a self,
        public_key: PublicKey,
        refreshed_at: UnixTimestamp,
        status: ProfileRefreshStatus,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        sqlx::query(
                            "UPDATE profile_cache SET refreshed_at_unix_s = ?, refresh_status = ? \
                             WHERE subject_public_key = ?",
                        )
                        .bind(refreshed_at.as_seconds())
                        .bind(encode_refresh_status(status))
                        .bind(public_key.as_bytes().as_slice())
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

    fn remove_profile(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.host()
                .transaction(|transaction| {
                    Box::pin(async move {
                        sqlx::query("DELETE FROM profile_cache WHERE subject_public_key = ?")
                            .bind(public_key.as_bytes().as_slice())
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

fn decode_profile(
    row: &sqlx::sqlite::SqliteRow,
    author: PublicKey,
) -> Result<CachedProfile, SafeError> {
    let event_id =
        bounded_blob(row, "event_id", "event_id_bytes", 32)?.ok_or_else(corrupt_storage)?;
    let event_id: [u8; 32] = event_id.try_into().map_err(|_| corrupt_storage())?;
    let created_at = UnixTimestamp::from_seconds(
        row.try_get("event_created_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    let metadata = ProfileMetadata::new(
        bounded_text(row, "name", "name_bytes", 128)?,
        bounded_text(row, "display_name", "display_name_bytes", 128)?,
        bounded_text(row, "nip05", "nip05_bytes", 320)?,
        bounded_text(row, "about", "about_bytes", 4_096)?,
        bounded_text(row, "picture", "picture_bytes", 2_048)?,
    )
    .map_err(|_| corrupt_storage())?;
    let refreshed_at = UnixTimestamp::from_seconds(
        row.try_get("refreshed_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    let status = bounded_text(row, "refresh_status", "refresh_status_bytes", 12)?
        .ok_or_else(corrupt_storage)?;
    Ok(CachedProfile::new(
        Kind0ProfileCandidate::new(EventId::from_bytes(event_id), author, created_at, metadata),
        refreshed_at,
        decode_refresh_status(&status)?,
    ))
}

fn bounded_text(
    row: &sqlx::sqlite::SqliteRow,
    value_column: &str,
    length_column: &str,
    maximum: usize,
) -> Result<Option<String>, SafeError> {
    bounded_blob(row, value_column, length_column, maximum)?
        .map(String::from_utf8)
        .transpose()
        .map_err(|_| corrupt_storage())
}

fn bounded_blob(
    row: &sqlx::sqlite::SqliteRow,
    value_column: &str,
    length_column: &str,
    maximum: usize,
) -> Result<Option<Vec<u8>>, SafeError> {
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
            Ok(Some(value))
        }
        _ => Err(corrupt_storage()),
    }
}

const fn encode_refresh_status(status: ProfileRefreshStatus) -> &'static str {
    match status {
        ProfileRefreshStatus::Success => "success",
        ProfileRefreshStatus::Offline => "offline",
        ProfileRefreshStatus::InvalidData => "invalid_data",
    }
}

fn decode_refresh_status(value: &str) -> Result<ProfileRefreshStatus, SafeError> {
    match value {
        "success" => Ok(ProfileRefreshStatus::Success),
        "offline" => Ok(ProfileRefreshStatus::Offline),
        "invalid_data" => Ok(ProfileRefreshStatus::InvalidData),
        _ => Err(corrupt_storage()),
    }
}
