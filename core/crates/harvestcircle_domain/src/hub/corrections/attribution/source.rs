use super::*;
use crate::hub::reference::HUB_REFERENCE_HISTORY_CAPACITY;
use crate::hub::stock::validate_source_reuse;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AmendedSource {
    pub source: SourceAttribution,
    pub issuer: Option<ReferenceRecord>,
}
impl AmendedSource {
    pub fn validate(&self, catalog: HubRevision) -> Result<(), AttributionError> {
        match (&self.source, &self.issuer) {
            (SourceAttribution::Unknown, None) => Ok(()),
            (SourceAttribution::Referenced { source, .. }, None) if source.issuer.is_none() => {
                Ok(())
            }
            (SourceAttribution::Referenced { source, .. }, Some(issuer))
                if source.issuer == Some(issuer.id) =>
            {
                party(issuer, catalog)
            }
            _ => Err(AttributionError::InvalidReference),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OriginalSourceAttribution {
    pub hub: HubId,
    pub operation: HubRecordId,
    pub trace: HubRecordId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
    pub actor: PublicKey,
    pub source: AmendedSource,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct SourceAttributionCorrection {
    pub stamp: AttributionStamp,
    pub original: OriginalSourceAttribution,
    pub prior: Option<HubRecordId>,
    pub before: AmendedSource,
    pub after: AmendedSource,
    pub impact: CorrectionImpactSet,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct SourceAttributionCommand {
    pub trace: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_prior: Option<HubRecordId>,
    pub source: SourceAttribution,
    pub basis: SourceProvenance,
}
pub struct SourceAttributionContext<'a> {
    pub cut: AttributionCut<'a>,
    pub original: &'a OriginalSourceAttribution,
    pub history: &'a [SourceAttributionCorrection],
    /// Complete immutable source registry, including prior amendment identities.
    pub known_sources: &'a [SourceAttribution],
    /// Complete downstream events supplied by native bounded readers, not a UI selection.
    pub affected: &'a [AttributionAffectedDeparture],
}

/// Returns frozen current interpretation plus every original/amended association.
/// No native current authority or affected-scope completeness follows from this.
pub fn current_source_attribution<'a>(
    original: &'a OriginalSourceAttribution,
    history: &'a [SourceAttributionCorrection],
) -> Result<&'a AmendedSource, AttributionError> {
    if history.len() > HUB_CORRECTION_ROWS {
        return Err(AttributionError::Capacity);
    }
    original.source.validate(original.catalog_revision)?;
    if original.stock_revision == HubRevision::INITIAL {
        return Err(AttributionError::InvalidHistory);
    }
    let mut current = &original.source;
    let mut prior = None;
    let mut revision = original.stock_revision;
    let mut catalog = original.catalog_revision;
    let mut at = original.recorded_at;
    let mut operations = BTreeSet::from([original.operation]);
    let mut identities = std::collections::BTreeMap::new();
    if let SourceAttribution::Referenced { source, .. } = &original.source.source {
        identities.insert(source.id, source.clone());
    }
    let mut required = ImpactBuilder::new();
    required.add(CorrectionImpact::Population(original.trace))?;
    required.source(original.trace, &original.source.source)?;
    for record in history {
        check_stamp(&record.stamp, original.hub, revision, catalog, at)?;
        if record.original != *original
            || record.prior != prior
            || record.before != *current
            || record.after.source == record.before.source
            || !record
                .impact
                .contains(CorrectionImpact::Population(original.trace))
            || !operations.insert(record.stamp.operation)
        {
            return Err(AttributionError::InvalidHistory);
        }
        record.after.validate(record.stamp.catalog_revision)?;
        validate_source_reuse(
            &record.after.source,
            [record.before.source.clone(), original.source.source.clone()],
        )
        .map_err(|_| AttributionError::SourceIdentityConflict)?;
        if let SourceAttribution::Referenced { source, .. } = &record.after.source
            && identities
                .insert(source.id, source.clone())
                .is_some_and(|old| old != *source)
        {
            return Err(AttributionError::SourceIdentityConflict);
        }
        required.source(original.trace, &record.after.source)?;
        if required
            .0
            .iter()
            .any(|target| !record.impact.contains(*target))
        {
            return Err(AttributionError::InvalidHistory);
        }
        for target in record.impact.rows() {
            required.add(*target)?;
        }
        current = &record.after;
        prior = Some(record.stamp.operation);
        revision = record.stamp.stock_revision;
        catalog = record.stamp.catalog_revision;
        at = record.stamp.recorded_at;
    }
    Ok(current)
}
impl SourceAttributionCommand {
    pub fn plan(
        &self,
        context: &SourceAttributionContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<SourceAttributionCorrection, AttributionError> {
        let next = context.cut.check(
            self.expected_stock_revision,
            self.expected_catalog_revision,
            now,
        )?;
        if context.history.len() >= HUB_CORRECTION_ROWS
            || context.known_sources.len() > HUB_REFERENCE_HISTORY_CAPACITY
            || context.affected.len() > HUB_CORRECTION_ROWS
        {
            return Err(AttributionError::Capacity);
        }
        if self.trace != context.original.trace || context.original.hub != context.cut.hub {
            return Err(AttributionError::InvalidScope);
        }
        let before = current_source_attribution(context.original, context.history)?;
        let last = context.history.last();
        if self.expected_prior != last.map(|r| r.stamp.operation) {
            return Err(AttributionError::StaleRevision);
        }
        let last_revision =
            last.map_or(context.original.stock_revision, |r| r.stamp.stock_revision);
        let last_at = last.map_or(context.original.recorded_at, |r| r.stamp.recorded_at);
        let last_catalog = last.map_or(context.original.catalog_revision, |r| {
            r.stamp.catalog_revision
        });
        if last_revision.value() > context.cut.stock_revision.value()
            || last_catalog.value() > context.cut.catalog.revision().value()
            || now < last_at
            || operation == context.original.operation
            || context
                .history
                .iter()
                .any(|r| r.stamp.operation == operation)
        {
            return Err(AttributionError::InvalidHistory);
        }
        if let SourceAttribution::Referenced { source, .. } = &self.source {
            source
                .validate(context.cut.catalog)
                .map_err(|_| AttributionError::InvalidReference)?;
        }
        validate_source_reuse(
            &self.source,
            context
                .known_sources
                .iter()
                .cloned()
                .chain(std::iter::once(context.original.source.source.clone()))
                .chain(context.history.iter().map(|r| r.after.source.clone())),
        )
        .map_err(|_| AttributionError::SourceIdentityConflict)?;
        let issuer = match &self.source {
            SourceAttribution::Referenced { source, .. } => source
                .issuer
                .map(|id| {
                    context
                        .cut
                        .catalog
                        .require_kind(id, ReferenceKind::Party)
                        .cloned()
                        .map_err(|_| AttributionError::InvalidReference)
                })
                .transpose()?,
            SourceAttribution::Unknown => None,
        };
        let after = AmendedSource {
            source: self.source.clone(),
            issuer,
        };
        after.validate(context.cut.catalog.revision())?;
        if before.source == after.source {
            return Err(AttributionError::NoChange);
        }
        let mut impact = ImpactBuilder::new();
        impact.add(CorrectionImpact::Population(self.trace))?;
        impact.source(self.trace, &context.original.source.source)?;
        for record in context.history {
            impact.source(self.trace, &record.after.source)?;
        }
        impact.source(self.trace, &after.source)?;
        let mut events = BTreeSet::new();
        let mut downstream = ImpactBuilder::new();
        downstream.add(CorrectionImpact::Population(self.trace))?;
        for event in context.affected {
            if !event.populations.contains(&self.trace)
                || !events.insert(event.operation)
                || event.operation == operation
            {
                return Err(AttributionError::InvalidScope);
            }
            event.impact(&mut downstream, context.cut.catalog.revision())?;
        }
        for prior in context.history {
            for target in prior.impact.rows() {
                if matches!(
                    target,
                    CorrectionImpact::Population(_)
                        | CorrectionImpact::Departure(_)
                        | CorrectionImpact::IssuedVersion(_)
                        | CorrectionImpact::Recipient(_)
                        | CorrectionImpact::UnresolvedRecipient(_)
                ) && !downstream.0.contains(target)
                {
                    return Err(AttributionError::InvalidScope);
                }
                impact.add(*target)?;
            }
        }
        for target in downstream.0 {
            impact.add(target)?;
        }
        Ok(SourceAttributionCorrection {
            stamp: AttributionStamp {
                operation,
                hub: context.cut.hub,
                stock_revision: next,
                catalog_revision: context.cut.catalog.revision(),
                actor,
                recorded_at: now,
                basis: self.basis.clone(),
            },
            original: context.original.clone(),
            prior: self.expected_prior,
            before: before.clone(),
            after,
            impact: impact.finish()?,
        })
    }
}
