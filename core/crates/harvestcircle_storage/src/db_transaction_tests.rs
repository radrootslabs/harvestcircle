use super::*;

#[test]
fn unconfirmed_rollback_never_becomes_a_retained_application_rejection() {
    for phase in [
        ServiceSqliteTransactionErrorKind::RollbackFailed,
        ServiceSqliteTransactionErrorKind::CommitOutcomeUnknown,
    ] {
        for operation in [
            None,
            Some(rejection(SafeErrorCode::HubConflict)),
            Some(rejection(SafeErrorCode::HubInvalidInput)),
        ] {
            assert_eq!(
                map_transaction_phase(phase, operation).code(),
                SafeErrorCode::PendingOperationRecoveryRequired
            );
        }
    }
}

#[test]
fn confirmed_no_commit_preserves_the_original_rejection() {
    for phase in [
        ServiceSqliteTransactionErrorKind::NotCommitted,
        ServiceSqliteTransactionErrorKind::OperationRolledBack,
    ] {
        let rejection = rejection(SafeErrorCode::HubConflict);
        assert_eq!(map_transaction_phase(phase, Some(rejection)), rejection);
        assert_eq!(
            map_transaction_phase(phase, None).code(),
            SafeErrorCode::StorageUnavailable
        );
    }
}

fn rejection(code: SafeErrorCode) -> SafeError {
    SafeError::new(code, SafeMessage::new("fixture"))
}
