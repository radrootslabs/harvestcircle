use harvestcircle_application::{AppSnapshot, RelayConfiguration, SessionState, SnapshotRevision};
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
    SafeError, SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
};

const SECRET_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SECRET_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
fn assert_redacted(text: &str) {
    assert!(!text.contains(SECRET_HEX));
    assert!(!text.contains(SECRET_NSEC));
    assert!(!text.contains("nsec1"));
}

#[test]
fn redaction_guards_public_snapshot_and_safe_error_debug() {
    let identity = NostrIdentity::new(
        NostrIdentityReference::derive(PublicKey::from_bytes([7; 32]).expect("valid public key"))
            .expect("identity"),
        LocalKeyringBinding::new(
            PublicKey::from_bytes([7; 32]).expect("valid public key"),
            SignerAvailability::Available,
        ),
        None,
        IdentityCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
        None,
    )
    .expect("identity");
    let snapshot = AppSnapshot::ready(
        SnapshotRevision::from_value(1),
        RelayConfiguration::default(),
        vec![identity.clone()],
        Some(identity.public_key()),
        SessionState::SignedOut,
        None,
        None,
    )
    .expect("snapshot");
    let error = SafeError::new(
        SafeErrorCode::KeyringUnavailable,
        SafeMessage::new("The operating system credential store is unavailable."),
    );

    assert_redacted(&format!("{snapshot:?}"));
    assert_redacted(&format!("{error:?} {error}"));
}
