use super::*;
use crate::hub::dispatch::{DispatchContext, DispatchMaterial, IdentificationStatus};
use crate::hub::movement::{hash_occurrence, hash_text};
use crate::hub::treatment::{PhysicalAction, TreatmentSelection};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepartureCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub preparation: HubRecordId,
    pub expected_preparation_revision: HubRevision,
    pub identification: HubRecordId,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}
/// Complete native current history, reprojected by Rust at the mutation cut.
pub struct DepartureTreatment {
    pub lineage: PhysicalLineage,
    pub state: TreatmentState,
}
pub struct DepartureContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub paper: &'a DispatchRecord,
    pub sources: &'a [DispatchSource],
    pub treatments: &'a [DepartureTreatment],
    /// Native current usage lookup for this preparation, after original replay.
    pub prior_departure: Option<&'a DepartureRecord>,
}
impl DepartureCommand {
    pub fn plan(
        &self,
        context: &DepartureContext<'_>,
        ids: DepartureIdentities,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<DepartureRecord, DepartureError> {
        if ids != DepartureIdentities::from_operation_bytes(ids.operation)? {
            return Err(DepartureError::InvalidIdentity);
        }
        if context.prior_departure.is_some() {
            return Err(DepartureError::AlreadyUsed);
        }
        let paper = context.paper;
        paper.validate()?;
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
            || self.preparation != paper.id
            || self.expected_preparation_revision != paper.revision
            || paper.hub != context.hub
        {
            return Err(DepartureError::StaleRevision);
        }
        let issued = paper
            .versions
            .last()
            .ok_or(DepartureError::IdentificationUnavailable)?;
        if paper.draft.is_some()
            || issued.id != self.identification
            || issued.status != IdentificationStatus::Issued
            || issued.closed.is_some()
        {
            return Err(DepartureError::IdentificationUnavailable);
        }
        let fresh = DispatchMaterial::capture(
            &DispatchContext {
                hub: context.hub,
                stock_revision: context.stock_revision,
                catalog: context.catalog,
                sources: context.sources,
                current: Some(paper),
            },
            issued.material.recipient.id,
            &issued.material.selection()?,
            issued.material.observation.clone(),
        )?;
        if !fresh.same_material(&issued.material) {
            return Err(DepartureError::MaterialChanged);
        }
        let traces: BTreeSet<_> = issued
            .material
            .lines
            .iter()
            .map(|line| line.trace.id)
            .collect();
        if context.treatments.len() != traces.len() {
            return Err(DepartureError::InvalidSources);
        }
        let mut seen = BTreeSet::new();
        for treatment in context.treatments {
            let graph = &treatment.lineage;
            if graph.hub() != context.hub
                || graph.revision() != context.stock_revision
                || !traces.contains(&graph.trace())
                || !seen.insert(graph.trace())
            {
                return Err(DepartureError::InvalidSources);
            }
            let mut selected = Vec::new();
            for source in context
                .sources
                .iter()
                .filter(|source| source.group.trace == graph.trace())
            {
                let group = &source.group;
                group.validate_catalog(context.catalog)?;
                let node = graph.node(group.id).ok_or(DepartureError::InvalidSources)?;
                if node.trace != group.trace
                    || node.variant != group.variant
                    || node.parent != group.parent
                    || node.packs != group.packs
                {
                    return Err(DepartureError::InvalidSources);
                }
                selected.push(group.id);
            }
            treatment
                .state
                .project(graph, context.catalog.handling_basis())?
                .require(
                    &TreatmentSelection::new(selected)?,
                    PhysicalAction::NormalDeparture,
                )?;
        }
        let mut consumed = Vec::with_capacity(issued.material.lines.len());
        for line in &issued.material.lines {
            let source = context
                .sources
                .iter()
                .find(|source| source.group.id == line.group)
                .ok_or(DepartureError::InvalidSources)?;
            consumed.push(ConsumedStockGroup {
                before: source.group.clone(),
                after_revision: next(source.group.revision)?,
                causal_floor: source.group.observation_floor(self.occurred, now)?,
            });
        }
        let record = DepartureRecord {
            id: ids.departure,
            operation: ids.operation,
            hub: context.hub,
            stock_revision: next(context.stock_revision)?,
            catalog_revision: context.catalog.revision(),
            actor,
            recorded_at: now,
            occurred: self.occurred,
            observation: self.observation.clone(),
            paper: paper.clone(),
            work_changes: retired_work(&consumed)?,
            consumed,
        };
        record.validate()?;
        Ok(record)
    }
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, ids: DepartureIdentities) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.departure.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(ids.operation.as_bytes());
        hash.update(ids.departure.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.preparation.as_bytes());
        hash.update(self.expected_preparation_revision.value().to_be_bytes());
        hash.update(self.identification.as_bytes());
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
