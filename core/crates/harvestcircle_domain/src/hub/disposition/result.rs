//! Distinct immutable disposition facts, without departure or current authority.
use super::*;
use crate::hub::custody_result;

pub fn disposition_result_digest(
    input: &[u8; 32],
    record: &DispositionRecord,
) -> Result<[u8; 32], DispositionError> {
    record.validate()?;
    let group = &record.consumed.before;
    let command = DispositionCommand {
        expected_stock_revision: HubRevision::from_value(record.stock_revision.value() - 1)
            .map_err(|_| DispositionError::InvalidHistory)?,
        expected_catalog_revision: record.catalog_revision,
        group: group.id,
        expected_group_revision: group.revision,
        packs: group.packs,
        kind: record.kind,
        occurred: record.occurred,
        observation: record.observation.clone(),
    };
    if command.fingerprint(record.hub, record.actor, record.identities) != *input {
        return Err(DispositionError::InvalidHistory);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.disposition.result.v1\0");
    hash.update(input);
    hash.update(record.hub.as_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    for value in [&record.product, &record.variant, &record.location] {
        custody_result::reference(
            &mut hash,
            value,
            record.hub,
            record.actor,
            record.catalog_revision,
        );
    }
    custody_result::group(
        &mut hash,
        group,
        record.hub,
        record.actor,
        record.catalog_revision,
    );
    hash.update(record.consumed.after_revision.value().to_be_bytes());
    custody_result::clock(&mut hash, record.consumed.causal_floor);
    hash.update(
        u32::try_from(record.work_changes.len())
            .map_err(|_| DispositionError::InvalidHistory)?
            .to_be_bytes(),
    );
    for change in &record.work_changes {
        custody_result::work(
            &mut hash,
            &change.before,
            record.hub,
            record.actor,
            record.catalog_revision,
        );
        custody_result::work(
            &mut hash,
            &change.after,
            record.hub,
            record.actor,
            record.catalog_revision,
        );
        hash.update(change.retired.value().to_be_bytes());
    }
    Ok(hash.finalize().into())
}
