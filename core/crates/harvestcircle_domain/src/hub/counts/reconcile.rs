use super::*;
use crate::hub::movement::{WorkProgress, WorkRequirement};
use crate::hub::reference::SourceAttribution;
use crate::hub::stock::QuantityWork;
use crate::hub::treatment::AcceptanceStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountPostCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub count: HubRecordId,
    /// Explicit operator review; it never substitutes for native activity/authority checks.
    pub reviewed_activity: bool,
    /// Positive difference is separately identified new counted stock of unknown origin.
    pub surplus_identification: Option<PhysicalIdentification>,
    pub reason: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountWorkChange {
    pub before: WorkRequirement,
    pub after: WorkRequirement,
    pub retired: WholePacks,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CountEffect {
    NoVariance,
    Shortage {
        packs: WholePacks,
        after_packs: WholePacks,
        after_revision: HubRevision,
        work_changes: Vec<CountWorkChange>,
    },
    /// Never enlarge an earlier accepted/held group or infer the extra packs' source.
    Surplus {
        packs: WholePacks,
        identification: PhysicalIdentification,
        source: SourceAttribution,
        acceptance: AcceptanceStatus,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountPostPlan {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub count: CountObservation,
    pub before: PhysicalStockGroup,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    /// A count establishes this on-hand observation, not the time/cause of any loss.
    pub confirmed_on_hand_at: UnixTimestamp,
    pub effect: CountEffect,
    pub reason: SourceProvenance,
}
impl CountPostCommand {
    pub fn plan(
        &self,
        count: &CountObservation,
        current: &CountContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        recorded_at: UnixTimestamp,
    ) -> Result<CountPostPlan, CountError> {
        if self.expected_stock_revision != current.stock_revision
            || self.expected_catalog_revision != current.catalog.revision()
        {
            return Err(CountError::StaleRevision);
        }
        if self.count != count.id || !self.reviewed_activity {
            return Err(CountError::ReviewRequired);
        }
        if recorded_at < count.recorded_at || recorded_at < current.latest_physical_recorded_at {
            return Err(CountError::InvalidHistory);
        }
        let review = count.review(current)?;
        if !review.ready() {
            return Err(CountError::ReviewRequired);
        }
        let Occurrence::Known(confirmed_on_hand_at) = count.cutoff else {
            return Err(CountError::ReviewRequired);
        };
        let effect = match review.difference {
            CountDifference::None if self.surplus_identification.is_none() => {
                CountEffect::NoVariance
            }
            CountDifference::Shortage(packs) if self.surplus_identification.is_none() => {
                let changes = current
                    .group
                    .work
                    .iter()
                    .filter(|a| a.progress == WorkProgress::Pending)
                    .map(|allocation| {
                        let before = &allocation.requirement;
                        let quantity = QuantityWork::restore(
                            before.quantity.required(),
                            before.quantity.completed(),
                            before.quantity.no_longer_required().checked_add(packs)?,
                            before.quantity.remaining().checked_sub(packs)?,
                        )?;
                        Ok(CountWorkChange {
                            before: before.clone(),
                            after: WorkRequirement {
                                quantity,
                                revision: super::next(before.revision)?,
                                ..before.clone()
                            },
                            retired: packs,
                        })
                    })
                    .collect::<Result<Vec<_>, CountError>>()?;
                CountEffect::Shortage {
                    packs,
                    after_packs: count.counted_packs,
                    after_revision: super::next(current.group.revision)?,
                    work_changes: changes,
                }
            }
            CountDifference::Surplus(packs) => CountEffect::Surplus {
                packs,
                identification: self
                    .surplus_identification
                    .clone()
                    .ok_or(CountError::InvalidInput)?,
                source: SourceAttribution::Unknown,
                acceptance: AcceptanceStatus::ReviewRequired,
            },
            _ => return Err(CountError::InvalidInput),
        };
        Ok(CountPostPlan {
            id: derived_id(
                b"harvestcircle.hub.count.reconciliation.identity.v1\0",
                operation,
            )?,
            operation,
            count: count.clone(),
            before: current.group.clone(),
            stock_revision: super::next(current.stock_revision)?,
            catalog_revision: current.catalog.revision(),
            actor,
            recorded_at,
            confirmed_on_hand_at,
            effect,
            reason: self.reason.clone(),
        })
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.count.post.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.count.as_bytes());
        hash.update([u8::from(self.reviewed_activity)]);
        if let Some(value) = &self.surplus_identification {
            hash.update([1]);
            hash_text(&mut hash, value.label.as_str());
            hash_text(&mut hash, value.observation.as_str());
        } else {
            hash.update([0]);
        }
        hash_text(&mut hash, self.reason.as_str());
        hash.finalize().into()
    }
}
