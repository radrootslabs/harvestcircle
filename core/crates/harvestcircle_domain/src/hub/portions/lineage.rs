use std::collections::{BTreeMap, BTreeSet};

use super::{HUB_LINEAGE_GROUPS, HUB_PHYSICAL_SCOPES};
use crate::hub::reference::{HubRecordId, WholePacks};
use crate::hub::{HubId, HubRevision};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalScopeError {
    Capacity,
    InvalidLineage,
    InvalidSelection,
    HistoricalGap,
    QuantityOverflow,
}

/// Creation uses native record order, independently of uncertain physical times.
/// Empty retained ancestors remain necessary to resolve older captured scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalLineageNode {
    pub id: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub created_revision: HubRevision,
    pub packs: WholePacks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalLineage {
    hub: HubId,
    trace: HubRecordId,
    variant: HubRecordId,
    revision: HubRevision,
    nodes: Vec<PhysicalLineageNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct PhysicalScopeAnchor {
    pub group: HubRecordId,
    pub created_revision: HubRevision,
    pub captured_packs: WholePacks,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CapturedPhysicalScope {
    hub: HubId,
    trace: HubRecordId,
    variant: HubRecordId,
    captured_revision: HubRevision,
    anchors: Vec<PhysicalScopeAnchor>,
}

impl CapturedPhysicalScope {
    #[must_use]
    pub const fn hub(&self) -> HubId {
        self.hub
    }
    #[must_use]
    pub const fn trace(&self) -> HubRecordId {
        self.trace
    }
    #[must_use]
    pub const fn variant(&self) -> HubRecordId {
        self.variant
    }
    #[must_use]
    pub const fn captured_revision(&self) -> HubRevision {
        self.captured_revision
    }

    #[must_use]
    pub fn anchors(&self) -> &[PhysicalScopeAnchor] {
        &self.anchors
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPhysicalScope {
    revision: HubRevision,
    groups: Vec<PhysicalLineageNode>,
    packs: WholePacks,
}

impl ResolvedPhysicalScope {
    #[must_use]
    pub const fn revision(&self) -> HubRevision {
        self.revision
    }

    #[must_use]
    pub fn groups(&self) -> &[PhysicalLineageNode] {
        &self.groups
    }

    #[must_use]
    pub const fn packs(&self) -> WholePacks {
        self.packs
    }
}

impl PhysicalLineage {
    pub fn new(
        hub: HubId,
        trace: HubRecordId,
        variant: HubRecordId,
        revision: HubRevision,
        mut nodes: Vec<PhysicalLineageNode>,
    ) -> Result<Self, PhysicalScopeError> {
        if nodes.is_empty() || nodes.len() > HUB_LINEAGE_GROUPS {
            return Err(PhysicalScopeError::Capacity);
        }
        nodes.sort_by_key(|node| *node.id.as_bytes());
        if nodes.windows(2).any(|pair| pair[0].id == pair[1].id)
            || nodes.iter().filter(|node| node.parent.is_none()).count() != 1
        {
            return Err(PhysicalScopeError::InvalidLineage);
        }
        let value = Self {
            hub,
            trace,
            variant,
            revision,
            nodes,
        };
        let mut total = WholePacks::from_value(0).expect("zero whole packs");
        for node in &value.nodes {
            if node.trace != trace
                || node.variant != variant
                || node.created_revision.value() > revision.value()
            {
                return Err(PhysicalScopeError::InvalidLineage);
            }
            if let Some(parent) = node.parent {
                let parent = value
                    .node(parent)
                    .ok_or(PhysicalScopeError::InvalidLineage)?;
                if parent.created_revision.value() >= node.created_revision.value() {
                    return Err(PhysicalScopeError::InvalidLineage);
                }
            }
            total = total
                .checked_add(node.packs)
                .map_err(|_| PhysicalScopeError::QuantityOverflow)?;
        }
        Ok(value)
    }

    #[must_use]
    pub const fn hub(&self) -> HubId {
        self.hub
    }
    #[must_use]
    pub const fn trace(&self) -> HubRecordId {
        self.trace
    }
    #[must_use]
    pub const fn variant(&self) -> HubRecordId {
        self.variant
    }

    /// Rehydrate verified native originals, never a client selection or authority.
    /// Captured quantities are historical facts; current quantities come from lineage.
    pub fn restore_scope(
        &self,
        captured_revision: HubRevision,
        mut anchors: Vec<PhysicalScopeAnchor>,
    ) -> Result<CapturedPhysicalScope, PhysicalScopeError> {
        if anchors.is_empty() || anchors.len() > HUB_LINEAGE_GROUPS {
            return Err(PhysicalScopeError::Capacity);
        }
        if captured_revision.value() > self.revision.value() {
            return Err(PhysicalScopeError::HistoricalGap);
        }
        anchors.sort_by_key(|anchor| *anchor.group.as_bytes());
        if anchors
            .windows(2)
            .any(|pair| pair[0].group == pair[1].group)
        {
            return Err(PhysicalScopeError::InvalidSelection);
        }
        let mut total = WholePacks::from_value(0).expect("zero packs");
        for anchor in &anchors {
            let node = self
                .node(anchor.group)
                .ok_or(PhysicalScopeError::HistoricalGap)?;
            if node.created_revision != anchor.created_revision
                || anchor.created_revision.value() > captured_revision.value()
            {
                return Err(PhysicalScopeError::HistoricalGap);
            }
            if anchor.captured_packs.value() == 0 {
                return Err(PhysicalScopeError::InvalidSelection);
            }
            total = total
                .checked_add(anchor.captured_packs)
                .map_err(|_| PhysicalScopeError::QuantityOverflow)?;
        }
        let scope = CapturedPhysicalScope {
            hub: self.hub,
            trace: self.trace,
            variant: self.variant,
            captured_revision,
            anchors,
        };
        self.resolve(&scope)?;
        Ok(scope)
    }

    #[must_use]
    pub const fn revision(&self) -> HubRevision {
        self.revision
    }

    #[must_use]
    pub fn nodes(&self) -> &[PhysicalLineageNode] {
        &self.nodes
    }

    #[must_use]
    pub fn node(&self, id: HubRecordId) -> Option<&PhysicalLineageNode> {
        self.nodes
            .binary_search_by_key(id.as_bytes(), |node| *node.id.as_bytes())
            .ok()
            .map(|index| &self.nodes[index])
    }

    pub fn capture(
        &self,
        selected: &[HubRecordId],
    ) -> Result<CapturedPhysicalScope, PhysicalScopeError> {
        if selected.is_empty() || selected.len() > HUB_LINEAGE_GROUPS {
            return Err(PhysicalScopeError::InvalidSelection);
        }
        let mut anchors = Vec::with_capacity(selected.len());
        for id in selected {
            let node = self.node(*id).ok_or(PhysicalScopeError::InvalidSelection)?;
            if node.packs.value() == 0 {
                return Err(PhysicalScopeError::InvalidSelection);
            }
            anchors.push(PhysicalScopeAnchor {
                group: *id,
                created_revision: node.created_revision,
                captured_packs: node.packs,
            });
        }
        anchors.sort_by_key(|anchor| *anchor.group.as_bytes());
        if anchors
            .windows(2)
            .any(|pair| pair[0].group == pair[1].group)
        {
            return Err(PhysicalScopeError::InvalidSelection);
        }
        Ok(CapturedPhysicalScope {
            hub: self.hub,
            trace: self.trace,
            variant: self.variant,
            captured_revision: self.revision,
            anchors,
        })
    }

    pub fn resolve(
        &self,
        scope: &CapturedPhysicalScope,
    ) -> Result<ResolvedPhysicalScope, PhysicalScopeError> {
        self.resolve_union(std::slice::from_ref(scope))
    }

    /// Resolve in creation order. A pre-existing detached branch is a boundary,
    /// even if one of its descendants was created after the capture revision.
    pub fn resolve_union(
        &self,
        scopes: &[CapturedPhysicalScope],
    ) -> Result<ResolvedPhysicalScope, PhysicalScopeError> {
        if scopes.len() > HUB_PHYSICAL_SCOPES {
            return Err(PhysicalScopeError::Capacity);
        }
        let mut chronological = self.nodes.iter().collect::<Vec<_>>();
        chronological.sort_by_key(|node| (node.created_revision.value(), *node.id.as_bytes()));
        let mut selected = BTreeMap::new();
        for scope in scopes {
            if scope.hub != self.hub || scope.trace != self.trace || scope.variant != self.variant {
                return Err(PhysicalScopeError::InvalidSelection);
            }
            if scope.captured_revision.value() > self.revision.value() {
                return Err(PhysicalScopeError::HistoricalGap);
            }
            let mut anchors = BTreeSet::new();
            for anchor in &scope.anchors {
                let node = self
                    .node(anchor.group)
                    .ok_or(PhysicalScopeError::HistoricalGap)?;
                if node.created_revision != anchor.created_revision {
                    return Err(PhysicalScopeError::HistoricalGap);
                }
                anchors.insert(*anchor.group.as_bytes());
            }
            let mut included = BTreeSet::new();
            for node in &chronological {
                let inherited = node.created_revision.value() > scope.captured_revision.value()
                    && node
                        .parent
                        .is_some_and(|parent| included.contains(parent.as_bytes()));
                if anchors.contains(node.id.as_bytes()) || inherited {
                    included.insert(*node.id.as_bytes());
                    if node.packs.value() > 0 {
                        selected.insert(*node.id.as_bytes(), **node);
                    }
                }
            }
        }
        let groups = selected.into_values().collect::<Vec<_>>();
        let packs = groups.iter().try_fold(
            WholePacks::from_value(0).expect("zero whole packs"),
            |total, node| {
                total
                    .checked_add(node.packs)
                    .map_err(|_| PhysicalScopeError::QuantityOverflow)
            },
        )?;
        Ok(ResolvedPhysicalScope {
            revision: self.revision,
            groups,
            packs,
        })
    }
}

#[cfg(test)]
mod tests;
