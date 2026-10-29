use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkRequirement {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    /// Original reference version defining this requirement's destination.
    pub target: Option<ReferenceRecord>,
    pub quantity: QuantityWork,
    pub revision: HubRevision,
}

impl WorkRequirement {
    pub fn validate(&self) -> Result<(), MovementError> {
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.value.kind() != ReferenceKind::Location)
            || self.quantity.required().value() == 0
            || (self.target.is_none() && self.quantity.completed().value() != 0)
        {
            return Err(MovementError::InvalidWorkScope);
        }
        Ok(())
    }

    pub(super) fn complete(&self, packs: WholePacks) -> Result<Self, MovementError> {
        if packs.value() == 0 {
            return Err(MovementError::InvalidWorkScope);
        }
        let quantity = QuantityWork::restore(
            self.quantity.required(),
            self.quantity.completed().checked_add(packs)?,
            self.quantity.no_longer_required(),
            self.quantity.remaining().checked_sub(packs)?,
        )?;
        Ok(Self {
            quantity,
            revision: next(self.revision)?,
            ..self.clone()
        })
    }

    fn retire_remaining(&self) -> Result<Self, MovementError> {
        if self.quantity.remaining().value() == 0 {
            return Err(MovementError::NoRemainingWork);
        }
        let quantity = QuantityWork::restore(
            self.quantity.required(),
            self.quantity.completed(),
            self.quantity
                .no_longer_required()
                .checked_add(self.quantity.remaining())?,
            WholePacks::from_value(0)?,
        )?;
        Ok(Self {
            quantity,
            revision: next(self.revision)?,
            ..self.clone()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReplanCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub requirement: HubRecordId,
    pub expected_work_revision: HubRevision,
    pub target: HubRecordId,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplanPlan {
    pub stock_revision: HubRevision,
    pub previous: WorkRequirement,
    pub replacement: WorkRequirement,
    pub groups: Vec<PhysicalStockGroup>,
}

impl ReplanCommand {
    /// All pending groups must come from one fresh protected native read. The
    /// checked sum must equal remaining work; a caller cannot replan a subset.
    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        stock_revision: HubRevision,
        work: &WorkRequirement,
        pending_groups: &[PhysicalStockGroup],
        replacement_id: HubRecordId,
    ) -> Result<ReplanPlan, MovementError> {
        work.validate()?;
        if self.expected_stock_revision != stock_revision
            || self.expected_catalog_revision != catalog.revision()
            || self.requirement != work.id
            || self.expected_work_revision != work.revision
        {
            return Err(MovementError::StaleRevision);
        }
        if replacement_id == work.id
            || work
                .target
                .as_ref()
                .is_some_and(|target| self.target == target.id)
        {
            return Err(MovementError::IdentityConflict);
        }
        if work.quantity.remaining().value() == 0 {
            return Err(MovementError::NoRemainingWork);
        }
        if pending_groups.is_empty() || pending_groups.len() > HUB_REQUIREMENT_GROUPS {
            return Err(MovementError::InvalidWorkScope);
        }
        let target = catalog
            .require_kind(self.target, ReferenceKind::Location)?
            .clone();
        let previous = work.retire_remaining()?;
        let replacement = WorkRequirement {
            id: replacement_id,
            trace: work.trace,
            variant: work.variant,
            target: Some(target),
            quantity: QuantityWork::pending(work.quantity.remaining()),
            revision: HubRevision::INITIAL,
        };
        let mut total = WholePacks::from_value(0)?;
        let mut groups = Vec::with_capacity(pending_groups.len());
        for (index, group) in pending_groups.iter().enumerate() {
            group.validate_catalog(catalog)?;
            if group.location == self.target
                || pending_groups[..index]
                    .iter()
                    .any(|other| other.id == group.id)
                || group
                    .work
                    .iter()
                    .any(|allocation| allocation.requirement.id == replacement_id)
            {
                return Err(MovementError::InvalidWorkScope);
            }
            if group.work.len() == HUB_GROUP_REQUIREMENTS {
                return Err(MovementError::Capacity);
            }
            let mut changed = group.clone();
            let allocation = changed
                .work
                .iter_mut()
                .find(|allocation| allocation.requirement.id == work.id)
                .ok_or(MovementError::InvalidWorkScope)?;
            if allocation.requirement != *work || allocation.progress != WorkProgress::Pending {
                return Err(MovementError::InvalidWorkScope);
            }
            total = total.checked_add(group.packs)?;
            allocation.requirement = previous.clone();
            allocation.progress = WorkProgress::NoLongerRequired;
            changed.work.push(WorkAllocation {
                requirement: replacement.clone(),
                progress: WorkProgress::Pending,
            });
            changed
                .work
                .sort_by(|a, b| a.requirement.id.as_bytes().cmp(b.requirement.id.as_bytes()));
            changed.revision = next(group.revision)?;
            changed.validate()?;
            groups.push(changed);
        }
        if total != work.quantity.remaining() {
            return Err(MovementError::InvalidWorkScope);
        }
        Ok(ReplanPlan {
            stock_revision: next(stock_revision)?,
            previous,
            replacement,
            groups,
        })
    }

    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        replacement_id: HubRecordId,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.replan.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(replacement_id.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.requirement.as_bytes());
        hash.update(self.expected_work_revision.value().to_be_bytes());
        hash.update(self.target.as_bytes());
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

/// Bounded follow-up only. Resolving it cannot change balances, work or holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discrepancy {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub group: Option<HubRecordId>,
    pub observation: SourceProvenance,
    pub revision: HubRevision,
    pub resolution: Option<SourceProvenance>,
}

impl Discrepancy {
    #[must_use]
    pub fn open(
        id: HubRecordId,
        trace: HubRecordId,
        group: Option<HubRecordId>,
        observation: SourceProvenance,
    ) -> Self {
        Self {
            id,
            trace,
            group,
            observation,
            revision: HubRevision::INITIAL,
            resolution: None,
        }
    }

    pub fn resolve(
        &self,
        expected_revision: HubRevision,
        resolution: SourceProvenance,
    ) -> Result<Self, MovementError> {
        if expected_revision != self.revision {
            return Err(MovementError::StaleRevision);
        }
        if self.resolution.is_some() {
            return Err(MovementError::AlreadyResolved);
        }
        Ok(Self {
            revision: next(self.revision)?,
            resolution: Some(resolution),
            ..self.clone()
        })
    }
}
