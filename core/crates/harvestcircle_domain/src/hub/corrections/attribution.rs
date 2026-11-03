//! Immutable attribution amendments. Native source readers, current authority,
//! durable admission and atomic persistence must protect these pure plans.
use super::HUB_CORRECTION_ROWS;
use crate::hub::reference::{
    HubRecordId, ReferenceCatalog, ReferenceKind, ReferenceRecord, SourceAttribution,
    SourceProvenance,
};
use crate::hub::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use std::collections::BTreeSet;

mod follow_up;
pub use follow_up::*;
mod source;
pub use source::*;
mod recipient;
pub use recipient::*;

pub const HUB_CORRECTION_IMPACTS: usize = 4_096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributionError {
    Capacity,
    InvalidHistory,
    InvalidReference,
    SourceIdentityConflict,
    StaleRevision,
    RevisionExhausted,
    UncertainSubmission,
    NoChange,
    InvalidScope,
}

/// A bounded native cut. A pure plan never establishes owner/credential authority.
pub struct AttributionCut<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub latest_recorded_at: UnixTimestamp,
    pub uncertain_submissions: bool,
}
impl AttributionCut<'_> {
    fn check(
        &self,
        expected: HubRevision,
        catalog: HubRevision,
        now: UnixTimestamp,
    ) -> Result<HubRevision, AttributionError> {
        if self.uncertain_submissions {
            return Err(AttributionError::UncertainSubmission);
        }
        if expected != self.stock_revision || catalog != self.catalog.revision() {
            return Err(AttributionError::StaleRevision);
        }
        if now < self.latest_recorded_at {
            return Err(AttributionError::InvalidHistory);
        }
        self.stock_revision
            .next()
            .map_err(|_| AttributionError::RevisionExhausted)
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
pub enum CorrectionImpact {
    Population(HubRecordId),
    Source(HubRecordId),
    Departure(HubRecordId),
    IssuedVersion(HubRecordId),
    Recipient(HubRecordId),
    UnresolvedSource(HubRecordId),
    UnresolvedRecipient(HubRecordId),
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectionImpactSet(Vec<CorrectionImpact>);
impl CorrectionImpactSet {
    pub fn new(mut rows: Vec<CorrectionImpact>) -> Result<Self, AttributionError> {
        if rows.is_empty() || rows.len() > HUB_CORRECTION_IMPACTS {
            return Err(AttributionError::Capacity);
        }
        rows.sort_unstable();
        if rows.windows(2).any(|w| w[0] == w[1]) {
            return Err(AttributionError::InvalidHistory);
        }
        Ok(Self(rows))
    }
    #[must_use]
    pub fn rows(&self) -> &[CorrectionImpact] {
        &self.0
    }
    #[must_use]
    pub fn contains(&self, target: CorrectionImpact) -> bool {
        self.0.binary_search(&target).is_ok()
    }
}
#[derive(Clone)]
struct ImpactBuilder(BTreeSet<CorrectionImpact>);
impl ImpactBuilder {
    fn new() -> Self {
        Self(BTreeSet::new())
    }
    fn add(&mut self, target: CorrectionImpact) -> Result<(), AttributionError> {
        self.0.insert(target);
        if self.0.len() > HUB_CORRECTION_IMPACTS {
            return Err(AttributionError::Capacity);
        }
        Ok(())
    }
    fn source(
        &mut self,
        trace: HubRecordId,
        source: &SourceAttribution,
    ) -> Result<(), AttributionError> {
        match source {
            SourceAttribution::Unknown => self.add(CorrectionImpact::UnresolvedSource(trace)),
            SourceAttribution::Referenced { source, certainty } => {
                self.add(CorrectionImpact::Source(source.id))?;
                if source.issuer.is_none()
                    || *certainty == crate::hub::reference::SourceCertainty::ReportedPossible
                {
                    self.add(CorrectionImpact::UnresolvedSource(trace))?;
                }
                Ok(())
            }
        }
    }
    fn recipient(
        &mut self,
        event: HubRecordId,
        recipient: &AmendedRecipient,
    ) -> Result<(), AttributionError> {
        match recipient {
            AmendedRecipient::Unknown => self.add(CorrectionImpact::UnresolvedRecipient(event)),
            AmendedRecipient::Known(r) => self.add(CorrectionImpact::Recipient(r.id)),
        }
    }
    fn finish(self) -> Result<CorrectionImpactSet, AttributionError> {
        CorrectionImpactSet::new(self.0.into_iter().collect())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AttributionStamp {
    pub operation: HubRecordId,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub basis: SourceProvenance,
}

fn party(record: &ReferenceRecord, catalog: HubRevision) -> Result<(), AttributionError> {
    if record.value.kind() != ReferenceKind::Party || record.revision.value() > catalog.value() {
        return Err(AttributionError::InvalidReference);
    }
    Ok(())
}
fn check_stamp(
    stamp: &AttributionStamp,
    hub: HubId,
    previous: HubRevision,
    catalog: HubRevision,
    at: UnixTimestamp,
) -> Result<(), AttributionError> {
    if stamp.hub != hub
        || stamp.stock_revision.value() <= previous.value()
        || stamp.catalog_revision.value() < catalog.value()
        || stamp.recorded_at < at
    {
        return Err(AttributionError::InvalidHistory);
    }
    Ok(())
}

/// Complete affected native event scope. All prior recipient interpretations and
/// every issued version are retained; a selected line/subtotal is insufficient.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AttributionAffectedDeparture {
    pub operation: HubRecordId,
    pub populations: Vec<HubRecordId>,
    pub recipients: Vec<AmendedRecipient>,
    pub issued_versions: Vec<HubRecordId>,
}
impl AttributionAffectedDeparture {
    fn impact(
        &self,
        targets: &mut ImpactBuilder,
        catalog: HubRevision,
    ) -> Result<(), AttributionError> {
        if self.populations.is_empty()
            || self.populations.len() > crate::hub::dispatch::HUB_DISPATCH_LINES
            || self.recipients.is_empty()
            || self.recipients.len() > HUB_CORRECTION_ROWS + 1
            || self.issued_versions.len() > crate::hub::dispatch::HUB_IDENTIFICATION_VERSIONS
        {
            return Err(AttributionError::Capacity);
        }
        if self.populations.iter().collect::<BTreeSet<_>>().len() != self.populations.len()
            || self.issued_versions.iter().collect::<BTreeSet<_>>().len()
                != self.issued_versions.len()
        {
            return Err(AttributionError::InvalidHistory);
        }
        targets.add(CorrectionImpact::Departure(self.operation))?;
        for trace in &self.populations {
            targets.add(CorrectionImpact::Population(*trace))?;
        }
        for recipient in &self.recipients {
            recipient.validate(catalog)?;
            targets.recipient(self.operation, recipient)?;
        }
        for paper in &self.issued_versions {
            targets.add(CorrectionImpact::IssuedVersion(*paper))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
