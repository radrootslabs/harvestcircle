//! Canonical facts for new custody results; existing issued-result bytes stay frozen.
use super::movement::{PhysicalStockGroup, WorkProgress, WorkRequirement, hash_text};
use super::reference::{ReferenceCommand, ReferenceMutation, ReferenceRecord};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

pub(super) fn reference(
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
pub(super) fn clock(hash: &mut Sha256, value: Option<UnixTimestamp>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        hash.update(value.as_seconds().to_be_bytes());
    }
}
pub(super) fn work(
    hash: &mut Sha256,
    value: &WorkRequirement,
    hub: HubId,
    actor: PublicKey,
    catalog: HubRevision,
) {
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
        reference(hash, target, hub, actor, catalog);
    }
}
pub(super) fn group(
    hash: &mut Sha256,
    value: &PhysicalStockGroup,
    hub: HubId,
    actor: PublicKey,
    catalog: HubRevision,
) {
    for id in [value.id, value.trace, value.variant, value.location] {
        hash.update(id.as_bytes());
    }
    hash.update([u8::from(value.parent.is_some())]);
    if let Some(id) = value.parent {
        hash.update(id.as_bytes());
    }
    hash.update([u8::from(value.identification.is_some())]);
    if let Some(id) = &value.identification {
        hash_text(hash, id.label.as_str());
        hash_text(hash, id.observation.as_str());
    }
    hash.update(value.packs.value().to_be_bytes());
    hash.update(value.revision.value().to_be_bytes());
    clock(hash, value.causal_floor);
    hash.update(
        u32::try_from(value.work.len())
            .expect("validated bounded group")
            .to_be_bytes(),
    );
    for allocation in &value.work {
        work(hash, &allocation.requirement, hub, actor, catalog);
        hash.update([match allocation.progress {
            WorkProgress::Pending => 1,
            WorkProgress::Completed => 2,
            WorkProgress::NoLongerRequired => 3,
        }]);
    }
}
