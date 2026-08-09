use std::fs;

use radroots_studio_application::{
    AccountNamespaceRepository, AccountPreferenceKey, Clock, InMemorySecretStore,
    RelayConfiguration, SessionState,
};
use radroots_studio_domain::{SecretKeyInput, UnixTimestamp};
use radroots_studio_runtime::PersistentAppCore;
use tempfile::tempdir;

const SECRET_A: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
const SECRET_B: &str = "0101010101010101010101010101010101010101010101010101010101010101";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixTimestamp {
        UnixTimestamp::from_seconds(200).expect("fixed timestamp")
    }
}

#[test]
fn restart_restores_selection_and_keeps_account_namespaces_isolated() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("studio.sqlite3");
    let secrets = InMemorySecretStore::default();
    let (owner_a, owner_b);

    {
        let adapter = PersistentAppCore::open(&path, RelayConfiguration::default())
            .expect("persistent adapter");
        adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
        owner_a = adapter
            .import_secret_key(
                SecretKeyInput::parse(SECRET_A.to_owned()).expect("secret A"),
                &secrets,
                &FixedClock,
            )
            .expect("account A")
            .account()
            .public_key();
        owner_b = adapter
            .import_secret_key(
                SecretKeyInput::parse(SECRET_B.to_owned()).expect("secret B"),
                &secrets,
                &FixedClock,
            )
            .expect("account B")
            .account()
            .public_key();
        adapter
            .database()
            .set_value(owner_a, AccountPreferenceKey::NamespaceProbe, "account-a")
            .expect("namespace A");
        adapter
            .database()
            .set_value(owner_b, AccountPreferenceKey::NamespaceProbe, "account-b")
            .expect("namespace B");
        adapter.select_account(owner_b).expect("select B");
    }

    let reopened =
        PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen adapter");
    let restored = reopened.bootstrap(&secrets, &FixedClock).expect("restore");
    assert_eq!(restored.accounts().len(), 2);
    assert_eq!(restored.selected_account(), Some(owner_b));
    assert_eq!(restored.session(), SessionState::SignedOut);
    assert_eq!(
        reopened
            .database()
            .get_value(owner_a, AccountPreferenceKey::NamespaceProbe)
            .expect("read A"),
        Some("account-a".to_owned())
    );
    assert_eq!(
        reopened
            .database()
            .get_value(owner_b, AccountPreferenceKey::NamespaceProbe)
            .expect("read B"),
        Some("account-b".to_owned())
    );

    let database = fs::read(path).expect("database bytes");
    assert!(
        !database
            .windows(SECRET_A.len())
            .any(|bytes| bytes == SECRET_A.as_bytes())
    );
    assert!(
        !database
            .windows(SECRET_B.len())
            .any(|bytes| bytes == SECRET_B.as_bytes())
    );
    assert!(!database.windows(5).any(|bytes| bytes == b"nsec1"));
}
