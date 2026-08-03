use radroots_studio_application::{CachedProfile, ProfileRefreshStatus, ProfileRepository};
use radroots_studio_domain::{
    EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, UnixTimestamp,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::Database;

impl ProfileRepository for Database {
    fn load_profile(&self, public_key: PublicKey) -> Result<Option<CachedProfile>, SafeError> {
        self.connection()
            .query_row(
                "SELECT event_id, event_created_at, name, display_name, nip05, about, picture, \
                 refreshed_at, refresh_status FROM profile_cache WHERE subject_pubkey = ?1",
                [public_key.to_hex()],
                |row| decode_profile(row, public_key),
            )
            .optional()
            .map_err(|_| corrupt_storage_error())
    }

    fn save_profile(&self, profile: &CachedProfile) -> Result<(), SafeError> {
        let candidate = profile.candidate();
        let metadata = candidate.metadata();
        self.connection()
            .execute(
                "INSERT INTO profile_cache (subject_pubkey, event_id, event_created_at, name, \
                 display_name, nip05, about, picture, refreshed_at, refresh_status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT(subject_pubkey) DO UPDATE SET \
                 event_id = excluded.event_id, event_created_at = excluded.event_created_at, \
                 name = excluded.name, display_name = excluded.display_name, nip05 = excluded.nip05, \
                 about = excluded.about, picture = excluded.picture, \
                 refreshed_at = excluded.refreshed_at, refresh_status = excluded.refresh_status \
                 WHERE excluded.event_created_at > profile_cache.event_created_at \
                 OR (excluded.event_created_at = profile_cache.event_created_at \
                 AND excluded.event_id < profile_cache.event_id)",
                params![
                    candidate.author().to_hex(),
                    candidate.event_id().to_hex(),
                    candidate.created_at().as_seconds(),
                    metadata.name(),
                    metadata.display_name(),
                    metadata.nip05(),
                    metadata.about(),
                    metadata.picture(),
                    profile.refreshed_at().as_seconds(),
                    encode_refresh_status(profile.refresh_status()),
                ],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    fn record_refresh_status(
        &self,
        public_key: PublicKey,
        refreshed_at: UnixTimestamp,
        status: ProfileRefreshStatus,
    ) -> Result<(), SafeError> {
        self.connection()
            .execute(
                "UPDATE profile_cache SET refreshed_at = ?2, refresh_status = ?3 \
                 WHERE subject_pubkey = ?1",
                params![
                    public_key.to_hex(),
                    refreshed_at.as_seconds(),
                    encode_refresh_status(status)
                ],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    fn remove_profile(&self, public_key: PublicKey) -> Result<(), SafeError> {
        self.connection()
            .execute(
                "DELETE FROM profile_cache WHERE subject_pubkey = ?1",
                [public_key.to_hex()],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }
}

fn decode_profile(row: &Row<'_>, author: PublicKey) -> rusqlite::Result<CachedProfile> {
    let event_id =
        EventId::from_hex(row.get::<_, String>(0)?.as_str()).map_err(|_| invalid_column(0))?;
    let created_at = UnixTimestamp::from_seconds(row.get(1)?).ok_or_else(|| invalid_column(1))?;
    let metadata = ProfileMetadata::new(
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    )
    .map_err(|_| invalid_column(2))?;
    let refreshed_at = UnixTimestamp::from_seconds(row.get(7)?).ok_or_else(|| invalid_column(7))?;
    let refresh_status = decode_refresh_status(row.get::<_, String>(8)?.as_str())?;
    Ok(CachedProfile::new(
        Kind0ProfileCandidate::new(event_id, author, created_at, metadata),
        refreshed_at,
        refresh_status,
    ))
}

const fn encode_refresh_status(status: ProfileRefreshStatus) -> &'static str {
    match status {
        ProfileRefreshStatus::Success => "success",
        ProfileRefreshStatus::Offline => "offline",
        ProfileRefreshStatus::InvalidData => "invalid_data",
    }
}

fn decode_refresh_status(value: &str) -> rusqlite::Result<ProfileRefreshStatus> {
    match value {
        "success" => Ok(ProfileRefreshStatus::Success),
        "offline" => Ok(ProfileRefreshStatus::Offline),
        "invalid_data" => Ok(ProfileRefreshStatus::InvalidData),
        _ => Err(invalid_column(8)),
    }
}

fn invalid_column(index: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        index,
        "cached Nostr profile".to_owned(),
        rusqlite::types::Type::Text,
    )
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The profile cache is unavailable."),
    )
}

const fn corrupt_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The profile cache could not be read."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_application::{
        AccountRepository, CachedProfile, ProfileRefreshStatus, ProfileRepository,
    };
    use radroots_studio_domain::{
        AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, EventId,
        Kind0ProfileCandidate, LocalSignerBinding, ProfileMetadata, PublicKey, UnixTimestamp,
    };

    use crate::Database;

    fn account(public_key: PublicKey) -> AccountSummary {
        AccountSummary::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
        .expect("account")
    }

    fn profile(public_key: PublicKey, id: u8, created_at: i64, name: &str) -> CachedProfile {
        CachedProfile::new(
            Kind0ProfileCandidate::new(
                EventId::from_bytes([id; 32]),
                public_key,
                UnixTimestamp::from_seconds(created_at).expect("time"),
                ProfileMetadata::new(Some(name.to_owned()), None, None, None, None)
                    .expect("metadata"),
            ),
            UnixTimestamp::from_seconds(created_at + 1).expect("refresh time"),
            ProfileRefreshStatus::Success,
        )
    }

    #[test]
    fn profile_cache_round_trips_and_records_refresh_status() {
        let database = Database::in_memory().expect("database");
        let public_key = PublicKey::from_bytes([1; 32]);
        database
            .insert_account(&account(public_key))
            .expect("account");
        database
            .save_profile(&profile(public_key, 1, 10, "Farm"))
            .expect("save profile");
        database
            .record_refresh_status(
                public_key,
                UnixTimestamp::from_seconds(20).expect("time"),
                ProfileRefreshStatus::Offline,
            )
            .expect("record status");

        let loaded = database
            .load_profile(public_key)
            .expect("load profile")
            .expect("cached profile");
        assert_eq!(loaded.candidate().metadata().name(), Some("Farm"));
        assert_eq!(loaded.refreshed_at().as_seconds(), 20);
        assert_eq!(loaded.refresh_status(), ProfileRefreshStatus::Offline);
    }

    #[test]
    fn profile_cache_keeps_newest_then_lowest_event_id() {
        let database = Database::in_memory().expect("database");
        let public_key = PublicKey::from_bytes([2; 32]);
        database
            .insert_account(&account(public_key))
            .expect("account");
        database
            .save_profile(&profile(public_key, 9, 20, "High ID"))
            .expect("initial");
        database
            .save_profile(&profile(public_key, 1, 20, "Low ID"))
            .expect("equal newer candidate");
        database
            .save_profile(&profile(public_key, 0, 10, "Older"))
            .expect("older candidate");

        let loaded = database
            .load_profile(public_key)
            .expect("load")
            .expect("profile");
        assert_eq!(loaded.candidate().metadata().name(), Some("Low ID"));
        assert_eq!(loaded.candidate().event_id(), EventId::from_bytes([1; 32]));
    }

    #[test]
    fn profile_cache_cascades_with_account_removal() {
        let database = Database::in_memory().expect("database");
        let public_key = PublicKey::from_bytes([3; 32]);
        database
            .insert_account(&account(public_key))
            .expect("account");
        database
            .save_profile(&profile(public_key, 1, 10, "Farm"))
            .expect("profile");
        database.remove_account(public_key).expect("remove account");

        assert_eq!(database.load_profile(public_key).expect("load"), None);
    }
}
