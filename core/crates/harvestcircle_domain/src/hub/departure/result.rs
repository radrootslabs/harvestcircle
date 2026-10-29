//! Canonical original-result consistency, never a signature or future permission.
use super::*;
use crate::hub::dispatch::dispatch_result_digest;
use crate::hub::movement::{WorkAllocation, hash_occurrence, hash_text};
use crate::hub::reference::{ReferenceCommand, ReferenceMutation};

pub fn departure_result_digest(
    input: &[u8; 32],
    record: &DepartureRecord,
) -> Result<[u8; 32], DepartureError> {
    record.validate()?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.departure.result.v1\0");
    hash.update(input);
    for id in [record.id, record.operation] {
        hash.update(id.as_bytes());
    }
    hash.update(record.hub.as_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash_occurrence(&mut hash, record.occurred);
    hash_text(&mut hash, record.observation.as_str());
    // Reuse the frozen paper v1 serialization rather than inventing a second
    // representation for historical recipient, reference and source content.
    hash.update(dispatch_result_digest(input, &record.paper)?);
    count(&mut hash, record.consumed.len());
    for effect in &record.consumed {
        let group = &effect.before;
        for id in [group.id, group.trace, group.variant, group.location] {
            hash.update(id.as_bytes());
        }
        optional_id(&mut hash, group.parent);
        hash.update([u8::from(group.identification.is_some())]);
        if let Some(identity) = &group.identification {
            hash_text(&mut hash, identity.label.as_str());
            hash_text(&mut hash, identity.observation.as_str());
        }
        hash.update(group.packs.value().to_be_bytes());
        hash.update(group.revision.value().to_be_bytes());
        clock(&mut hash, group.causal_floor);
        count(&mut hash, group.work.len());
        for allocation in &group.work {
            allocation_digest(&mut hash, record, allocation);
        }
        hash.update(effect.after_revision.value().to_be_bytes());
        clock(&mut hash, effect.causal_floor);
    }
    count(&mut hash, record.work_changes.len());
    for change in &record.work_changes {
        work(&mut hash, record, &change.before);
        work(&mut hash, record, &change.after);
        hash.update(change.retired.value().to_be_bytes());
    }
    Ok(hash.finalize().into())
}
fn count(hash: &mut Sha256, n: usize) {
    hash.update(
        u32::try_from(n)
            .expect("bounded departure data")
            .to_be_bytes(),
    );
}
fn optional_id(hash: &mut Sha256, value: Option<HubRecordId>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(id) = value {
        hash.update(id.as_bytes());
    }
}
fn clock(hash: &mut Sha256, value: Option<UnixTimestamp>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(time) = value {
        hash.update(time.as_seconds().to_be_bytes());
    }
}
fn allocation_digest(hash: &mut Sha256, record: &DepartureRecord, value: &WorkAllocation) {
    work(hash, record, &value.requirement);
    hash.update([match value.progress {
        WorkProgress::Pending => 1,
        WorkProgress::Completed => 2,
        WorkProgress::NoLongerRequired => 3,
    }]);
}
fn work(hash: &mut Sha256, record: &DepartureRecord, value: &WorkRequirement) {
    for id in [value.id, value.trace, value.variant] {
        hash.update(id.as_bytes());
    }
    hash.update(value.revision.value().to_be_bytes());
    for packs in [
        value.quantity.required(),
        value.quantity.completed(),
        value.quantity.no_longer_required(),
        value.quantity.remaining(),
    ] {
        hash.update(packs.value().to_be_bytes());
    }
    hash.update([u8::from(value.target.is_some())]);
    if let Some(target) = &value.target {
        hash.update(target.revision.value().to_be_bytes());
        hash.update(
            ReferenceCommand {
                expected_catalog_revision: record.catalog_revision,
                mutation: ReferenceMutation::Create {
                    id: target.id,
                    value: target.value.clone(),
                },
            }
            .fingerprint(record.hub, record.actor),
        );
    }
}
