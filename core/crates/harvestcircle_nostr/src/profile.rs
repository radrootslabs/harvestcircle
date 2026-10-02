use harvestcircle_domain::{
    EventId, Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, UnixTimestamp,
};
use nostr::{Event, JsonUtil, Kind, Metadata};

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
    use harvestcircle_domain::{PublicKey, SafeErrorCode};
    use nostr::{EventBuilder, JsonUtil, Keys, Metadata, Url};

    use super::parse_verified_kind0;

    fn signed_profile() -> (Keys, String) {
        let keys = Keys::generate();
        let event = EventBuilder::metadata(
            &Metadata::new()
                .name(" farmer ")
                .display_name(" Farm Identity ")
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

    fn text_at_utf8_limit(unit: &str, maximum: usize) -> String {
        unit.repeat(maximum / unit.len()) + &"x".repeat(maximum % unit.len())
    }

    fn signed_metadata(fields: [Option<String>; 5]) -> (PublicKey, String) {
        let [name, display_name, nip05, about, picture] = fields;
        let mut metadata = Metadata::new();
        metadata.name = name;
        metadata.display_name = display_name;
        metadata.nip05 = nip05;
        metadata.about = about;
        metadata.picture = picture;
        let keys = Keys::generate();
        let event = EventBuilder::new(nostr::Kind::Metadata, metadata.as_json())
            .sign_with_keys(&keys)
            .expect("signed metadata");
        event
            .verify()
            .expect("fixture has a valid signature and event ID");
        let author = PublicKey::from_bytes(keys.public_key().to_bytes()).expect("public author");
        (author, event.as_json())
    }

    fn assert_signed_one_byte_over_limit_is_rejected(index: usize, maximum: usize) {
        for unit in ["x", "é", "🥕", "e\u{301}"] {
            let value = text_at_utf8_limit(unit, maximum) + "x";
            assert_eq!(value.len(), maximum + 1);
            let mut fields: [Option<String>; 5] = std::array::from_fn(|_| None);
            fields[index] = Some(format!(" {value} "));
            let (author, json) = signed_metadata(fields);
            assert_eq!(
                parse_verified_kind0(&json, author)
                    .expect_err("correctly signed overlimit metadata")
                    .code(),
                SafeErrorCode::InvalidProfileMetadata
            );
        }
    }

    #[test]
    fn signed_kind0_accepts_exact_utf8_profile_boundaries() {
        for unit in ["x", "é", "🥕", "e\u{301}"] {
            let fields = [128, 128, 320, 4_096, 2_048]
                .map(|maximum| Some(text_at_utf8_limit(unit, maximum)));
            let (author, json) = signed_metadata(fields.clone());
            let candidate = parse_verified_kind0(&json, author).expect("signed UTF-8 boundaries");
            assert_eq!(candidate.author(), author);
            assert_eq!(candidate.metadata().name(), fields[0].as_deref());
            assert_eq!(candidate.metadata().display_name(), fields[1].as_deref());
            assert_eq!(candidate.metadata().nip05(), fields[2].as_deref());
            assert_eq!(candidate.metadata().about(), fields[3].as_deref());
            assert_eq!(candidate.metadata().picture(), fields[4].as_deref());
        }
    }

    #[test]
    fn signed_kind0_rejects_name_one_byte_over_utf8_limit() {
        assert_signed_one_byte_over_limit_is_rejected(0, 128);
    }

    #[test]
    fn signed_kind0_rejects_display_name_one_byte_over_utf8_limit() {
        assert_signed_one_byte_over_limit_is_rejected(1, 128);
    }

    #[test]
    fn signed_kind0_rejects_nip05_one_byte_over_utf8_limit() {
        assert_signed_one_byte_over_limit_is_rejected(2, 320);
    }

    #[test]
    fn signed_kind0_rejects_about_one_byte_over_utf8_limit() {
        assert_signed_one_byte_over_limit_is_rejected(3, 4_096);
    }

    #[test]
    fn signed_kind0_rejects_picture_one_byte_over_utf8_limit() {
        assert_signed_one_byte_over_limit_is_rejected(4, 2_048);
    }

    #[test]
    fn signed_kind0_preserves_trim_blank_and_about_layout_policy() {
        let (author, json) = signed_metadata([
            Some(" \u{2003}e\u{301}\u{2003} ".to_owned()),
            Some(" \t ".to_owned()),
            None,
            Some(" \nFirst\nSecond\rThird\tFourth\t ".to_owned()),
            Some(" ".to_owned()),
        ]);
        let candidate = parse_verified_kind0(&json, author).expect("normalized signed metadata");
        assert_eq!(candidate.metadata().name(), Some("e\u{301}"));
        assert_eq!(candidate.metadata().display_name(), None);
        assert_eq!(candidate.metadata().nip05(), None);
        assert_eq!(
            candidate.metadata().about(),
            Some("First\nSecond\rThird\tFourth")
        );
        assert_eq!(candidate.metadata().picture(), None);

        for index in 0..5 {
            for control in ['\0', '\u{1b}', '\u{7f}', '\u{85}', '\n', '\r', '\t'] {
                if index == 3 && matches!(control, '\n' | '\r' | '\t') {
                    continue;
                }
                let mut fields: [Option<String>; 5] = std::array::from_fn(|_| None);
                fields[index] = Some(format!("a{control}b"));
                let (author, json) = signed_metadata(fields);
                assert_eq!(
                    parse_verified_kind0(&json, author)
                        .expect_err("signed metadata with forbidden embedded control")
                        .code(),
                    SafeErrorCode::InvalidProfileMetadata
                );
            }
        }
    }

    #[test]
    fn profile_event_verifies_signature_author_kind_and_metadata() {
        let (keys, json) = signed_profile();
        let expected_author =
            PublicKey::from_bytes(keys.public_key().to_bytes()).expect("valid public key");

        let candidate = parse_verified_kind0(&json, expected_author).expect("verified profile");

        assert_eq!(candidate.author(), expected_author);
        assert_eq!(candidate.metadata().name(), Some("farmer"));
        assert_eq!(candidate.metadata().display_name(), Some("Farm Identity"));
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
