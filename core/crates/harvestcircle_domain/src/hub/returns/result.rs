//! A seal of original captured facts; never permission to execute archived input.
use super::*;
use crate::hub::{custody_result, departure::departure_result_digest};

/// The context must be the complete immutable original capture when reading an
/// original, or fresh protected native data when planning a new effect.
pub fn return_result_digest(
    input: &[u8; 32],
    command: &ReturnCommand,
    context: &ReturnContext<'_>,
    record: &ReturnPlan,
) -> Result<[u8; 32], ReturnError> {
    if command.fingerprint(context.hub, record.intake.actor, record.identities) != *input
        || command.plan(
            context,
            record.identities,
            record.intake.actor,
            record.intake.recorded_at,
        )? != *record
    {
        return Err(ReturnError::InvalidNativeContext);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.result.v1\0");
    hash.update(input);
    hash.update(context.hub.as_bytes());
    hash.update(record.intake.actor.as_bytes());
    hash.update(record.intake.recorded_at.as_seconds().to_be_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    // Replanning above checks every derived group, treatment, work and linkage
    // fact. Seal all captured inputs as well as current reference content, so
    // equal revision numbers cannot conceal a changed historical label/value.
    for value in [
        &record.intake.product,
        &record.intake.variant,
        &record.intake.location,
    ] {
        custody_result::reference(
            &mut hash,
            value,
            context.hub,
            record.intake.actor,
            record.catalog_revision,
        );
    }
    for value in [
        &record.intake.returned_by,
        &record.intake.source_issuer,
        &record.intake.planned_location,
    ] {
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            custody_result::reference(
                &mut hash,
                value,
                context.hub,
                record.intake.actor,
                record.catalog_revision,
            );
        }
    }
    hash.update([u8::from(context.departure.is_some())]);
    if let Some(departure) = context.departure {
        hash.update(departure_result_digest(&[0; 32], departure)?);
    }
    let mut totals = context.established_return_totals.to_vec();
    totals.sort_by_key(|total| total.original_group);
    hash.update(
        u32::try_from(totals.len())
            .map_err(|_| ReturnError::InvalidNativeContext)?
            .to_be_bytes(),
    );
    for total in totals {
        hash.update(total.original_group.as_bytes());
        hash.update(total.packs.value().to_be_bytes());
    }
    Ok(hash.finalize().into())
}
