//! Original-result integrity, not authentication or future treatment authority.
use super::*;
use crate::hub::movement::{hash_occurrence, hash_text};
use crate::hub::portions::CapturedPhysicalScope;
use crate::hub::reference::WholePacks;
use sha2::{Digest, Sha256};

#[must_use]
pub fn treatment_result_digest(
    input: &[u8; 32],
    change: &TreatmentChange,
    summary: TreatmentSummary,
    affected: WholePacks,
    capacity_reached: bool,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.treatment.result.v1\0");
    hash.update(input);
    for packs in [
        summary.on_hand,
        summary.accepted,
        summary.review_required,
        summary.rejected,
        summary.restricted,
        summary.departure_eligible,
        affected,
    ] {
        hash.update(packs.value().to_be_bytes());
    }
    hash.update([u8::from(capacity_reached)]);
    match change {
        TreatmentChange::Acceptance(value) => {
            hash.update([0]);
            stamp(&mut hash, &value.stamp);
            scope(&mut hash, &value.scope);
            hash.update([match value.status {
                AcceptanceStatus::ReviewRequired => 0,
                AcceptanceStatus::Accepted => 1,
                AcceptanceStatus::Rejected => 2,
            }]);
        }
        TreatmentChange::HoldPlaced(value) => {
            hash.update([1]);
            stamp(&mut hash, &value.stamp);
            scope(&mut hash, &value.scope);
        }
        TreatmentChange::HoldReleased { hold, release } => {
            hash.update([2]);
            hash.update(hold.as_bytes());
            stamp(&mut hash, release);
        }
    }
    hash.finalize().into()
}

fn stamp(hash: &mut Sha256, value: &TreatmentStamp) {
    hash.update(value.id.as_bytes());
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.actor.as_bytes());
    hash_text(hash, value.basis.id());
    hash.update(value.basis.version().to_be_bytes());
    hash_text(hash, value.basis.site_timezone());
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
    hash_occurrence(hash, value.occurred);
    hash_text(hash, value.observation.as_str());
}

fn scope(hash: &mut Sha256, value: &CapturedPhysicalScope) {
    hash.update(value.hub().as_bytes());
    hash.update(value.trace().as_bytes());
    hash.update(value.variant().as_bytes());
    hash.update(value.captured_revision().value().to_be_bytes());
    hash.update(
        u32::try_from(value.anchors().len())
            .expect("bounded scope")
            .to_be_bytes(),
    );
    for anchor in value.anchors() {
        hash.update(anchor.group.as_bytes());
        hash.update(anchor.created_revision.value().to_be_bytes());
        hash.update(anchor.captured_packs.value().to_be_bytes());
    }
}
