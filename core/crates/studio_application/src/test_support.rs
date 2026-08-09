use std::sync::atomic::{AtomicU8, Ordering};

use radroots_studio_domain::{
    Npub, Nsec, PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput,
};

use crate::{GeneratedKeyMaterial, ImportedKeyMaterial, KeyMaterialProvider};

#[derive(Default)]
pub(crate) struct TestKeyMaterialProvider {
    next: AtomicU8,
}

impl KeyMaterialProvider for TestKeyMaterialProvider {
    fn generate(&self) -> Result<GeneratedKeyMaterial, SafeError> {
        let public_key = (0..=u8::MAX)
            .find_map(|_| {
                let candidate = self.next.fetch_add(1, Ordering::Relaxed).wrapping_add(9);
                PublicKey::from_bytes([candidate; 32]).ok()
            })
            .ok_or_else(invalid_secret_key)?;
        let secret_byte = public_key.as_bytes()[0];
        Ok(GeneratedKeyMaterial::new(
            public_key,
            Npub::derive(public_key)?,
            SecretKeyInput::parse(format!("{secret_byte:02x}").repeat(32))?,
            Nsec::from_encoded(
                "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5".to_owned(),
            )?,
        ))
    }

    fn import(&self, input: SecretKeyInput) -> Result<ImportedKeyMaterial, SafeError> {
        let discriminator = input.with_exposed_secret(|value| value.as_bytes()[0]);
        if input.with_exposed_secret(|value| value.starts_with("nsec1qq")) {
            return Err(invalid_secret_key());
        }
        let public_key = valid_test_public_key(discriminator)?;
        Ok(ImportedKeyMaterial::new(
            public_key,
            Npub::derive(public_key)?,
            input,
        ))
    }
}

pub(crate) fn valid_test_public_key(discriminator: u8) -> Result<PublicKey, SafeError> {
    (0..=u8::MAX)
        .find_map(|offset| PublicKey::from_bytes([discriminator.wrapping_add(offset); 32]).ok())
        .ok_or_else(invalid_secret_key)
}

const fn invalid_secret_key() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidSecretKey,
        SafeMessage::new("The Nostr secret key is invalid."),
    )
}
