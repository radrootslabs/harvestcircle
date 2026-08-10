use harvestcircle_application::{IdentityNamespaceRepository, IdentityPreferenceKey};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage};
use rusqlite::{OptionalExtension, params};

use crate::Database;

const MAX_VALUE_CHARS: usize = 4_096;

impl IdentityNamespaceRepository for Database {
    fn get_value(
        &self,
        owner: PublicKey,
        key: IdentityPreferenceKey,
    ) -> Result<Option<String>, SafeError> {
        self.connection()
            .query_row(
                "SELECT preference_value FROM account_preferences \
                 WHERE owner_public_key = ?1 AND preference_key = ?2",
                params![owner.to_hex(), encode_key(key)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn set_value(
        &self,
        owner: PublicKey,
        key: IdentityPreferenceKey,
        value: &str,
    ) -> Result<(), SafeError> {
        if value.chars().count() > MAX_VALUE_CHARS || value.chars().any(char::is_control) {
            return Err(invalid_preference());
        }
        self.connection()
            .execute(
                "INSERT INTO account_preferences (owner_public_key, preference_key, preference_value) \
                 VALUES (?1, ?2, ?3) ON CONFLICT(owner_public_key, preference_key) DO UPDATE SET \
                 preference_value = excluded.preference_value",
                params![owner.to_hex(), encode_key(key), value],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    fn clear_owner(&self, owner: PublicKey) -> Result<(), SafeError> {
        self.connection()
            .execute(
                "DELETE FROM account_preferences WHERE owner_public_key = ?1",
                [owner.to_hex()],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }
}

const fn encode_key(key: IdentityPreferenceKey) -> &'static str {
    match key {
        IdentityPreferenceKey::NamespaceProbe => "namespace_probe",
    }
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The identity preference is unavailable."),
    )
}

const fn invalid_preference() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidIdentityMetadata,
        SafeMessage::new("The identity preference is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_application::{
        AppStateRepository, IdentityNamespaceRepository, IdentityPreferenceKey, IdentityRepository,
    };
    use harvestcircle_domain::{
        IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
        SignerAvailability, UnixTimestamp,
    };

    use crate::Database;

    fn public_key(byte: u8) -> PublicKey {
        let value = match byte {
            1 => "585591529da0bab31b3b1b1f986611cf5f435dca84f978c89ee8a40cca7103df",
            2 => "e0266e3cfb0d2886f91c73f5f868f3b98273713e5fcd97c081663f5518a4b3af",
            _ => "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
        };
        PublicKey::from_hex(value).expect("valid public key")
    }

    fn identity(byte: u8) -> NostrIdentity {
        let public_key = public_key(byte);
        NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(i64::from(byte)).expect("time")),
            None,
        )
        .expect("identity")
    }

    #[test]
    fn namespace_partitions_same_typed_key_by_owner_and_selection() {
        let database = Database::in_memory().expect("database");
        let owner_a = public_key(1);
        let owner_b = public_key(2);
        database.insert_identity(&identity(1)).expect("identity a");
        database.insert_identity(&identity(2)).expect("identity b");
        database
            .set_value(owner_a, IdentityPreferenceKey::NamespaceProbe, "A")
            .expect("set a");
        database
            .set_value(owner_b, IdentityPreferenceKey::NamespaceProbe, "B")
            .expect("set b");

        database
            .save_selected_identity(Some(owner_b))
            .expect("select b");
        let selected = database
            .load_selected_identity()
            .expect("selection")
            .expect("selected owner");
        assert_eq!(
            database
                .get_value(selected, IdentityPreferenceKey::NamespaceProbe)
                .expect("selected value"),
            Some("B".to_owned())
        );
        assert_eq!(
            database
                .get_value(owner_a, IdentityPreferenceKey::NamespaceProbe)
                .expect("owner a value"),
            Some("A".to_owned())
        );
    }

    #[test]
    fn namespace_updates_and_cascades_with_owner_removal() {
        let database = Database::in_memory().expect("database");
        let owner = public_key(3);
        database.insert_identity(&identity(3)).expect("identity");
        database
            .set_value(owner, IdentityPreferenceKey::NamespaceProbe, "before")
            .expect("set");
        database
            .set_value(owner, IdentityPreferenceKey::NamespaceProbe, "after")
            .expect("update");
        assert_eq!(
            database
                .get_value(owner, IdentityPreferenceKey::NamespaceProbe)
                .expect("value"),
            Some("after".to_owned())
        );

        database.remove_identity(owner).expect("remove");
        assert_eq!(
            database
                .get_value(owner, IdentityPreferenceKey::NamespaceProbe)
                .expect("deleted value"),
            None
        );
    }

    #[test]
    fn namespace_rejects_oversized_and_control_character_values() {
        let database = Database::in_memory().expect("database");
        let owner = public_key(3);
        database.insert_identity(&identity(3)).expect("identity");
        let oversized = "a".repeat(super::MAX_VALUE_CHARS + 1);
        assert!(
            database
                .set_value(owner, IdentityPreferenceKey::NamespaceProbe, &oversized)
                .is_err()
        );
        assert!(
            database
                .set_value(owner, IdentityPreferenceKey::NamespaceProbe, "line\nbreak")
                .is_err()
        );
    }
}
