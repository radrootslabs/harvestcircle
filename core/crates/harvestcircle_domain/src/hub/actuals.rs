//! Preserved reports of already-taken departures, separate from planned actions.
//!
//! A report is unverified evidence, not a physical effect or permission. Review
//! comparison is checked data only: current native authority, complete duplicate
//! checks and atomic persistence must precede any later balanced reconciliation.
//! No report, historical restriction finding or archived review is a stock grant.

use super::movement::{hash_occurrence, hash_text};
use super::reference::{HubRecordId, SourceProvenance, WholePacks};
use super::stock::Occurrence;
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

mod hold_scope;
mod reconcile;
mod review;
pub use hold_scope::{
    ActualPhysicalBirth, ActualPhysicalBirthKind, HistoricalHoldScopeAssessment,
    HistoricalHoldScopeContext, HistoricalHoldScopeUncertainty, VerifiedHistoricalHoldScope,
};
mod source;
pub use reconcile::{
    ActualDuplicateLink, ActualDuplicateLinkPlan, ActualKnownDeparture, ActualKnownDepartureLine,
    ActualNewDepartureCommand, ActualNewDepartureContext, ActualNewDeparturePlan,
    ActualPhysicalReduction, ActualRemovedWorkScope,
};
pub use review::{
    ActualBalanceComparison, ActualReviewComparison, ActualReviewContext, ActualReviewFacts,
    ActualSourceComparison, HistoricalHoldApplicability, HistoricalHoldFinding,
    HistoricalHoldTiming, classify_hold_timing,
};
pub use source::{
    ActualPhysicalActivity, ActualPhysicalActivityKind, ActualSourceContext,
    HistoricalActualSource, ResolvedActualSource,
};

pub const HUB_ACTUAL_REPORT_SOURCE_CANDIDATES: usize = 64;
pub const HUB_ACTUAL_REPORT_CAPACITY: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActualReportError {
    Capacity,
    InvalidIdentity,
    InvalidInput,
    InvalidNativeContext,
    InvalidHistory,
    StaleRevision,
    RevisionExhausted,
    ConflictingEvidence,
    InsufficientBalance,
    UncertainSubmission,
    HistoricalSourceUnresolved,
    DuplicateReviewRequired,
    AlreadyReviewed,
}

/// Reported identifiers remain claims. Their existence or a selected row does
/// not establish historical attribution. Unknown fields are never filled in.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReportedActualSource {
    pub trace: HubRecordId,
    pub group: Option<HubRecordId>,
    pub location: Option<HubRecordId>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActualReportSources {
    Unknown,
    Possible(Vec<ReportedActualSource>),
    EstablishedClaim(ReportedActualSource),
}

impl ActualReportSources {
    pub fn validate(&self) -> Result<(), ActualReportError> {
        if let Self::Possible(candidates) = self {
            if candidates.is_empty() {
                return Err(ActualReportError::InvalidInput);
            }
            if candidates.len() > HUB_ACTUAL_REPORT_SOURCE_CANDIDATES {
                return Err(ActualReportError::Capacity);
            }
            let mut seen = BTreeSet::new();
            for c in candidates {
                if !seen.insert((c.trace, c.group, c.location)) {
                    return Err(ActualReportError::InvalidInput);
                }
            }
        }
        Ok(())
    }

    /// Exact supported claim scope for trace retrieval; an unknown source must
    /// use conservative native retrieval rather than treating this as empty.
    #[must_use]
    pub fn candidates(&self) -> &[ReportedActualSource] {
        match self {
            Self::Unknown => &[],
            Self::Possible(values) => values,
            Self::EstablishedClaim(value) => std::slice::from_ref(value),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualReportCommand {
    pub variant: Option<HubRecordId>,
    pub recipient: Option<HubRecordId>,
    pub packs: Option<WholePacks>,
    pub sources: ActualReportSources,
    pub occurred: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ActualReportRecord {
    pub id: HubRecordId,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub report: ActualReportCommand,
}

pub fn actual_report_id(operation: HubRecordId) -> Result<HubRecordId, ActualReportError> {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.actual.report.identity.v1\0");
    hash.update(operation.as_bytes());
    HubRecordId::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 prefix"))
        .map_err(|_| ActualReportError::InvalidIdentity)
}

impl ActualReportCommand {
    pub fn validate(&self, recorded_at: UnixTimestamp) -> Result<(), ActualReportError> {
        self.sources.validate()?;
        if self.packs.is_some_and(|v| v.value() == 0) {
            return Err(ActualReportError::InvalidInput);
        }
        self.occurred
            .validate(recorded_at)
            .map_err(|_| ActualReportError::InvalidInput)
    }

    /// Does not use acceptance, current holds or balance as an ingress filter.
    /// Missing quantity/source/recipient and unknown time remain reportable.
    pub fn preserve(
        &self,
        hub: HubId,
        stock_revision: HubRevision,
        catalog_revision: HubRevision,
        operation: HubRecordId,
        actor: PublicKey,
        recorded_at: UnixTimestamp,
    ) -> Result<ActualReportRecord, ActualReportError> {
        self.validate(recorded_at)?;
        let record = ActualReportRecord {
            id: actual_report_id(operation)?,
            operation,
            hub,
            stock_revision: stock_revision
                .next()
                .map_err(|_| ActualReportError::RevisionExhausted)?,
            catalog_revision,
            actor,
            recorded_at,
            report: self.clone(),
        };
        record.validate()?;
        Ok(record)
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, operation: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.actual.report.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(operation.as_bytes());
        let optional = |hash: &mut Sha256, id: Option<HubRecordId>| {
            hash.update([u8::from(id.is_some())]);
            if let Some(id) = id {
                hash.update(id.as_bytes());
            }
        };
        optional(&mut hash, self.variant);
        optional(&mut hash, self.recipient);
        hash.update([u8::from(self.packs.is_some())]);
        if let Some(packs) = self.packs {
            hash.update(packs.value().to_be_bytes());
        }
        hash.update([match &self.sources {
            ActualReportSources::Unknown => 0,
            ActualReportSources::Possible(_) => 1,
            ActualReportSources::EstablishedClaim(_) => 2,
        }]);
        hash.update((self.sources.candidates().len() as u64).to_be_bytes());
        for source in self.sources.candidates() {
            hash.update(source.trace.as_bytes());
            optional(&mut hash, source.group);
            optional(&mut hash, source.location);
        }
        hash_occurrence(&mut hash, self.occurred);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

impl ActualReportRecord {
    pub fn validate(&self) -> Result<(), ActualReportError> {
        if self.id != actual_report_id(self.operation)?
            || self.id == self.operation
            || self.stock_revision == HubRevision::INITIAL
        {
            return Err(ActualReportError::InvalidHistory);
        }
        self.report.validate(self.recorded_at)
    }
}

#[cfg(test)]
mod tests;
