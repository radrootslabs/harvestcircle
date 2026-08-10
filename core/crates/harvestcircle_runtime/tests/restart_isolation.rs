use std::fs;

use harvestcircle_application::{
    Clock, IdentityNamespaceRepository, IdentityPreferenceKey, InMemorySecretStore,
    RelayConfiguration, SessionState,
};
use harvestcircle_domain::{SecretKeyInput, UnixTimestamp};
use harvestcircle_runtime::PersistentAppCore;
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
fn restart_restores_selection_and_keeps_identity_namespaces_isolated() {
    let directory = tempdir().expect("temporary directory");
    let path = directory
        .path()
        .canonicalize()
        .expect("canonical temporary directory")
        .join("harvestcircle.sqlite3");
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
            .expect("identity A")
            .identity()
            .public_key();
        owner_b = adapter
            .import_secret_key(
                SecretKeyInput::parse(SECRET_B.to_owned()).expect("secret B"),
                &secrets,
                &FixedClock,
            )
            .expect("identity B")
            .identity()
            .public_key();
        adapter
            .database()
            .set_value(owner_a, IdentityPreferenceKey::NamespaceProbe, "identity-a")
            .expect("namespace A");
        adapter
            .database()
            .set_value(owner_b, IdentityPreferenceKey::NamespaceProbe, "identity-b")
            .expect("namespace B");
        adapter.select_identity(owner_b).expect("select B");
    }

    let reopened =
        PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen adapter");
    let restored = reopened.bootstrap(&secrets, &FixedClock).expect("restore");
    assert_eq!(restored.identities().len(), 2);
    assert_eq!(restored.selected_identity(), Some(owner_b));
    assert_eq!(restored.session(), SessionState::SignedOut);
    assert_eq!(
        reopened
            .database()
            .get_value(owner_a, IdentityPreferenceKey::NamespaceProbe)
            .expect("read A"),
        Some("identity-a".to_owned())
    );
    assert_eq!(
        reopened
            .database()
            .get_value(owner_b, IdentityPreferenceKey::NamespaceProbe)
            .expect("read B"),
        Some("identity-b".to_owned())
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
