use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InstallationIdentity(String);

impl InstallationIdentity {
    pub fn parse(value: impl Into<String>) -> Result<Self, SafeError> {
        let value = value.into();
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_installation_identity());
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub trait InstallationIdentitySource: Send + Sync {
    fn generate(&self) -> Result<InstallationIdentity, SafeError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UuidInstallationIdentitySource;

impl InstallationIdentitySource for UuidInstallationIdentitySource {
    fn generate(&self) -> Result<InstallationIdentity, SafeError> {
        InstallationIdentity::parse(uuid::Uuid::new_v4().simple().to_string())
    }
}

const fn invalid_installation_identity() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The installation identity is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use super::{InstallationIdentity, InstallationIdentitySource, UuidInstallationIdentitySource};

    #[test]
    fn installation_identity_is_fixed_width_lowercase_hex() {
        let identity = UuidInstallationIdentitySource.generate().expect("identity");
        assert_eq!(identity.as_str().len(), 32);
        assert!(InstallationIdentity::parse(identity.as_str()).is_ok());
        for denied in ["", "AAaabbccddeeff001122334455667788", "not-an-identity"] {
            assert!(InstallationIdentity::parse(denied).is_err());
        }
    }
}
