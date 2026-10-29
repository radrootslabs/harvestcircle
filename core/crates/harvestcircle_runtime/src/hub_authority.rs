//! Current owner and credential proof shared by native hub mutations.
use super::*;
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_storage::HubMutationResult;

pub(super) async fn require_owner_credential(
    adapter: &PersistentAppCore,
    secrets: &dyn SecretStore,
    actor: PublicKey,
) -> Result<(), SafeError> {
    // Reject another owner before touching its credential. SQL rechecks ownership.
    adapter
        .database()
        .load_hub()
        .await?
        .ok_or_else(denied)?
        .authorize(Some(actor))
        .map_err(|_| denied())?;
    // The actor serializes session transitions until this bounded work completes.
    let material = adapter.key_material().import(secrets.load(actor).await?)?;
    let (actual, _, secret) = material.into_parts();
    drop(secret);
    if actual != actor {
        return Err(denied());
    }
    Ok(())
}

/// This is the only native hub mutation entry: credential proof is never restored
/// from submission bytes, and SQL still verifies live scope and current ownership.
pub(super) async fn submit(
    adapter: &PersistentAppCore,
    secrets: &dyn SecretStore,
    clock: &dyn Clock,
    scope: HubSubmissionScope,
    operation: DurableRequestId,
    mutation: HubMutation,
) -> Result<HubMutationResult, SafeError> {
    if matches!(&mutation, HubMutation::Initialize(_)) {
        if let Some(hub) = adapter.database().load_hub().await? {
            hub.authorize(Some(scope.actor)).map_err(|_| denied())?;
        }
        let material = adapter
            .key_material()
            .import(secrets.load(scope.actor).await?)?;
        let (actual, _, secret) = material.into_parts();
        drop(secret);
        if actual != scope.actor {
            return Err(denied());
        }
    } else {
        require_owner_credential(adapter, secrets, scope.actor).await?;
    }
    adapter
        .database()
        .submit_hub_mutation(scope, operation, mutation, clock.now())
        .await
}

pub(super) const fn denied() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubPermissionDenied,
        SafeMessage::new("Activate the current hub owner's local identity to access hub records."),
    )
}

pub(super) const fn mutation_error(error: SafeError) -> SafeError {
    match error.code() {
        SafeErrorCode::StorageUnavailable
        | SafeErrorCode::InvalidApplicationState
        | SafeErrorCode::PendingOperationRecoveryRequired => outcome_unknown(),
        _ => error,
    }
}

pub(super) const fn outcome_unknown() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubOutcomeUnknown,
        SafeMessage::new(
            "The hub operation outcome is unconfirmed. Check its original operation ID before another submission.",
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfirmed_storage_failures_preserve_hub_operation_recovery() {
        for code in [
            SafeErrorCode::StorageUnavailable,
            SafeErrorCode::InvalidApplicationState,
            SafeErrorCode::PendingOperationRecoveryRequired,
        ] {
            let error = SafeError::new(code, SafeMessage::new("fixture"));
            assert_eq!(
                mutation_error(error).code(),
                SafeErrorCode::HubOutcomeUnknown
            );
        }
        for code in [
            SafeErrorCode::HubConflict,
            SafeErrorCode::HubInvalidInput,
            SafeErrorCode::HubCapacity,
            SafeErrorCode::HubPermissionDenied,
            SafeErrorCode::CredentialMissing,
            SafeErrorCode::KeyringUnavailable,
            SafeErrorCode::StorageCorrupt,
        ] {
            let error = SafeError::new(code, SafeMessage::new("fixture"));
            assert_eq!(mutation_error(error), error);
        }
    }
}
