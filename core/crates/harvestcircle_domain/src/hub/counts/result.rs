//! Original-result consistency, not a signature or current mutation authority.
use super::*;
use crate::hub::movement::{WorkProgress, WorkRequirement};
use crate::hub::reference::{ReferenceCommand, ReferenceMutation};

pub fn count_observation_result_digest(
    input: &[u8; 32],
    record: &CountObservation,
) -> Result<[u8; 32], CountError> {
    record.validate()?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count.observation.result.v1\0");
    hash.update(input);
    hash.update(record.id.as_bytes());
    hash.update(record.operation.as_bytes());
    hash.update(record.recorded_revision.value().to_be_bytes());
    let scope = &record.scope;
    hash.update(scope.hub.as_bytes());
    hash.update(scope.stock_revision.value().to_be_bytes());
    hash.update(scope.catalog_revision.value().to_be_bytes());
    hash.update(scope.trace.as_bytes());
    hash.update(scope.group.as_bytes());
    hash.update(scope.group_revision.value().to_be_bytes());
    for value in [&scope.product, &scope.variant, &scope.location] {
        reference(
            &mut hash,
            value,
            scope.hub,
            record.actor,
            scope.catalog_revision,
        );
    }
    hash.update(scope.recorded_packs.value().to_be_bytes());
    hash.update(scope.latest_physical_recorded_at.as_seconds().to_be_bytes());
    optional_id(&mut hash, scope.previous_reconciliation);
    hash.update(record.counted_packs.value().to_be_bytes());
    hash_occurrence(&mut hash, record.cutoff);
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash_text(&mut hash, record.observation.as_str());
    Ok(hash.finalize().into())
}

pub fn count_post_result_digest(
    input: &[u8; 32],
    record: &CountPostPlan,
) -> Result<[u8; 32], CountError> {
    record.validate()?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count.post.result.v1\0");
    hash.update(input);
    hash.update(record.id.as_bytes());
    hash.update(record.operation.as_bytes());
    let scope = &record.count.scope;
    let observe = CountObserveCommand {
        expected_stock_revision: scope.stock_revision,
        expected_catalog_revision: scope.catalog_revision,
        group: scope.group,
        expected_group_revision: scope.group_revision,
        counted_packs: record.count.counted_packs,
        cutoff: record.count.cutoff,
        observation: record.count.observation.clone(),
    }
    .fingerprint(scope.hub, record.count.actor, record.count.operation);
    hash.update(count_observation_result_digest(&observe, &record.count)?);
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    for value in [
        &record.references.product,
        &record.references.variant,
        &record.references.location,
    ] {
        reference(
            &mut hash,
            value,
            scope.hub,
            record.actor,
            record.catalog_revision,
        );
    }
    hash.update(record.actor.as_bytes());
    hash.update(record.recorded_at.as_seconds().to_be_bytes());
    hash.update(record.confirmed_on_hand_at.as_seconds().to_be_bytes());
    hash_text(&mut hash, record.reason.as_str());
    let before = &record.before;
    for id in [before.id, before.trace, before.variant, before.location] {
        hash.update(id.as_bytes());
    }
    optional_id(&mut hash, before.parent);
    hash.update([u8::from(before.identification.is_some())]);
    if let Some(value) = &before.identification {
        hash_text(&mut hash, value.label.as_str());
        hash_text(&mut hash, value.observation.as_str());
    }
    hash.update(before.packs.value().to_be_bytes());
    hash.update(before.revision.value().to_be_bytes());
    hash.update([u8::from(before.causal_floor.is_some())]);
    if let Some(at) = before.causal_floor {
        hash.update(at.as_seconds().to_be_bytes());
    }
    hash.update(
        u32::try_from(before.work.len())
            .map_err(|_| CountError::InvalidHistory)?
            .to_be_bytes(),
    );
    for allocation in &before.work {
        hash.update([match allocation.progress {
            WorkProgress::Pending => 1,
            WorkProgress::Completed => 2,
            WorkProgress::NoLongerRequired => 3,
        }]);
        work(&mut hash, record, &allocation.requirement);
    }
    match &record.effect {
        CountEffect::NoVariance => hash.update([0]),
        CountEffect::Shortage {
            packs,
            after_packs,
            after_revision,
            work_changes,
        } => {
            hash.update([1]);
            hash.update(packs.value().to_be_bytes());
            hash.update(after_packs.value().to_be_bytes());
            hash.update(after_revision.value().to_be_bytes());
            hash.update(
                u32::try_from(work_changes.len())
                    .map_err(|_| CountError::InvalidHistory)?
                    .to_be_bytes(),
            );
            for change in work_changes {
                work(&mut hash, record, &change.before);
                work(&mut hash, record, &change.after);
                hash.update(change.retired.value().to_be_bytes());
            }
        }
        CountEffect::Surplus {
            packs,
            identification,
            ..
        } => {
            // Validation requires unknown source and review-required acceptance.
            hash.update([2]);
            hash.update(packs.value().to_be_bytes());
            hash_text(&mut hash, identification.label.as_str());
            hash_text(&mut hash, identification.observation.as_str());
        }
    }
    Ok(hash.finalize().into())
}

fn reference(
    hash: &mut Sha256,
    value: &ReferenceRecord,
    hub: HubId,
    actor: PublicKey,
    catalog: HubRevision,
) {
    hash.update(value.revision.value().to_be_bytes());
    hash.update(
        ReferenceCommand {
            expected_catalog_revision: catalog,
            mutation: ReferenceMutation::Create {
                id: value.id,
                value: value.value.clone(),
            },
        }
        .fingerprint(hub, actor),
    );
}
fn optional_id(hash: &mut Sha256, id: Option<HubRecordId>) {
    hash.update([u8::from(id.is_some())]);
    if let Some(id) = id {
        hash.update(id.as_bytes());
    }
}
fn work(hash: &mut Sha256, record: &CountPostPlan, value: &WorkRequirement) {
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
        reference(
            hash,
            target,
            record.count.scope.hub,
            record.actor,
            record.catalog_revision,
        );
    }
}
