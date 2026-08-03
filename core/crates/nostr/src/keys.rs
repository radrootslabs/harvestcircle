use nostr::{Keys, ToBech32};
use radroots_studio_domain::{
    Npub, Nsec, PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput,
};

pub struct GeneratedKeyMaterial {
    public_key: PublicKey,
    npub: Npub,
    secret: SecretKeyInput,
    nsec: Nsec,
}

impl GeneratedKeyMaterial {
    #[must_use]
    pub fn into_parts(self) -> (PublicKey, Npub, SecretKeyInput, Nsec) {
        (self.public_key, self.npub, self.secret, self.nsec)
    }
}

pub struct ImportedKeyMaterial {
    public_key: PublicKey,
    npub: Npub,
    secret: SecretKeyInput,
}

impl ImportedKeyMaterial {
    #[must_use]
    pub fn into_parts(self) -> (PublicKey, Npub, SecretKeyInput) {
        (self.public_key, self.npub, self.secret)
    }
}

/// Generates one cryptographically random local Nostr keypair.
///
/// # Errors
///
/// Returns a safe key error if an upstream encoding cannot be represented by
/// the stricter Radroots domain boundary.
pub fn generate_local_keypair() -> Result<GeneratedKeyMaterial, SafeError> {
    let keys = Keys::generate();
    let (public_key, npub, secret, nsec) = encode_keys(&keys)?;
    Ok(GeneratedKeyMaterial {
        public_key,
        npub,
        secret,
        nsec,
    })
}

/// Parses nsec or canonical secret hex and derives public Nostr identity.
///
/// # Errors
///
/// Returns a safe invalid-secret-key error for checksum, scalar, or encoding
/// failures without exposing the rejected input.
pub fn import_secret(input: SecretKeyInput) -> Result<ImportedKeyMaterial, SafeError> {
    let keys = input
        .with_exposed_secret(Keys::parse)
        .map_err(|_| invalid_secret_key())?;
    drop(input);
    let public_key = PublicKey::from_bytes(keys.public_key().to_bytes());
    let npub = keys
        .public_key()
        .to_bech32()
        .map_err(|_| invalid_public_key())
        .and_then(Npub::from_encoded)?;
    let secret = SecretKeyInput::parse(keys.secret_key().to_secret_hex())?;
    Ok(ImportedKeyMaterial {
        public_key,
        npub,
        secret,
    })
}

fn encode_keys(keys: &Keys) -> Result<(PublicKey, Npub, SecretKeyInput, Nsec), SafeError> {
    let public_key = PublicKey::from_bytes(keys.public_key().to_bytes());
    let npub = keys
        .public_key()
        .to_bech32()
        .map_err(|_| invalid_public_key())
        .and_then(Npub::from_encoded)?;
    let secret = SecretKeyInput::parse(keys.secret_key().to_secret_hex())?;
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|_| invalid_secret_key())
        .and_then(Nsec::from_encoded)?;
    Ok((public_key, npub, secret, nsec))
}

const fn invalid_secret_key() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidSecretKey,
        SafeMessage::new("The Nostr secret key is invalid."),
    )
}

const fn invalid_public_key() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidPublicKey,
        SafeMessage::new("The Nostr public key is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_domain::{SafeErrorCode, SecretKeyInput};

    use super::{generate_local_keypair, import_secret};

    const SECRET_HEX: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
    const NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NSEC_PUBLIC_HEX: &str =
        "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";
    const HEX_PUBLIC_HEX: &str = "0cfda0afa91cc2fbbd6050c285802fe95c7a1755e0f68323999e13760501dc40";

    #[test]
    fn keys_generate_valid_redacted_material() {
        let generated = generate_local_keypair().expect("generated");
        let (public_key, npub, secret, nsec) = generated.into_parts();
        assert_eq!(public_key.to_hex().len(), 64);
        assert!(npub.as_str().starts_with("npub1"));
        assert_eq!(secret.with_exposed_secret(str::len), 64);
        assert_eq!(nsec.with_exposed_secret(str::len), 63);
        assert_eq!(secret.with_exposed_secret(str::len), 64);
        assert_eq!(nsec.with_exposed_secret(str::len), 63);
    }

    #[test]
    fn keys_import_known_nsec_and_hex_vectors() {
        let from_nsec = import_secret(SecretKeyInput::parse(NSEC.to_owned()).expect("nsec"))
            .expect("import nsec");
        let from_hex = import_secret(SecretKeyInput::parse(SECRET_HEX.to_owned()).expect("hex"))
            .expect("import hex");
        let (nsec_public, nsec_npub, _) = from_nsec.into_parts();
        let (hex_public, hex_npub, _) = from_hex.into_parts();
        assert_eq!(nsec_public.to_hex(), NSEC_PUBLIC_HEX);
        assert_eq!(hex_public.to_hex(), HEX_PUBLIC_HEX);
        assert!(nsec_npub.as_str().starts_with("npub1"));
        assert!(hex_npub.as_str().starts_with("npub1"));
    }

    #[test]
    fn keys_reject_structurally_plausible_nsec_with_invalid_checksum() {
        let input = SecretKeyInput::parse(
            "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_owned(),
        )
        .expect("domain shape");
        let error = import_secret(input).err().expect("invalid checksum");
        assert_eq!(error.code(), SafeErrorCode::InvalidSecretKey);
    }
}
