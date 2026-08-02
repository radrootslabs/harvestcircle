use radroots_studio_application::{
    AppSnapshot, RelayConfiguration, SessionState, SnapshotRevision,
};
use radroots_studio_domain::{
    AccountCreatedAt, AccountSummary, KeyAvailability, Npub, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, SignerKind, UnixTimestamp,
};

const SECRET_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SECRET_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

fn assert_redacted(text: &str) {
    assert!(!text.contains(SECRET_HEX));
    assert!(!text.contains(SECRET_NSEC));
    assert!(!text.contains("nsec1"));
}

#[test]
fn redaction_guards_public_snapshot_and_safe_error_debug() {
    let account = AccountSummary::new(
        PublicKey::from_bytes([2; 32]),
        Npub::from_encoded(NPUB.to_owned()).expect("npub"),
        SignerKind::LocalSecret,
        KeyAvailability::Available,
        None,
        AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
        None,
    );
    let snapshot = AppSnapshot::ready(
        SnapshotRevision::from_value(1),
        RelayConfiguration::default(),
        vec![account.clone()],
        Some(account.public_key()),
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
