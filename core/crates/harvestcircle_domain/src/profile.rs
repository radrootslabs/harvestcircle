//! Public Nostr profile metadata values.

use crate::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};

const EVENT_ID_BYTES: usize = 32;
const EVENT_ID_HEX: usize = EVENT_ID_BYTES * 2;
const MAX_NAME_UTF8_BYTES: usize = 128;
const MAX_NIP05_UTF8_BYTES: usize = 320;
const MAX_ABOUT_UTF8_BYTES: usize = 4_096;
const MAX_PICTURE_UTF8_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventId([u8; EVENT_ID_BYTES]);

impl EventId {
    /// Parses a canonical lowercase hexadecimal Nostr event ID.
    ///
    /// # Errors
    ///
    /// Returns a safe profile error for malformed input.
    pub fn from_hex(value: &str) -> Result<Self, SafeError> {
        if value.len() != EVENT_ID_HEX
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_profile_metadata());
        }

        let mut bytes = [0_u8; EVENT_ID_BYTES];
        for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let high = decode_hex(pair[0]).ok_or_else(invalid_profile_metadata)?;
            let low = decode_hex(pair[1]).ok_or_else(invalid_profile_metadata)?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; EVENT_ID_BYTES]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; EVENT_ID_BYTES] {
        self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(EVENT_ID_HEX);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileMetadata {
    name: Option<String>,
    display_name: Option<String>,
    nip05: Option<String>,
    about: Option<String>,
    picture: Option<String>,
}

impl ProfileMetadata {
    /// Normalizes and bounds public kind-0 profile fields.
    ///
    /// # Errors
    ///
    /// Returns a safe profile error when a field exceeds its limit or contains
    /// a forbidden control character.
    pub fn new(
        name: Option<String>,
        display_name: Option<String>,
        nip05: Option<String>,
        about: Option<String>,
        picture: Option<String>,
    ) -> Result<Self, SafeError> {
        Ok(Self {
            name: normalize_field(name, MAX_NAME_UTF8_BYTES, false)?,
            display_name: normalize_field(display_name, MAX_NAME_UTF8_BYTES, false)?,
            nip05: normalize_field(nip05, MAX_NIP05_UTF8_BYTES, false)?,
            about: normalize_field(about, MAX_ABOUT_UTF8_BYTES, true)?,
            picture: normalize_field(picture, MAX_PICTURE_UTF8_BYTES, false)?,
        })
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    #[must_use]
    pub fn nip05(&self) -> Option<&str> {
        self.nip05.as_deref()
    }

    #[must_use]
    pub fn about(&self) -> Option<&str> {
        self.about.as_deref()
    }

    #[must_use]
    pub fn picture(&self) -> Option<&str> {
        self.picture.as_deref()
    }

    #[must_use]
    pub fn preferred_name(&self) -> Option<&str> {
        self.display_name().or_else(|| self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kind0ProfileCandidate {
    event_id: EventId,
    author: PublicKey,
    created_at: UnixTimestamp,
    metadata: ProfileMetadata,
}

impl Kind0ProfileCandidate {
    #[must_use]
    pub const fn new(
        event_id: EventId,
        author: PublicKey,
        created_at: UnixTimestamp,
        metadata: ProfileMetadata,
    ) -> Self {
        Self {
            event_id,
            author,
            created_at,
            metadata,
        }
    }

    #[must_use]
    pub const fn event_id(&self) -> EventId {
        self.event_id
    }

    #[must_use]
    pub const fn author(&self) -> PublicKey {
        self.author
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixTimestamp {
        self.created_at
    }

    #[must_use]
    pub const fn metadata(&self) -> &ProfileMetadata {
        &self.metadata
    }
}

#[must_use]
pub fn select_latest_kind0(
    candidates: impl IntoIterator<Item = Kind0ProfileCandidate>,
) -> Option<Kind0ProfileCandidate> {
    candidates.into_iter().reduce(|selected, candidate| {
        if candidate.created_at > selected.created_at
            || (candidate.created_at == selected.created_at
                && candidate.event_id < selected.event_id)
        {
            candidate
        } else {
            selected
        }
    })
}

fn normalize_field(
    value: Option<String>,
    max_utf8_bytes: usize,
    allow_layout_controls: bool,
) -> Result<Option<String>, SafeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.trim();
    if normalized.is_empty() {
        return Ok(None);
    }
    if normalized.len() > max_utf8_bytes
        || normalized.chars().any(|character| {
            character.is_control()
                && !(allow_layout_controls && matches!(character, '\n' | '\r' | '\t'))
        })
    {
        return Err(invalid_profile_metadata());
    }
    Ok(Some(normalized.to_owned()))
}

const fn invalid_profile_metadata() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidProfileMetadata,
        SafeMessage::new("The Nostr profile metadata is invalid."),
    )
}

const fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{PublicKey, UnixTimestamp};

    use super::{EventId, Kind0ProfileCandidate, ProfileMetadata, select_latest_kind0};

    fn profile(name: &str) -> ProfileMetadata {
        ProfileMetadata::new(Some(name.to_owned()), None, None, None, None).expect("valid profile")
    }

    fn candidate(id_byte: u8, created_at: i64, name: &str) -> Kind0ProfileCandidate {
        Kind0ProfileCandidate::new(
            EventId::from_bytes([id_byte; 32]),
            PublicKey::from_bytes([7_u8; 32]).expect("valid public key"),
            UnixTimestamp::from_seconds(created_at).expect("valid timestamp"),
            profile(name),
        )
    }

    fn text_at_utf8_limit(unit: &str, maximum: usize) -> String {
        unit.repeat(maximum / unit.len()) + &"x".repeat(maximum % unit.len())
    }

    fn metadata_with_field(
        index: usize,
        value: String,
    ) -> Result<ProfileMetadata, crate::SafeError> {
        let mut fields: [Option<String>; 5] = std::array::from_fn(|_| None);
        fields[index] = Some(value);
        let [name, display_name, nip05, about, picture] = fields;
        ProfileMetadata::new(name, display_name, nip05, about, picture)
    }

    fn metadata_field(metadata: &ProfileMetadata, index: usize) -> Option<&str> {
        match index {
            0 => metadata.name(),
            1 => metadata.display_name(),
            2 => metadata.nip05(),
            3 => metadata.about(),
            4 => metadata.picture(),
            _ => unreachable!("test field index"),
        }
    }

    fn assert_one_byte_over_limit_is_rejected(index: usize, maximum: usize) {
        for unit in ["x", "é", "🥕", "e\u{301}"] {
            let value = text_at_utf8_limit(unit, maximum) + "x";
            assert_eq!(value.len(), maximum + 1);
            let error = metadata_with_field(index, format!(" {value} "))
                .expect_err("normalized UTF-8 field must fail before storage");
            assert_eq!(error.code(), crate::SafeErrorCode::InvalidProfileMetadata);
        }
    }

    #[test]
    fn profile_fields_accept_exact_utf8_boundaries_after_trimming() {
        for (index, maximum) in [128, 128, 320, 4_096, 2_048].into_iter().enumerate() {
            for unit in ["x", "é", "🥕", "e\u{301}"] {
                let value = text_at_utf8_limit(unit, maximum);
                assert_eq!(value.len(), maximum);
                let metadata = metadata_with_field(index, format!(" \u{2003}{value}\u{2003} "))
                    .expect("exact normalized UTF-8 boundary");
                assert_eq!(metadata_field(&metadata, index), Some(value.as_str()));
            }
        }
    }

    #[test]
    fn profile_name_rejects_one_byte_over_utf8_limit() {
        assert_one_byte_over_limit_is_rejected(0, 128);
    }

    #[test]
    fn profile_display_name_rejects_one_byte_over_utf8_limit() {
        assert_one_byte_over_limit_is_rejected(1, 128);
    }

    #[test]
    fn profile_nip05_rejects_one_byte_over_utf8_limit() {
        assert_one_byte_over_limit_is_rejected(2, 320);
    }

    #[test]
    fn profile_about_rejects_one_byte_over_utf8_limit() {
        assert_one_byte_over_limit_is_rejected(3, 4_096);
    }

    #[test]
    fn profile_picture_rejects_one_byte_over_utf8_limit() {
        assert_one_byte_over_limit_is_rejected(4, 2_048);
    }

    #[test]
    fn profile_fields_preserve_blank_and_about_layout_control_policy() {
        for index in 0..5 {
            for blank in ["", " \u{2003}\t\r\n "] {
                assert_eq!(
                    metadata_with_field(index, blank.to_owned()).expect("blank optional metadata"),
                    ProfileMetadata::default()
                );
            }
            for control in ['\0', '\u{1b}', '\u{7f}', '\u{85}'] {
                assert_eq!(
                    metadata_with_field(index, format!("a{control}b"))
                        .expect_err("forbidden embedded control")
                        .code(),
                    crate::SafeErrorCode::InvalidProfileMetadata
                );
            }
            for control in ['\n', '\r', '\t'] {
                let value = format!("a{control}b");
                let result = metadata_with_field(index, value.clone());
                if index == 3 {
                    assert_eq!(result.expect("about layout").about(), Some(value.as_str()));
                } else {
                    assert_eq!(
                        result.expect_err("layout outside about").code(),
                        crate::SafeErrorCode::InvalidProfileMetadata
                    );
                }
            }
        }
        let about = "First\nSecond\rThird\tFourth";
        assert_eq!(
            metadata_with_field(3, format!(" \n{about}\t "))
                .expect("normalized about layout")
                .about(),
            Some(about)
        );
    }

    #[test]
    fn profile_fields_are_trimmed_bounded_and_public() {
        let metadata = ProfileMetadata::new(
            Some("  farmer  ".to_owned()),
            Some("  Farm Identity  ".to_owned()),
            Some("farmer@example.test".to_owned()),
            Some("First line\nSecond line".to_owned()),
            Some("https://images.example.test/profile.png".to_owned()),
        )
        .expect("valid profile");

        assert_eq!(metadata.name(), Some("farmer"));
        assert_eq!(metadata.display_name(), Some("Farm Identity"));
        assert_eq!(metadata.preferred_name(), Some("Farm Identity"));
        assert_eq!(metadata.nip05(), Some("farmer@example.test"));
        assert_eq!(metadata.about(), Some("First line\nSecond line"));
        assert_eq!(
            metadata.picture(),
            Some("https://images.example.test/profile.png")
        );
    }

    #[test]
    fn profile_fields_reject_forbidden_controls_and_oversize_values() {
        assert!(
            ProfileMetadata::new(Some("bad\0name".to_owned()), None, None, None, None).is_err()
        );
        assert!(ProfileMetadata::new(Some("x".repeat(129)), None, None, None, None).is_err());
    }

    #[test]
    fn latest_kind0_uses_timestamp_then_lowest_event_id() {
        let older = candidate(0, 10, "older");
        let equal_high_id = candidate(9, 20, "high-id");
        let equal_low_id = candidate(1, 20, "low-id");

        let selected =
            select_latest_kind0([older, equal_high_id, equal_low_id]).expect("selected profile");

        assert_eq!(selected.metadata().name(), Some("low-id"));
        assert_eq!(selected.event_id().as_bytes(), [1_u8; 32]);
        assert_eq!(
            selected.author(),
            PublicKey::from_bytes([7_u8; 32]).expect("valid public key")
        );
        assert_eq!(selected.created_at().as_seconds(), 20);
    }

    #[test]
    fn event_id_rejects_noncanonical_hex_and_round_trips() {
        let hex = "12".repeat(32);
        let event_id = EventId::from_hex(&hex).expect("valid event id");

        assert_eq!(event_id.to_hex(), hex);
        assert!(EventId::from_hex(&"GG".repeat(32)).is_err());
    }
}
