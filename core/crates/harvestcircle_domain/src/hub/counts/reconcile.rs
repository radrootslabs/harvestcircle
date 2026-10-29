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

/// A removed count scope remains historical work accounting, never a fake group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountWorkScopeChange {
    pub work: HubRecordId,
    pub before_progress: WorkProgress,
    pub before_scoped: WholePacks,
    pub on_hand_after: WholePacks,
    pub removed: WholePacks,
    pub removed_progress: WorkProgress,
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
        let plan = CountPostPlan {
            id: count_reconciliation_id(operation)?,
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
        };
        plan.validate()?;
        Ok(plan)
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

pub fn count_reconciliation_id(operation: HubRecordId) -> Result<HubRecordId, CountError> {
    derived_id(
        b"harvestcircle.hub.count.reconciliation.identity.v1\0",
        operation,
    )
}

impl CountPostPlan {
    /// Retain every removed scope, including work completed before the count.
    /// Only pending lost scope becomes no-longer-required; this derives no event
    /// time/cause or physical identity for the missing packs.
    pub fn work_scope_changes(&self) -> Result<Vec<CountWorkScopeChange>, CountError> {
        self.validate()?;
        let CountEffect::Shortage {
            packs, after_packs, ..
        } = &self.effect
        else {
            return Ok(Vec::new());
        };
        Ok(self
            .before
            .work
            .iter()
            .map(|allocation| CountWorkScopeChange {
                work: allocation.requirement.id,
                before_progress: allocation.progress,
                before_scoped: self.before.packs,
                on_hand_after: *after_packs,
                removed: *packs,
                removed_progress: match allocation.progress {
                    WorkProgress::Pending => WorkProgress::NoLongerRequired,
                    progress => progress,
                },
            })
            .collect())
    }

    /// Consistency of retained original facts, never current posting permission.
    pub fn validate(&self) -> Result<(), CountError> {
        self.count.validate()?;
        self.before
            .validate()
            .map_err(|_| CountError::InvalidHistory)?;
        let scope = &self.count.scope;
        if self.id != count_reconciliation_id(self.operation)?
            || self.stock_revision.value() <= self.count.recorded_revision.value()
            || self.catalog_revision.value() < scope.catalog_revision.value()
            || self.recorded_at < self.count.recorded_at
            || self.count.cutoff != Occurrence::Known(self.confirmed_on_hand_at)
            || self.confirmed_on_hand_at < scope.latest_physical_recorded_at
            || self
                .before
                .causal_floor
                .is_some_and(|at| at > self.confirmed_on_hand_at)
            || self.before.id != scope.group
            || self.before.trace != scope.trace
            || self.before.variant != scope.variant.id
            || self.before.location != scope.location.id
            || self.before.revision != scope.group_revision
            || self.before.packs != scope.recorded_packs
        {
            return Err(CountError::InvalidHistory);
        }
        match (&self.effect, self.count.difference()?) {
            (CountEffect::NoVariance, CountDifference::None) => Ok(()),
            (
                CountEffect::Surplus {
                    packs,
                    source,
                    acceptance,
                    ..
                },
                CountDifference::Surplus(expected),
            ) if *packs == expected
                && *source == SourceAttribution::Unknown
                && *acceptance == AcceptanceStatus::ReviewRequired =>
            {
                Ok(())
            }
            (
                CountEffect::Shortage {
                    packs,
                    after_packs,
                    after_revision,
                    work_changes,
                },
                CountDifference::Shortage(expected),
            ) if *packs == expected
                && *after_packs == self.count.counted_packs
                && *after_revision == super::next(self.before.revision)? =>
            {
                let pending = self
                    .before
                    .work
                    .iter()
                    .filter(|a| a.progress == WorkProgress::Pending)
                    .collect::<Vec<_>>();
                if pending.len() != work_changes.len() {
                    return Err(CountError::InvalidHistory);
                }
                for allocation in pending {
                    let before = &allocation.requirement;
                    let change = work_changes
                        .iter()
                        .find(|change| change.before.id == before.id)
                        .ok_or(CountError::InvalidHistory)?;
                    let expected = WorkRequirement {
                        quantity: QuantityWork::restore(
                            before.quantity.required(),
                            before.quantity.completed(),
                            before.quantity.no_longer_required().checked_add(*packs)?,
                            before.quantity.remaining().checked_sub(*packs)?,
                        )?,
                        revision: super::next(before.revision)?,
                        ..before.clone()
                    };
                    if change.before != *before
                        || change.after != expected
                        || change.retired != *packs
                    {
                        return Err(CountError::InvalidHistory);
                    }
                }
                Ok(())
            }
            _ => Err(CountError::InvalidHistory),
        }
    }
}
