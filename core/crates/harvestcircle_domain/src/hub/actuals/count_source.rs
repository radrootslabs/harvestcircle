//! Original-derived count source data. Native storage must independently verify
//! the full canonical explanation, its captured scope and original result. This
//! type has no decoder and neither its construction nor digest creates a grant.
use super::*;
use crate::hub::{
    corrections::CountExplanationPlan,
    counts::{CountEffect, CountPostPlan},
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CountExplanationSourceWitness {
    known_digest: [u8; 32],
    count_digest: [u8; 32],
    source: ResolvedActualSource,
}
impl CountExplanationSourceWitness {
    pub fn from_explanation(value: &CountExplanationPlan) -> Result<Self, ActualReportError> {
        let known = &value.known_event;
        let count = &value.count;
        let source = value.historical_source.historical();
        known.validate()?;
        count
            .validate()
            .map_err(|_| ActualReportError::InvalidHistory)?;
        source.validate()?;
        value.report.validate()?;
        let CountEffect::Shortage { packs, .. } = count.effect else {
            return Err(ActualReportError::InvalidHistory);
        };
        let latest = match known.occurred {
            Occurrence::Known(at) => at,
            Occurrence::Interval { latest, .. } => latest,
            Occurrence::Unknown => return Err(ActualReportError::HistoricalSourceUnresolved),
        };
        if known.operation != value.operation
            || known.operation != value.explanation.operation
            || known.actor != value.actor
            || known.recorded_at != value.recorded_at
            || known.stock_revision != value.explanation.stock_revision
            || value.facts.expected_stock_revision.value().checked_add(1)
                != Some(known.stock_revision.value())
            || known.catalog_revision != value.facts.expected_catalog_revision
            || known.hub != count.count.scope.hub
            || known.trace != count.before.trace
            || known.variant.id != count.before.variant
            || known.lines.len() != 1
            || known.lines[0].group != count.before.id
            || known.lines[0].location != source.location
            || known.lines[0].packs != value.explanation.packs
            || value.explanation.packs != value.facts.packs
            || value.explanation.packs.value() == 0
            || value.explanation.packs.value() > packs.value()
            || value.facts.count_reconciliation != count.id
            || value.facts.recipient != known.recipient.id
            || value.facts.occurred != known.occurred
            || value.facts.report != value.report.id
            || value.report.hub != known.hub
            || value.report.recorded_at > known.recorded_at
            || value.report.stock_revision.value() > value.facts.expected_stock_revision.value()
            || count.stock_revision.value() >= known.stock_revision.value()
            || count.catalog_revision.value() > known.catalog_revision.value()
            || count.recorded_at > known.recorded_at
            || latest > count.confirmed_on_hand_at
            || source.hub != known.hub
            || source.group.trace != known.trace
            || source.group.variant != known.variant.id
            || source.stock_revision.value() > count.count.scope.stock_revision.value()
            || source.catalog_revision.value() > count.catalog_revision.value()
            || source.recorded_at > count.recorded_at
            || value.historical_source.balancing_group() != count.before.id
            || [
                known.operation,
                count.operation,
                count.count.operation,
                value.report.operation,
            ]
            .contains(&source.operation)
        {
            return Err(ActualReportError::InvalidHistory);
        }
        let raw = &value.report.report;
        let matches = |claim: &ReportedActualSource| {
            claim.trace == source.group.trace
                && claim.group.is_none_or(|id| id == source.group.id)
                && claim.location.is_none_or(|id| id == source.location.id)
        };
        if raw.variant.is_some_and(|id| id != known.variant.id)
            || raw.recipient.is_some_and(|id| id != known.recipient.id)
            || raw
                .packs
                .is_some_and(|packs| packs != value.explanation.packs)
            || !reported_occurrence_refines(raw.occurred, known.occurred)
            || (!matches!(&raw.sources, ActualReportSources::Unknown)
                && !raw.sources.candidates().iter().any(matches))
        {
            return Err(ActualReportError::ConflictingEvidence);
        }
        Ok(Self {
            known_digest: super::result::known_original_digest(known)?,
            count_digest: crate::hub::counts::count_post_result_digest(&[0; 32], count)
                .map_err(|_| ActualReportError::InvalidHistory)?,
            source: value.historical_source.clone(),
        })
    }
    pub fn validate_for(
        &self,
        known: &ActualKnownDeparture,
        count: &CountPostPlan,
    ) -> Result<(), ActualReportError> {
        if self.known_digest != super::result::known_original_digest(known)?
            || self.count_digest
                != crate::hub::counts::count_post_result_digest(&[0; 32], count)
                    .map_err(|_| ActualReportError::InvalidHistory)?
            || self.source.balancing_group() != count.before.id
        {
            return Err(ActualReportError::InvalidHistory);
        }
        Ok(())
    }
    #[must_use]
    pub const fn historical(&self) -> &HistoricalActualSource {
        self.source.historical()
    }
    /// Exact original position only. No current-ancestry or same-trace fallback.
    #[must_use]
    pub fn matches_source(&self, claim: &ReportedActualSource) -> bool {
        let source = self.source.historical();
        claim.trace == source.group.trace
            && claim.group.is_none_or(|id| id == source.group.id)
            && claim.location.is_none_or(|id| id == source.location.id)
    }
    pub fn digest(&self) -> Result<[u8; 32], ActualReportError> {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.count.explanation.source_witness.v1\0");
        hash.update(self.known_digest);
        hash.update(self.count_digest);
        super::result::hash_source(&mut hash, self.source.historical())?;
        hash.update(self.source.balancing_group().as_bytes());
        Ok(hash.finalize().into())
    }
}

pub struct ActualCountDuplicateContext<'a> {
    pub interpretation: &'a ActualKnownInterpretation,
    /// Derived from the independently corroborated full native explanation.
    pub source: &'a CountExplanationSourceWitness,
}
