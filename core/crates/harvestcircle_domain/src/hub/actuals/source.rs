//! Immutable historical position evidence is distinct from today's balance.
//! Native storage must load the original operation and complete affected physical
//! activity. These values have no request decoder and confer no execution grant.
use super::*;
use crate::hub::movement::PhysicalStockGroup;
use crate::hub::portions::{PhysicalLineage, PhysicalScopeAnchor};
use crate::hub::reference::{ReferenceKind, ReferenceRecord};

/// An original physical position, without historical work or action authority.
/// Native adapters construct it from an immutable birth/arrival original.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct HistoricalActualPosition {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub location: HubRecordId,
    pub packs: WholePacks,
    pub revision: HubRevision,
}
impl From<&PhysicalStockGroup> for HistoricalActualPosition {
    fn from(group: &PhysicalStockGroup) -> Self {
        Self {
            id: group.id,
            trace: group.trace,
            variant: group.variant,
            parent: group.parent,
            location: group.location,
            packs: group.packs,
            revision: group.revision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct HistoricalActualSource {
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    /// Original group snapshot from a native birth or arrival operation.
    pub group: HistoricalActualPosition,
    pub location: ReferenceRecord,
    pub occurred: Occurrence,
    pub recorded_at: UnixTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActualPhysicalActivityKind {
    GroupBorn,
    LocationChanged,
    CountReconciled,
    OtherPhysicalEffect,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualPhysicalActivity {
    pub sequence: u32,
    pub operation: HubRecordId,
    pub actor: PublicKey,
    pub stock_revision: HubRevision,
    pub hub: HubId,
    pub trace: HubRecordId,
    pub group: HubRecordId,
    pub location: Option<HubRecordId>,
    pub kind: ActualPhysicalActivityKind,
    pub occurred: Occurrence,
    pub recorded_at: UnixTimestamp,
}

pub struct ActualSourceContext<'a> {
    pub source: &'a HistoricalActualSource,
    pub lineage: &'a PhysicalLineage,
    pub balancing_group: &'a PhysicalStockGroup,
    /// Complete native physical activity for this population after the selected
    /// immutable witness, in native stock order. Never client-supplied clear flags.
    pub subsequent_activity: &'a [ActualPhysicalActivity],
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ResolvedActualSource {
    historical: HistoricalActualSource,
    balancing_group: HubRecordId,
}
impl ResolvedActualSource {
    #[must_use]
    pub const fn historical(&self) -> &HistoricalActualSource {
        &self.historical
    }
    #[must_use]
    pub const fn balancing_group(&self) -> HubRecordId {
        self.balancing_group
    }
}

fn bounds(value: Occurrence) -> Option<(UnixTimestamp, UnixTimestamp)> {
    match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } if earliest <= latest => Some((earliest, latest)),
        _ => None,
    }
}

impl HistoricalActualSource {
    pub fn validate(&self) -> Result<(), ActualReportError> {
        self.occurred
            .validate(self.recorded_at)
            .map_err(|_| ActualReportError::InvalidHistory)?;
        if self.location.id != self.group.location
            || self.group.packs.value() == 0
            || self.group.parent == Some(self.group.id)
            || self.location.value.kind() != ReferenceKind::Location
            || self.location.revision.value() > self.catalog_revision.value()
            || self.group.revision.value() > self.stock_revision.value()
        {
            return Err(ActualReportError::InvalidHistory);
        }
        Ok(())
    }
}

impl ActualSourceContext<'_> {
    /// Resolves a manually reviewed historical claim against a specific original
    /// native physical snapshot and complete subsequent activity. Does not infer
    /// a source from today's location, select the first candidate or post stock.
    pub fn resolve(
        &self,
        original: &ActualReportRecord,
        occurred: Occurrence,
        now: UnixTimestamp,
    ) -> Result<ResolvedActualSource, ActualReportError> {
        original.validate()?;
        let source = self.source;
        source.validate()?;
        let current = self.balancing_group;
        current
            .validate()
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        let graph = self.lineage;
        if source.hub != original.hub
            || source.hub != graph.hub()
            || source.group.trace != graph.trace()
            || source.group.variant != graph.variant()
            || current.trace != graph.trace()
            || current.variant != graph.variant()
            || source.stock_revision.value() > graph.revision().value()
            || original.stock_revision.value() > graph.revision().value()
            || source.recorded_at > now
            || original.recorded_at > now
            || current.revision.value() > graph.revision().value()
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        occurred
            .validate(original.recorded_at)
            .map_err(|_| ActualReportError::InvalidInput)?;
        if original
            .report
            .variant
            .is_some_and(|v| v != source.group.variant)
            || !super::review::refines(original.report.occurred, occurred)
        {
            return Err(ActualReportError::ConflictingEvidence);
        }
        let native = graph
            .node(source.group.id)
            .ok_or(ActualReportError::InvalidNativeContext)?;
        let current_node = graph
            .node(current.id)
            .ok_or(ActualReportError::InvalidNativeContext)?;
        if native.parent != source.group.parent
            || current_node.parent != current.parent
            || current_node.packs != current.packs
        {
            return Err(ActualReportError::InvalidNativeContext);
        }
        let captured = graph
            .restore_scope(
                source.stock_revision,
                vec![PhysicalScopeAnchor {
                    group: source.group.id,
                    created_revision: native.created_revision,
                    captured_packs: source.group.packs,
                }],
            )
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        let resolved = graph
            .resolve(&captured)
            .map_err(|_| ActualReportError::InvalidNativeContext)?;
        if resolved.packs().value() > source.group.packs.value() {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        if !resolved.groups().iter().any(|g| g.id == current.id) {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        let matches = |s: &ReportedActualSource| {
            s.trace == source.group.trace
                && s.group.is_none_or(|v| v == source.group.id)
                && s.location.is_none_or(|v| v == source.location.id)
        };
        if !matches!(&original.report.sources, ActualReportSources::Unknown)
            && !original.report.sources.candidates().iter().any(matches)
        {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        let Some((depart_min, depart_max)) = bounds(occurred) else {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        };
        if !bounds(source.occurred).is_some_and(|(_, latest)| latest <= depart_min) {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        if self.subsequent_activity.len() > super::super::stock::HUB_STOCK_EVENT_CAPACITY {
            return Err(ActualReportError::Capacity);
        }
        let mut previous = source.stock_revision.value();
        let mut previous_sequence = 0;
        let mut operations = std::collections::BTreeMap::new();
        let mut balancing_birth_known = current.id == source.group.id;
        for event in self.subsequent_activity {
            if event.hub != source.hub
                || event.trace != source.group.trace
                || event.stock_revision.value() < previous
                || event.stock_revision.value() <= source.stock_revision.value()
                || event.stock_revision.value() > graph.revision().value()
                || event.recorded_at < source.recorded_at
                || event.recorded_at > now
                || event.operation == source.operation
                || event.sequence <= previous_sequence
                || event.sequence as usize > super::super::stock::HUB_STOCK_EVENT_CAPACITY
            {
                return Err(ActualReportError::InvalidNativeContext);
            }
            if operations
                .insert(
                    event.operation,
                    (event.stock_revision, event.recorded_at, event.actor),
                )
                .is_some_and(|old| old != (event.stock_revision, event.recorded_at, event.actor))
            {
                return Err(ActualReportError::InvalidNativeContext);
            }
            event
                .occurred
                .validate(event.recorded_at)
                .map_err(|_| ActualReportError::InvalidNativeContext)?;
            previous = event.stock_revision.value();
            previous_sequence = event.sequence;
            let node = graph
                .node(event.group)
                .ok_or(ActualReportError::InvalidNativeContext)?;
            if event.kind == ActualPhysicalActivityKind::GroupBorn {
                if event.stock_revision != node.created_revision || event.location.is_none() {
                    return Err(ActualReportError::InvalidNativeContext);
                }
                if event.group == current.id
                    && event.location == Some(source.location.id)
                    && bounds(event.occurred).is_some_and(|(_, latest)| latest <= depart_min)
                {
                    balancing_birth_known = true;
                }
            }
            let affected = event.group == source.group.id
                || (node.created_revision.value() > source.stock_revision.value()
                    && descendant_of(graph, event.group, source.group.id));
            if !affected {
                continue;
            }
            match event.kind {
                // A later count may already account for this missing quantity.
                // A linked correction, not another stock reduction, is required.
                ActualPhysicalActivityKind::CountReconciled => {
                    return Err(ActualReportError::HistoricalSourceUnresolved);
                }
                ActualPhysicalActivityKind::LocationChanged
                    if !bounds(event.occurred)
                        .is_some_and(|(earliest, _)| depart_max < earliest) =>
                {
                    return Err(ActualReportError::HistoricalSourceUnresolved);
                }
                _ => {}
            }
        }
        if !balancing_birth_known {
            return Err(ActualReportError::HistoricalSourceUnresolved);
        }
        Ok(ResolvedActualSource {
            historical: source.clone(),
            balancing_group: current.id,
        })
    }
}

fn descendant_of(graph: &PhysicalLineage, mut group: HubRecordId, ancestor: HubRecordId) -> bool {
    // Native lineage construction guarantees an acyclic strictly advancing tree.
    while let Some(parent) = graph.node(group).and_then(|g| g.parent) {
        if parent == ancestor {
            return true;
        }
        group = parent;
    }
    false
}
