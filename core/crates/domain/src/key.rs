//! Validated Nostr public and secret-key boundary values.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use secrecy::{ExposeSecret, SecretString};

use crate::{SafeError, SafeErrorCode, SafeMessage};

pub const PUBLIC_KEY_BYTE_LENGTH: usize = 32;
pub const PUBLIC_KEY_HEX_LENGTH: usize = PUBLIC_KEY_BYTE_LENGTH * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretKeyInputKind {
    Nsec,
    Hex,
}

pub struct SecretKeyInput {
    value: SecretString,
    kind: SecretKeyInputKind,
}

impl SecretKeyInput {
    /// Moves one secret input string into a zeroizing boundary.
    ///
    /// Nsec inputs receive complete NIP-19 validation in the Nostr adapter.
    /// Hex input is structurally validated here to prevent ambiguous fallback.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-secret-key error when the input is neither an
    /// nsec-looking value nor exactly 64 lowercase hexadecimal characters.
    pub fn parse(value: String) -> Result<Self, SafeError> {
        let kind = if value.len() == PUBLIC_KEY_HEX_LENGTH
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            SecretKeyInputKind::Hex
        } else if value.starts_with("nsec1") && value.len() > "nsec1".len() {
            SecretKeyInputKind::Nsec
        } else {
            return Err(invalid_secret_key());
        };

        Ok(Self {
            value: SecretString::from(value),
            kind,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> SecretKeyInputKind {
        self.kind
    }

    pub fn with_exposed_secret<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        operation(self.value.expose_secret())
    }
}

impl fmt::Debug for SecretKeyInput {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretKeyInput")
            .field("value", &"[REDACTED]")
            .field("kind", &self.kind)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicKey([u8; PUBLIC_KEY_BYTE_LENGTH]);

impl PublicKey {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTE_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Parses a canonical lowercase hexadecimal Nostr public key.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-public-key error when the value is not exactly
    /// 64 lowercase hexadecimal characters.
    pub fn from_hex(value: &str) -> Result<Self, SafeError> {
        if value.len() != PUBLIC_KEY_HEX_LENGTH
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_public_key());
        }

        let mut bytes = [0_u8; PUBLIC_KEY_BYTE_LENGTH];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex_digit(pair[0]).ok_or_else(invalid_public_key)?;
            let low = decode_hex_digit(pair[1]).ok_or_else(invalid_public_key)?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTE_LENGTH] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(PUBLIC_KEY_HEX_LENGTH);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }

    #[must_use]
    pub fn short_hex(self) -> String {
        let hex = self.to_hex();
        format!("{}…{}", &hex[..8], &hex[PUBLIC_KEY_HEX_LENGTH - 8..])
    }
}

impl Display for PublicKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl From<[u8; PUBLIC_KEY_BYTE_LENGTH]> for PublicKey {
    fn from(bytes: [u8; PUBLIC_KEY_BYTE_LENGTH]) -> Self {
        Self::from_bytes(bytes)
    }
}

impl FromStr for PublicKey {
    type Err = SafeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_hex(value)
    }
}

const fn invalid_public_key() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidPublicKey,
        SafeMessage::new("The Nostr public key is invalid."),
    )
}

const fn invalid_secret_key() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidSecretKey,
        SafeMessage::new("The Nostr secret key is invalid."),
    )
}

const fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{PUBLIC_KEY_BYTE_LENGTH, PublicKey, SecretKeyInput, SecretKeyInputKind};
    use crate::SafeErrorCode;

    const HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

    #[test]
    fn public_key_round_trips_canonical_hex_and_bytes() {
        let key = PublicKey::from_str(HEX).expect("valid public key");

        assert_eq!(key.to_hex(), HEX);
        assert_eq!(key.to_string(), HEX);
        assert_eq!(key.short_hex(), "7e7e9c42…2107f6d7");
        assert_eq!(PublicKey::from_bytes(*key.as_bytes()), key);
        assert_eq!(key.as_bytes().len(), PUBLIC_KEY_BYTE_LENGTH);
    }

    #[test]
    fn public_key_rejects_noncanonical_or_malformed_hex() {
        for value in [
            "",
            "00",
            "7E7E9C42A91BFEF19FA7EA99D52D8AFDB67D893A8FEFBA1F5CB9793F2107F6D7",
            "ze7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
            " 7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
        ] {
            let error = PublicKey::from_hex(value).expect_err("invalid public key");
            assert_eq!(error.code(), SafeErrorCode::InvalidPublicKey);
        }
    }

    #[test]
    fn public_keys_are_ordered_by_canonical_bytes() {
        let low = PublicKey::from_bytes([0_u8; PUBLIC_KEY_BYTE_LENGTH]);
        let high = PublicKey::from_bytes([1_u8; PUBLIC_KEY_BYTE_LENGTH]);

        assert!(low < high);
    }

    #[test]
    fn secret_input_is_redacted_and_exposed_only_to_a_scoped_operation() {
        let secret = "11".repeat(PUBLIC_KEY_BYTE_LENGTH);
        let input = SecretKeyInput::parse(secret.clone()).expect("valid secret hex");

        assert_eq!(input.kind(), SecretKeyInputKind::Hex);
        assert_eq!(input.with_exposed_secret(str::len), secret.len());
        assert_eq!(
            format!("{input:?}"),
            "SecretKeyInput { value: \"[REDACTED]\", kind: Hex }"
        );
        assert!(!format!("{input:?}").contains(&secret));
    }

    #[test]
    fn secret_input_accepts_nsec_shape_without_exposing_it() {
        let secret = "nsec1known-test-secret".to_owned();
        let input = SecretKeyInput::parse(secret.clone()).expect("nsec-shaped input");

        assert_eq!(input.kind(), SecretKeyInputKind::Nsec);
        assert!(!format!("{input:?}").contains(&secret));
    }

    #[test]
    fn secret_input_rejects_invalid_hex_and_arbitrary_text() {
        for value in [
            "",
            "very-sensitive-input",
            &"GG".repeat(PUBLIC_KEY_BYTE_LENGTH),
        ] {
            let error = SecretKeyInput::parse(value.to_owned()).expect_err("invalid secret");
            assert_eq!(error.code(), SafeErrorCode::InvalidSecretKey);
            if !value.is_empty() {
                assert!(!format!("{error:?}").contains(value));
            }
        }
    }
}
