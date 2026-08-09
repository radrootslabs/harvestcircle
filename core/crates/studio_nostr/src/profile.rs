use nostr::{Event, JsonUtil, Kind, Metadata};
use radroots_studio_domain::{
    EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, UnixTimestamp,
};

const MAX_EVENT_JSON_BYTES: usize = 64 * 1_024;
const MAX_PROFILE_CONTENT_BYTES: usize = 16 * 1_024;

/// Verifies and converts one serialized Nostr kind-0 event.
///
/// # Errors
///
/// Returns a safe profile-refresh error when the event is oversized,
/// malformed, invalidly signed, authored by another key, or not kind 0.
pub fn parse_verified_kind0(
    event_json: &str,
    expected_author: PublicKey,
) -> Result<Kind0ProfileCandidate, SafeError> {
    if event_json.len() > MAX_EVENT_JSON_BYTES {
        return Err(invalid_event());
    }

    let event = Event::from_json(event_json).map_err(|_| invalid_event())?;
    event.verify().map_err(|_| invalid_event())?;
    if event.kind != Kind::Metadata
        || event.pubkey.to_bytes() != *expected_author.as_bytes()
        || event.content.len() > MAX_PROFILE_CONTENT_BYTES
    {
        return Err(invalid_event());
    }

    let metadata = Metadata::from_json(&event.content).map_err(|_| invalid_metadata())?;
    let profile = ProfileMetadata::new(
        metadata.name,
        metadata.display_name,
        metadata.nip05,
        metadata.about,
        metadata.picture,
    )?;
    let created_at = i64::try_from(event.created_at.as_secs())
        .ok()
        .and_then(UnixTimestamp::from_seconds)
        .ok_or_else(invalid_event)?;

    Ok(Kind0ProfileCandidate::new(
        EventId::from_bytes(event.id.to_bytes()),
        expected_author,
        created_at,
        profile,
    ))
}

const fn invalid_event() -> SafeError {
    SafeError::new(
        SafeErrorCode::ProfileRefreshFailed,
        SafeMessage::new("The Nostr profile event is invalid."),
    )
}

const fn invalid_metadata() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidProfileMetadata,
        SafeMessage::new("The Nostr profile metadata is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, JsonUtil, Keys, Metadata, Url};
    use radroots_studio_domain::{PublicKey, SafeErrorCode};

    use super::parse_verified_kind0;

    fn signed_profile() -> (Keys, String) {
        let keys = Keys::generate();
        let event = EventBuilder::metadata(
            &Metadata::new()
                .name(" farmer ")
                .display_name(" Farm Account ")
                .nip05("farmer@example.test")
                .about("Local grower")
                .picture(
                    Url::parse("https://images.example.test/farmer.png")
                        .expect("valid picture URL"),
                ),
        )
        .sign_with_keys(&keys)
        .expect("signed metadata event");
        (keys, event.as_json())
    }

    #[test]
    fn profile_event_verifies_signature_author_kind_and_metadata() {
        let (keys, json) = signed_profile();
        let expected_author =
            PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key");

        let candidate = parse_verified_kind0(&json, expected_author).expect("verified profile");

        assert_eq!(candidate.author(), expected_author);
        assert_eq!(candidate.metadata().name(), Some("farmer"));
        assert_eq!(candidate.metadata().display_name(), Some("Farm Account"));
        assert_eq!(candidate.metadata().nip05(), Some("farmer@example.test"));
        assert_eq!(candidate.metadata().about(), Some("Local grower"));
        assert_eq!(
            candidate.metadata().picture(),
            Some("https://images.example.test/farmer.png")
        );
    }

    #[test]
    fn profile_event_rejects_tampering_wrong_author_kind_and_oversize_content() {
        let (keys, json) = signed_profile();
        let expected_author =
            PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key");
        let wrong_author = PublicKey::from_bytes(Keys::generate().public_key().to_bytes())
            .expect("valid public key");
        let tampered = json.replace("Local grower", "Remote grower");
        let note = EventBuilder::text_note("not metadata")
            .sign_with_keys(&keys)
            .expect("signed note")
            .as_json();
        let oversized = EventBuilder::metadata(&Metadata::new().about("x".repeat(16 * 1_024 + 1)))
            .sign_with_keys(&keys)
            .expect("signed oversized profile")
            .as_json();

        for rejected in [
            parse_verified_kind0(&tampered, expected_author),
            parse_verified_kind0(&json, wrong_author),
            parse_verified_kind0(&note, expected_author),
            parse_verified_kind0(&oversized, expected_author),
        ] {
            assert_eq!(
                rejected.expect_err("invalid event").code(),
                SafeErrorCode::ProfileRefreshFailed
            );
        }
    }

    #[test]
    fn profile_event_rejects_malformed_and_bounded_invalid_metadata() {
        let keys = Keys::generate();
        let malformed = EventBuilder::new(nostr::Kind::Metadata, "not json")
            .sign_with_keys(&keys)
            .expect("signed malformed metadata")
            .as_json();
        let invalid = EventBuilder::metadata(&Metadata::new().name("x".repeat(129)))
            .sign_with_keys(&keys)
            .expect("signed invalid metadata")
            .as_json();
        let author = PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key");

        assert_eq!(
            parse_verified_kind0(&malformed, author)
                .expect_err("malformed metadata")
                .code(),
            SafeErrorCode::InvalidProfileMetadata
        );
        assert_eq!(
            parse_verified_kind0(&invalid, author)
                .expect_err("bounded metadata")
                .code(),
            SafeErrorCode::InvalidProfileMetadata
        );
        assert_eq!(
            parse_verified_kind0(&"x".repeat(64 * 1_024 + 1), author)
                .expect_err("oversized event")
                .code(),
            SafeErrorCode::ProfileRefreshFailed
        );
        assert_eq!(
            super::invalid_event().code(),
            SafeErrorCode::ProfileRefreshFailed
        );
        assert_eq!(
            super::invalid_metadata().code(),
            SafeErrorCode::InvalidProfileMetadata
        );
    }
}
