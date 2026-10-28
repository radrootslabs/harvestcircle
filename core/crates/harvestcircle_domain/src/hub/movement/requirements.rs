use super::*;

/// Explicit new handling after earlier work. Ordinary backward movement never
/// creates this requirement or resets completed progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewWorkCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    /// Canonical ascending group identities; one trace and original-pack variant.
    pub groups: Vec<HubRecordId>,
    pub target: HubRecordId,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewWorkPlan {
    pub stock_revision: HubRevision,
    pub requirement: WorkRequirement,
    pub groups: Vec<PhysicalStockGroup>,
}

impl NewWorkCommand {
    pub fn validate_shape(&self) -> Result<(), MovementError> {
        if self.groups.is_empty()
            || self.groups.len() > HUB_REQUIREMENT_GROUPS
            || self
                .groups
                .windows(2)
                .any(|pair| pair[0].as_bytes() >= pair[1].as_bytes())
        {
            return Err(MovementError::InvalidWorkScope);
        }
        Ok(())
    }

    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        stock_revision: HubRevision,
        groups: &[PhysicalStockGroup],
        requirement_id: HubRecordId,
    ) -> Result<NewWorkPlan, MovementError> {
        if self.expected_stock_revision != stock_revision
            || self.expected_catalog_revision != catalog.revision()
        {
            return Err(MovementError::StaleRevision);
        }
        self.validate_shape()?;
        if self.groups.len() != groups.len() {
            return Err(MovementError::InvalidWorkScope);
        }
        let first = groups.first().ok_or(MovementError::InvalidWorkScope)?;
        catalog.require_kind(first.variant, ReferenceKind::Pack)?;
        let target = catalog
            .require_kind(self.target, ReferenceKind::Location)?
            .clone();
        let mut quantity = WholePacks::from_value(0)?;
        for (expected, group) in self.groups.iter().zip(groups) {
            group.validate_catalog(catalog)?;
            if group.id != *expected
                || group.trace != first.trace
                || group.variant != first.variant
                || group.location == self.target
                || group.work.iter().any(|allocation| {
                    allocation.progress == WorkProgress::Pending
                        || allocation.requirement.id == requirement_id
                })
            {
                return Err(MovementError::InvalidWorkScope);
            }
            if group.work.len() == HUB_GROUP_REQUIREMENTS {
                return Err(MovementError::Capacity);
            }
            quantity = quantity.checked_add(group.packs)?;
        }
        let requirement = WorkRequirement {
            id: requirement_id,
            trace: first.trace,
            variant: first.variant,
            target: Some(target),
            quantity: QuantityWork::pending(quantity),
            revision: HubRevision::INITIAL,
        };
        let changed = groups
            .iter()
            .map(|group| {
                let mut changed = group.clone();
                changed.work.push(WorkAllocation {
                    requirement: requirement.clone(),
                    progress: WorkProgress::Pending,
                });
                changed.revision = next(group.revision)?;
                changed.validate()?;
                Ok(changed)
            })
            .collect::<Result<Vec<_>, MovementError>>()?;
        Ok(NewWorkPlan {
            stock_revision: next(stock_revision)?,
            requirement,
            groups: changed,
        })
    }

    /// Validate command shape in plan before accepting its intent hash natively.
    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        requirement_id: HubRecordId,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.new-work.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(requirement_id.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(
            u64::try_from(self.groups.len())
                .expect("bounded group list")
                .to_be_bytes(),
        );
        for group in &self.groups {
            hash.update(group.as_bytes());
        }
        hash.update(self.target.as_bytes());
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
