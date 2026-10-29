use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchLineInput {
    pub group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub packs: WholePacks,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchSelection(Vec<DispatchLineInput>);

impl DispatchSelection {
    pub fn new(mut lines: Vec<DispatchLineInput>) -> Result<Self, DispatchError> {
        if lines.is_empty() || lines.iter().any(|line| line.packs.value() == 0) {
            return Err(DispatchError::InvalidSelection);
        }
        if lines.len() > HUB_DISPATCH_LINES {
            return Err(DispatchError::Capacity);
        }
        lines.sort_by_key(|line| line.group);
        if lines.windows(2).any(|pair| pair[0].group == pair[1].group) {
            return Err(DispatchError::InvalidSelection);
        }
        Ok(Self(lines))
    }

    #[must_use]
    pub fn lines(&self) -> &[DispatchLineInput] {
        &self.0
    }
}

/// Already validated original receipt and complete current physical group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchSource {
    pub group: PhysicalStockGroup,
    pub trace: StockPopulation,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchLineSnapshot {
    pub group: HubRecordId,
    pub group_revision: HubRevision,
    pub packs: WholePacks,
    pub identification: Option<PhysicalIdentification>,
    pub location: ReferenceRecord,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub trace: StockPopulation,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchMaterial {
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub basis: SampleHandlingBasis,
    pub recipient: ReferenceRecord,
    pub lines: Vec<DispatchLineSnapshot>,
    pub observation: SourceProvenance,
}

impl DispatchMaterial {
    pub fn capture(
        context: &DispatchContext<'_>,
        recipient: HubRecordId,
        selection: &DispatchSelection,
        observation: SourceProvenance,
    ) -> Result<Self, DispatchError> {
        if context.sources.len() != selection.lines().len() {
            return Err(DispatchError::InvalidSelection);
        }
        let mut source_ids = BTreeSet::new();
        if context
            .sources
            .iter()
            .any(|source| !source_ids.insert(source.group.id))
        {
            return Err(DispatchError::InvalidSelection);
        }
        let catalog = context.catalog;
        let reference = |id, kind| {
            catalog
                .require_kind(id, kind)
                .cloned()
                .map_err(|_| DispatchError::InvalidSelection)
        };
        let mut lines = Vec::with_capacity(selection.lines().len());
        for input in selection.lines() {
            let source = context
                .sources
                .iter()
                .find(|source| source.group.id == input.group)
                .ok_or(DispatchError::InvalidSelection)?;
            let group = &source.group;
            group
                .validate()
                .map_err(|_| DispatchError::InvalidSelection)?;
            if group.revision != input.expected_group_revision {
                return Err(DispatchError::StaleRevision);
            }
            if input.packs != group.packs {
                return Err(DispatchError::PhysicalSeparationRequired);
            }
            if group.trace != source.trace.id || group.variant != source.trace.variant.id {
                return Err(DispatchError::InvalidSelection);
            }
            let variant = reference(group.variant, ReferenceKind::Pack)?;
            let ReferenceValue::Pack { product, .. } = variant.value else {
                return Err(DispatchError::InvalidSelection);
            };
            lines.push(DispatchLineSnapshot {
                group: group.id,
                group_revision: group.revision,
                packs: group.packs,
                identification: group.identification.clone(),
                location: reference(group.location, ReferenceKind::Location)?,
                product: reference(product, ReferenceKind::Product)?,
                variant,
                trace: source.trace.clone(),
            });
        }
        let material = Self {
            stock_revision: context.stock_revision,
            catalog_revision: catalog.revision(),
            basis: catalog.handling_basis(),
            recipient: reference(recipient, ReferenceKind::Party)?,
            lines,
            observation,
        };
        material.validate()?;
        Ok(material)
    }

    pub fn validate(&self) -> Result<(), DispatchError> {
        if self.lines.is_empty()
            || self.lines.len() > HUB_DISPATCH_LINES
            || self
                .lines
                .windows(2)
                .any(|pair| pair[0].group >= pair[1].group)
            || self.recipient.value.kind() != ReferenceKind::Party
            || self.recipient.revision.value() > self.catalog_revision.value()
        {
            return Err(DispatchError::InvalidHistory);
        }
        let mut ids = BTreeSet::new();
        for line in &self.lines {
            if line.packs.value() == 0
                || !ids.insert(line.group)
                || line.product.value.kind() != ReferenceKind::Product
                || line.variant.value.kind() != ReferenceKind::Pack
                || line.location.value.kind() != ReferenceKind::Location
                || line.variant.id != line.trace.variant.id
                || line.product.id != line.trace.product.id
                || [
                    line.product.revision,
                    line.variant.revision,
                    line.location.revision,
                ]
                .iter()
                .any(|revision| revision.value() > self.catalog_revision.value())
                || !matches!(line.variant.value, ReferenceValue::Pack { product, .. } if product == line.product.id)
            {
                return Err(DispatchError::InvalidHistory);
            }
        }
        Ok(())
    }

    /// Whole packs across different source lines; never a mass or reservation.
    pub fn total_packs(&self) -> Result<u32, DispatchError> {
        self.validate()?;
        self.lines.iter().try_fold(0_u32, |total, line| {
            total
                .checked_add(line.packs.value())
                .ok_or(DispatchError::Capacity)
        })
    }

    pub fn selection(&self) -> Result<DispatchSelection, DispatchError> {
        DispatchSelection::new(
            self.lines
                .iter()
                .map(|line| DispatchLineInput {
                    group: line.group,
                    expected_group_revision: line.group_revision,
                    packs: line.packs,
                })
                .collect(),
        )
    }

    /// Captured revision counters alone do not change the material identification.
    #[must_use]
    pub fn same_material(&self, other: &Self) -> bool {
        self.recipient == other.recipient
            && self.lines == other.lines
            && self.observation == other.observation
            && self.basis == other.basis
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchStamp {
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub observation: SourceProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentificationStatus {
    Issued,
    Superseded,
    Unused,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct IdentificationVersion {
    pub id: HubRecordId,
    pub number: u32,
    pub previous: Option<HubRecordId>,
    pub material: DispatchMaterial,
    pub issued: DispatchStamp,
    pub status: IdentificationStatus,
    pub closed: Option<DispatchStamp>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchRecord {
    pub id: HubRecordId,
    pub hub: HubId,
    pub revision: HubRevision,
    pub created: DispatchStamp,
    pub updated: DispatchStamp,
    pub draft: Option<DispatchMaterial>,
    pub versions: Vec<IdentificationVersion>,
}

impl DispatchRecord {
    pub fn validate(&self) -> Result<(), DispatchError> {
        let minimum_revision = self.versions.len() as u64
            + 1
            + u64::from(
                self.versions
                    .last()
                    .is_some_and(|version| version.status == IdentificationStatus::Unused),
            );
        if self.versions.len() > HUB_IDENTIFICATION_VERSIONS
            || self.draft.is_some() != self.versions.is_empty()
            || self.created.stock_revision.value() < 2
            || DispatchIdentities::from_operation_bytes(self.created.operation)?.preparation
                != self.id
            || self.created.stock_revision.value() > self.updated.stock_revision.value()
            || (self.revision == HubRevision::INITIAL && self.created != self.updated)
            || self.revision.value() < minimum_revision
            || self
                .updated
                .stock_revision
                .value()
                .checked_sub(self.created.stock_revision.value())
                .is_none_or(|advanced| advanced < self.revision.value() - 1)
        {
            return Err(DispatchError::InvalidHistory);
        }
        if let Some(draft) = &self.draft {
            draft.validate()?;
            if draft.stock_revision.value().checked_add(1)
                != Some(self.updated.stock_revision.value())
            {
                return Err(DispatchError::InvalidHistory);
            }
        }
        let mut ids = BTreeSet::new();
        for (index, version) in self.versions.iter().enumerate() {
            version.material.validate()?;
            let prior = index.checked_sub(1).map(|prior| &self.versions[prior]);
            if version.id == self.id
                || !ids.insert(version.id)
                || DispatchIdentities::from_operation_bytes(version.issued.operation)?
                    .identification
                    != version.id
                || version.number as usize != index + 1
                || version.previous != prior.map(|previous| previous.id)
                || version.material.stock_revision.value() >= version.issued.stock_revision.value()
                || version.issued.stock_revision.value() < self.created.stock_revision.value()
                || version.issued.stock_revision.value() > self.updated.stock_revision.value()
                || (version.status == IdentificationStatus::Issued) != version.closed.is_none()
                || (index + 1 < self.versions.len()
                    && version.status != IdentificationStatus::Superseded)
            {
                return Err(DispatchError::InvalidHistory);
            }
            if let Some(closed) = &version.closed
                && (closed.stock_revision.value() <= version.issued.stock_revision.value()
                    || closed.stock_revision.value() > self.updated.stock_revision.value())
            {
                return Err(DispatchError::InvalidHistory);
            }
            if let Some(previous) = prior
                && previous.closed.as_ref() != Some(&version.issued)
            {
                return Err(DispatchError::InvalidHistory);
            }
        }
        if let Some(last) = self.versions.last() {
            let expected_update = match last.status {
                IdentificationStatus::Issued => &last.issued,
                IdentificationStatus::Unused => {
                    last.closed.as_ref().ok_or(DispatchError::InvalidHistory)?
                }
                IdentificationStatus::Superseded => return Err(DispatchError::InvalidHistory),
            };
            if &self.updated != expected_update {
                return Err(DispatchError::InvalidHistory);
            }
        }
        Ok(())
    }
}
