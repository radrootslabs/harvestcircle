use std::fs;

use radroots_studio_application::{AccountOperationKind, AccountRepository, OperationJournal};
use radroots_studio_domain::{
    AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, LocalSignerBinding,
    PublicKey, UnixTimestamp,
};
use radroots_studio_storage::Database;
use tempfile::tempdir;

const SECRET_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SECRET_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
fn assert_redacted(bytes: &[u8]) {
    assert!(
        !bytes
            .windows(SECRET_HEX.len())
            .any(|value| value == SECRET_HEX.as_bytes())
    );
    assert!(
        !bytes
            .windows(SECRET_NSEC.len())
            .any(|value| value == SECRET_NSEC.as_bytes())
    );
    assert!(!bytes.windows(5).any(|value| value == b"nsec1"));
}

#[test]
fn redaction_guards_sqlite_schema_and_non_secret_records() {
    let directory = tempdir().expect("directory");
    let path = directory.path().join("studio.sqlite3");
    {
        let database = Database::open(&path).expect("database");
        let public_key = PublicKey::from_bytes([2; 32]);
        let account = AccountSummary::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
        .expect("account");
        database.insert_account(&account).expect("account");
        database
            .begin_operation(
                AccountOperationKind::Add,
                account.public_key(),
                UnixTimestamp::from_seconds(2).expect("time"),
            )
            .expect("journal");
    }
    assert_redacted(&fs::read(path).expect("database bytes"));
}
