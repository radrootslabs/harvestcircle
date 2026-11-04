use super::super::movement::{hash_occurrence, hash_text};
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub variant: HubRecordId,
    pub location: HubRecordId,
    pub returned_by: Option<HubRecordId>,
    pub source: SourceAttribution,
    pub packs: WholePacks,
    pub identification: PhysicalIdentification,
    /// An explicit new handling requirement, never the original departed work.
    pub planned_location: Option<HubRecordId>,
    pub departure: Option<ReturnDepartureClaim>,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct EstablishedReturnTotal {
    pub original_group: HubRecordId,
    pub packs: WholePacks,
}
/// Complete current native lookup, not data supplied by a UI or a reusable grant.
pub struct ReturnContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub departure: Option<&'a DepartureRecord>,
    /// Exactly one total for every original departure line, including explicit zeros.
    pub established_return_totals: &'a [EstablishedReturnTotal],
    pub current_allowances: Option<&'a ReturnAllowanceCapture>,
}
impl ReturnCommand {
    pub fn plan(
        &self,
        context: &ReturnContext<'_>,
        ids: ReturnIdentities,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<ReturnPlan, ReturnError> {
        ids.validate()?;
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
        {
            return Err(ReturnError::StaleRevision);
        }
        if self.packs.value() == 0 {
            return Err(ReturnError::EmptyQuantity);
        }
        self.occurred.validate(now)?;
        let catalog = context.catalog;
        let variant = catalog
            .require_kind(self.variant, ReferenceKind::Pack)?
            .clone();
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(ReturnError::InvalidNativeContext);
        };
        let product = catalog
            .require_kind(product, ReferenceKind::Product)?
            .clone();
        let location = catalog
            .require_kind(self.location, ReferenceKind::Location)?
            .clone();
        let returned_by = self
            .returned_by
            .map(|id| catalog.require_kind(id, ReferenceKind::Party).cloned())
            .transpose()?;
        let source_issuer = match &self.source {
            SourceAttribution::Unknown => None,
            SourceAttribution::Referenced { source, .. } => {
                source.validate(catalog)?;
                source
                    .issuer
                    .map(|id| catalog.require_kind(id, ReferenceKind::Party).cloned())
                    .transpose()?
            }
        };
        if self.planned_location == Some(self.location) {
            return Err(ReturnError::InvalidPlannedLocation);
        }
        let planned_location = self
            .planned_location
            .map(|id| catalog.require_kind(id, ReferenceKind::Location).cloned())
            .transpose()?;
        let intake_work = planned_location.as_ref().map(|target| WorkRequirement {
            id: ids.requirement,
            trace: ids.population,
            variant: self.variant,
            target: Some(target.clone()),
            quantity: QuantityWork::pending(self.packs),
            revision: HubRevision::INITIAL,
        });
        let group = PhysicalStockGroup {
            id: ids.group,
            trace: ids.population,
            variant: self.variant,
            parent: None,
            identification: Some(self.identification.clone()),
            location: self.location,
            packs: self.packs,
            revision: HubRevision::INITIAL,
            causal_floor: match self.occurred {
                Occurrence::Known(at) => Some(at),
                Occurrence::Interval { earliest, .. } => Some(earliest),
                Occurrence::Unknown => None,
            },
            work: intake_work
                .iter()
                .map(|requirement| WorkAllocation {
                    requirement: requirement.clone(),
                    progress: WorkProgress::Pending,
                })
                .collect(),
        };
        group
            .validate()
            .map_err(|_| ReturnError::InvalidNativeContext)?;
        Ok(ReturnPlan {
            identities: ids,
            stock_revision: context
                .stock_revision
                .next()
                .map_err(|_| ReturnError::RevisionExhausted)?,
            catalog_revision: catalog.revision(),
            intake: ReturnIntake {
                population: ids.population,
                product,
                variant,
                location,
                returned_by,
                source: self.source.clone(),
                source_issuer,
                packs: self.packs,
                planned_location,
                occurred: self.occurred,
                actor,
                recorded_at: now,
                observation: self.observation.clone(),
            },
            group,
            intake_work,
            acceptance: AcceptanceStatus::ReviewRequired,
            linkage: self.resolve_link(context)?,
            current_allowances: context.current_allowances.cloned(),
        })
    }
    fn resolve_link(&self, context: &ReturnContext<'_>) -> Result<ReturnLinkSnapshot, ReturnError> {
        let mut snapshot = ReturnLinkSnapshot {
            claim: self.departure,
            departure: None,
            claimed_source: None,
            plausible_sources: Vec::new(),
            timing: ReturnTiming::Uncertain,
            resolution: ReturnLinkResolution::Unknown,
        };
        let Some(claim) = self.departure else {
            if context.departure.is_some()
                || !context.established_return_totals.is_empty()
                || context.current_allowances.is_some()
            {
                return Err(ReturnError::InvalidNativeContext);
            }
            return Ok(snapshot);
        };
        let Some(departure) = context.departure else {
            if !context.established_return_totals.is_empty() || context.current_allowances.is_some()
            {
                return Err(ReturnError::InvalidNativeContext);
            }
            snapshot.resolution = ReturnLinkResolution::Unrecorded;
            return Ok(snapshot);
        };
        departure.validate()?;
        if departure.hub != context.hub
            || departure.id != claim.departure
            || departure.stock_revision.value() > context.stock_revision.value()
            || departure.catalog_revision.value() > context.catalog.revision().value()
        {
            return Err(ReturnError::InvalidNativeContext);
        }
        let issued = departure
            .paper
            .versions
            .last()
            .ok_or(ReturnError::InvalidNativeContext)?;
        if let Some(allowances) = context.current_allowances {
            allowances.validate(context, departure)?;
        }
        if context.established_return_totals.len() != issued.material.lines.len() {
            return Err(ReturnError::InvalidNativeContext);
        }
        let mut totals = BTreeMap::new();
        for total in context.established_return_totals {
            if totals.insert(total.original_group, total.packs).is_some() {
                return Err(ReturnError::InvalidNativeContext);
            }
        }
        if totals.len() != issued.material.lines.len() {
            return Err(ReturnError::InvalidNativeContext);
        }
        for line in &issued.material.lines {
            let prior = totals
                .get(&line.group)
                .ok_or(ReturnError::InvalidNativeContext)?;
            if prior.value() > line.packs.value() {
                return Err(ReturnError::InvalidNativeContext);
            }
            if let Some(allowance) = context.current_allowances.and_then(|capture| {
                capture
                    .lines
                    .iter()
                    .find(|allowance| allowance.original_group == line.group)
            }) && prior.value() > allowance.linkable_quantity().value()
            {
                return Err(ReturnError::InvalidNativeContext);
            }
            let candidate = ReturnSourceSnapshot {
                original: line.clone(),
                previously_established_returns: *prior,
            };
            if line.variant.id == self.variant {
                snapshot.plausible_sources.push(candidate.clone());
            }
            if claim.group == Some(line.group) {
                snapshot.claimed_source = Some(candidate);
            }
        }
        snapshot.departure = Some(ReturnDepartureHeader {
            departure: departure.id,
            operation: departure.operation,
            hub: departure.hub,
            stock_revision: departure.stock_revision,
            catalog_revision: departure.catalog_revision,
            identification: issued.id,
            recipient: issued.material.recipient.clone(),
            actor: departure.actor,
            recorded_at: departure.recorded_at,
            occurred: departure.occurred,
        });
        snapshot.timing = return_timing(departure.occurred, self.occurred);
        let reason = match (&snapshot.claimed_source, claim.group) {
            (_, None) if snapshot.plausible_sources.is_empty() => {
                Some(UnresolvedReturnLink::NoMatchingVariant)
            }
            (_, None) => Some(UnresolvedReturnLink::OriginalGroupUnknown),
            (None, Some(_)) => Some(UnresolvedReturnLink::ClaimedGroupMissing),
            (Some(source), _) if source.original.variant.id != self.variant => {
                Some(UnresolvedReturnLink::VariantConflict)
            }
            (Some(source), _)
                if !source
                    .previously_established_returns
                    .checked_add(self.packs)
                    .is_ok_and(|total| total.value() <= source.original.packs.value()) =>
            {
                Some(UnresolvedReturnLink::QuantityConflict)
            }
            (Some(source), _)
                if context
                    .current_allowances
                    .and_then(|capture| {
                        capture
                            .lines
                            .iter()
                            .find(|allowance| allowance.original_group == source.original.group)
                    })
                    .is_some_and(|allowance| {
                        allowance.unresolved_returns
                            || !source
                                .previously_established_returns
                                .checked_add(self.packs)
                                .is_ok_and(|total| {
                                    total.value() <= allowance.linkable_quantity().value()
                                })
                    }) =>
            {
                Some(UnresolvedReturnLink::QuantityConflict)
            }
            (Some(_), _) if snapshot.timing == ReturnTiming::Contradictory => {
                Some(UnresolvedReturnLink::TimeConflict)
            }
            (Some(_), _) => None,
        };
        snapshot.resolution = reason.map_or(
            ReturnLinkResolution::EstablishedScope,
            ReturnLinkResolution::Unresolved,
        );
        Ok(snapshot)
    }
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, ids: ReturnIdentities) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.return.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        for id in [
            ids.operation,
            ids.returned,
            ids.population,
            ids.group,
            ids.source,
            ids.requirement,
        ] {
            hash.update(id.as_bytes());
        }
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.variant.as_bytes());
        hash.update(self.location.as_bytes());
        hash.update(self.packs.value().to_be_bytes());
        hash_optional_id(&mut hash, self.returned_by);
        hash_optional_id(&mut hash, self.planned_location);
        hash_source(&mut hash, &self.source);
        hash_text(&mut hash, self.identification.label.as_str());
        hash_text(&mut hash, self.identification.observation.as_str());
        hash.update([u8::from(self.departure.is_some())]);
        if let Some(claim) = self.departure {
            hash.update(claim.departure.as_bytes());
            hash_optional_id(&mut hash, claim.group);
        }
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
fn return_timing(departure: Occurrence, returned: Occurrence) -> ReturnTiming {
    let bounds = |value| match value {
        Occurrence::Known(at) => Some((at, at)),
        Occurrence::Interval { earliest, latest } => Some((earliest, latest)),
        Occurrence::Unknown => None,
    };
    match (bounds(departure), bounds(returned)) {
        (Some((first, _)), Some((_, last))) if last < first => ReturnTiming::Contradictory,
        (Some((_, last)), Some((first, _))) if first >= last => ReturnTiming::AfterOrSame,
        _ => ReturnTiming::Uncertain,
    }
}
