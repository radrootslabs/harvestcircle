//! Current owner and credential proof shared by native hub mutations.
use super::*;

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

pub(super) const fn denied() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubPermissionDenied,
        SafeMessage::new("Activate the current hub owner's local identity to access hub records."),
    )
}
