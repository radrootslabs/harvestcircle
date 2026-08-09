use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};
use rusqlite::OptionalExtension;

use crate::Database;

impl Database {
    pub fn load_installation_id(&self) -> Result<Option<String>, SafeError> {
        self.connection()
            .query_row(
                "SELECT installation_id FROM installation_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| installation_storage_error())
    }

    pub fn initialize_installation_id(&self, candidate: &str) -> Result<String, SafeError> {
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO installation_identity (singleton, installation_id) VALUES (1, ?1) ON CONFLICT(singleton) DO NOTHING",
                [candidate],
            )
            .map_err(|_| installation_storage_error())?;
        connection
            .query_row(
                "SELECT installation_id FROM installation_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| installation_storage_error())
    }
}

const fn installation_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The installation identity is unavailable."),
    )
}

#[cfg(test)]
mod tests {
    use crate::Database;

    #[test]
    fn installation_identity_is_insert_once_and_stable() {
        let database = Database::in_memory().expect("database");
        assert_eq!(database.load_installation_id().expect("empty"), None);
        let first = database
            .initialize_installation_id("11aabbccddeeff001122334455667788")
            .expect("first identity");
        let second = database
            .initialize_installation_id("22aabbccddeeff001122334455667788")
            .expect("existing identity");
        assert_eq!(first, "11aabbccddeeff001122334455667788");
        assert_eq!(second, first);
        assert_eq!(database.load_installation_id().expect("load"), Some(first));
    }

    #[test]
    fn installation_identity_rejects_invalid_values() {
        let database = Database::in_memory().expect("database");
        assert!(
            database
                .initialize_installation_id("not-an-identity")
                .is_err()
        );
    }
}
