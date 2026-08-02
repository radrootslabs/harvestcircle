use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SafeErrorCode {
    InvalidPublicKey,
    InvalidSecretKey,
    InvalidAccountMetadata,
    InvalidProfileMetadata,
    InvalidApplicationState,
    AccountAlreadyExists,
    AccountNotFound,
    KeyringUnavailable,
    CredentialMissing,
    StorageUnavailable,
    StorageCorrupt,
    PendingOperationRecoveryRequired,
    InvalidRelayConfiguration,
    RelayConnectionFailed,
    ProfileRefreshFailed,
    ObserverRegistrationFailed,
    NativeLibraryLoadFailed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SafeMessage(&'static str);

impl SafeMessage {
    #[must_use]
    pub const fn new(message: &'static str) -> Self {
        Self(message)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl Debug for SafeMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("SafeMessage").field(&self.0).finish()
    }
}

impl Display for SafeMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SafeError {
    code: SafeErrorCode,
    message: SafeMessage,
}

impl SafeError {
    #[must_use]
    pub const fn new(code: SafeErrorCode, message: SafeMessage) -> Self {
        Self { code, message }
    }

    #[must_use]
    pub const fn code(self) -> SafeErrorCode {
        self.code
    }

    #[must_use]
    pub const fn message(self) -> SafeMessage {
        self.message
    }
}

impl Debug for SafeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeError")
            .field("code", &self.code)
            .field("message", &self.message)
            .finish()
    }
}

impl Display for SafeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.message, formatter)
    }
}

impl Error for SafeError {}

#[cfg(test)]
mod tests {
    use super::{SafeError, SafeErrorCode, SafeMessage};

    #[test]
    fn safe_error_formats_only_a_static_public_message() {
        let error = SafeError::new(
            SafeErrorCode::InvalidSecretKey,
            SafeMessage::new("The secret key is invalid."),
        );

        assert_eq!(error.to_string(), "The secret key is invalid.");
        assert_eq!(error.code(), SafeErrorCode::InvalidSecretKey);
        assert_eq!(error.message().as_str(), "The secret key is invalid.");
        assert!(!format!("{error:?}").contains("nsec1unsafe-test-value"));
    }
}
