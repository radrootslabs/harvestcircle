use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafeErrorCode {
    InvalidPublicKey,
    InvalidSecretKey,
    InvalidIdentityMetadata,
    InvalidProfileMetadata,
    InvalidApplicationState,
    IdentityAlreadyExists,
    IdentityNotFound,
    KeyringUnavailable,
    CredentialMissing,
    StorageUnavailable,
    StorageCorrupt,
    StorageQuarantined,
    StorageBackupInvalid,
    UnsupportedSchemaVersion,
    RepairUnauthorized,
    PendingOperationRecoveryRequired,
    InvalidRelayConfiguration,
    RelayConnectionFailed,
    ProfileRefreshFailed,
    ObserverRegistrationFailed,
    NativeLibraryLoadFailed,
    AvailabilityInvalidInput,
    AvailabilityUnsupportedProfile,
    AvailabilityScopeMismatch,
    AvailabilityStaleQuery,
    AvailabilityCapacity,
    AvailabilityUnavailable,
}

/// Finite availability failures with fixed public diagnostic messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityFailure {
    InvalidInput,
    UnsupportedProfile,
    ScopeMismatch,
    StaleQuery,
    Capacity,
    Unavailable,
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

impl From<AvailabilityFailure> for SafeError {
    fn from(failure: AvailabilityFailure) -> Self {
        let (code, message) = match failure {
            AvailabilityFailure::InvalidInput => (
                SafeErrorCode::AvailabilityInvalidInput,
                "The availability request is invalid.",
            ),
            AvailabilityFailure::UnsupportedProfile => (
                SafeErrorCode::AvailabilityUnsupportedProfile,
                "This availability profile is unsupported.",
            ),
            AvailabilityFailure::ScopeMismatch => (
                SafeErrorCode::AvailabilityScopeMismatch,
                "The availability request belongs to another scope.",
            ),
            AvailabilityFailure::StaleQuery => (
                SafeErrorCode::AvailabilityStaleQuery,
                "The availability query is stale.",
            ),
            AvailabilityFailure::Capacity => (
                SafeErrorCode::AvailabilityCapacity,
                "The availability operation reached its capacity.",
            ),
            AvailabilityFailure::Unavailable => (
                SafeErrorCode::AvailabilityUnavailable,
                "Availability discovery is unavailable.",
            ),
        };
        Self::new(code, SafeMessage::new(message))
    }
}

impl From<crate::AvailabilityQueryError> for SafeError {
    fn from(error: crate::AvailabilityQueryError) -> Self {
        use crate::AvailabilityQueryError;

        let failure = match error {
            AvailabilityQueryError::InvalidInput | AvailabilityQueryError::InputTooLarge => {
                AvailabilityFailure::InvalidInput
            }
            AvailabilityQueryError::ScopeMismatch => AvailabilityFailure::ScopeMismatch,
            AvailabilityQueryError::StaleQuery => AvailabilityFailure::StaleQuery,
            AvailabilityQueryError::Capacity => AvailabilityFailure::Capacity,
        };
        failure.into()
    }
}

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

    #[test]
    fn availability_failures_and_query_errors_are_static() {
        use super::AvailabilityFailure;
        use crate::AvailabilityQueryError;

        for (failure, code, message) in [
            (
                AvailabilityFailure::InvalidInput,
                SafeErrorCode::AvailabilityInvalidInput,
                "The availability request is invalid.",
            ),
            (
                AvailabilityFailure::UnsupportedProfile,
                SafeErrorCode::AvailabilityUnsupportedProfile,
                "This availability profile is unsupported.",
            ),
            (
                AvailabilityFailure::ScopeMismatch,
                SafeErrorCode::AvailabilityScopeMismatch,
                "The availability request belongs to another scope.",
            ),
            (
                AvailabilityFailure::StaleQuery,
                SafeErrorCode::AvailabilityStaleQuery,
                "The availability query is stale.",
            ),
            (
                AvailabilityFailure::Capacity,
                SafeErrorCode::AvailabilityCapacity,
                "The availability operation reached its capacity.",
            ),
            (
                AvailabilityFailure::Unavailable,
                SafeErrorCode::AvailabilityUnavailable,
                "Availability discovery is unavailable.",
            ),
        ] {
            let error = SafeError::from(failure);
            assert_eq!(error.code(), code);
            assert_eq!(error.message().as_str(), message);
            assert_eq!(error.to_string(), message);
            assert!(!format!("{error:?}").contains("HCAV_PRIVATE_REMOTE_PAYLOAD"));
        }
        for (query, code) in [
            (
                AvailabilityQueryError::InvalidInput,
                SafeErrorCode::AvailabilityInvalidInput,
            ),
            (
                AvailabilityQueryError::InputTooLarge,
                SafeErrorCode::AvailabilityInvalidInput,
            ),
            (
                AvailabilityQueryError::ScopeMismatch,
                SafeErrorCode::AvailabilityScopeMismatch,
            ),
            (
                AvailabilityQueryError::StaleQuery,
                SafeErrorCode::AvailabilityStaleQuery,
            ),
            (
                AvailabilityQueryError::Capacity,
                SafeErrorCode::AvailabilityCapacity,
            ),
        ] {
            assert_eq!(SafeError::from(query).code(), code);
        }
    }
}
