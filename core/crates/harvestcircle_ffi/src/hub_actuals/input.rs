//! Bounded transport of claims and explicit review facts; all decisions stay native.
use crate::commands::HarvestCircleError;
use crate::hub_reference::invalid_input;
use crate::hub_stock::HubOccurrenceDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::{
    actuals::{
        ActualDuplicateLink, ActualNewDepartureCommand, ActualReportCommand, ActualReportSources,
        ActualReviewFacts, HUB_ACTUAL_REPORT_SOURCE_CANDIDATES, ReportedActualSource,
    },
    reference::{HubRecordId, SourceProvenance, WholePacks},
};
use harvestcircle_storage::{HubActualReportFilter, HubActualReportQuery};

pub(super) fn operation_id(value: &str) -> Result<HubRecordId, HarvestCircleError> {
    let parsed = DurableRequestId::parse(value).map_err(|_| invalid_input())?;
    HubRecordId::from_hex(&parsed.as_str().replace('-', "")).map_err(|_| invalid_input())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualSourceKindDto {
    Unknown,
    Possible,
    EstablishedClaim,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualSourceClaimDto {
    pub trace_id: String,
    pub group_id: Option<String>,
    pub location_id: Option<String>,
}
impl HubActualSourceClaimDto {
    fn decode(self) -> Result<ReportedActualSource, HarvestCircleError> {
        Ok(ReportedActualSource {
            trace: id(&self.trace_id)?,
            group: self.group_id.as_deref().map(id).transpose()?,
            location: self.location_id.as_deref().map(id).transpose()?,
        })
    }
}
impl From<&ReportedActualSource> for HubActualSourceClaimDto {
    fn from(value: &ReportedActualSource) -> Self {
        Self {
            trace_id: value.trace.to_hex(),
            group_id: value.group.map(|v| v.to_hex()),
            location_id: value.location.map(|v| v.to_hex()),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReportInputDto {
    pub variant_id: Option<String>,
    pub recipient_id: Option<String>,
    pub whole_packs: Option<u32>,
    pub source_kind: HubActualSourceKindDto,
    pub source_claims: Vec<HubActualSourceClaimDto>,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubActualReportInputDto {
    pub fn command(self) -> Result<ActualReportCommand, HarvestCircleError> {
        if self.source_claims.len() > HUB_ACTUAL_REPORT_SOURCE_CANDIDATES {
            return Err(invalid_input());
        }
        let mut claims = self
            .source_claims
            .into_iter()
            .map(HubActualSourceClaimDto::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let sources = match self.source_kind {
            HubActualSourceKindDto::Unknown if claims.is_empty() => ActualReportSources::Unknown,
            HubActualSourceKindDto::Possible if !claims.is_empty() => {
                ActualReportSources::Possible(claims)
            }
            HubActualSourceKindDto::EstablishedClaim if claims.len() == 1 => {
                ActualReportSources::EstablishedClaim(claims.remove(0))
            }
            _ => return Err(invalid_input()),
        };
        let command = ActualReportCommand {
            variant: self.variant_id.as_deref().map(id).transpose()?,
            recipient: self.recipient_id.as_deref().map(id).transpose()?,
            packs: self
                .whole_packs
                .map(WholePacks::from_value)
                .transpose()
                .map_err(|_| invalid_input())?,
            sources,
            occurred: self.occurrence.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        };
        command.validate().map_err(|_| invalid_input())?;
        Ok(command)
    }
}
impl From<&ActualReportCommand> for HubActualReportInputDto {
    fn from(value: &ActualReportCommand) -> Self {
        Self {
            variant_id: value.variant.map(|v| v.to_hex()),
            recipient_id: value.recipient.map(|v| v.to_hex()),
            whole_packs: value.packs.map(|packs| packs.value()),
            source_kind: match value.sources {
                ActualReportSources::Unknown => HubActualSourceKindDto::Unknown,
                ActualReportSources::Possible(_) => HubActualSourceKindDto::Possible,
                ActualReportSources::EstablishedClaim(_) => {
                    HubActualSourceKindDto::EstablishedClaim
                }
            },
            source_claims: value.sources.candidates().iter().map(Into::into).collect(),
            occurrence: value.occurred.into(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualDepartureInputDto {
    pub report_id: String,
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_id: String,
    pub expected_group_revision: u64,
    pub whole_packs: u32,
    pub recipient_id: String,
    pub occurrence: HubOccurrenceDto,
    pub source_operation_id: String,
    pub source_group_id: String,
    pub source_evidence: String,
    pub quantity_evidence: String,
    pub recipient_evidence: String,
    pub timing_evidence: String,
    pub duplicate_review_basis: String,
}
impl HubActualDepartureInputDto {
    pub fn command(self) -> Result<ActualNewDepartureCommand, HarvestCircleError> {
        let note = |text| SourceProvenance::new(text).map_err(|_| invalid_input());
        Ok(ActualNewDepartureCommand {
            facts: ActualReviewFacts {
                report: id(&self.report_id)?,
                expected_stock_revision: revision(self.expected_stock_revision)?,
                expected_catalog_revision: revision(self.expected_catalog_revision)?,
                group: id(&self.group_id)?,
                expected_group_revision: revision(self.expected_group_revision)?,
                packs: WholePacks::from_value(self.whole_packs).map_err(|_| invalid_input())?,
                recipient: id(&self.recipient_id)?,
                occurred: self.occurrence.decode()?,
                source_evidence: note(self.source_evidence)?,
                quantity_evidence: note(self.quantity_evidence)?,
                recipient_evidence: note(self.recipient_evidence)?,
                timing_evidence: note(self.timing_evidence)?,
            },
            source_operation: operation_id(&self.source_operation_id)?,
            source_group: id(&self.source_group_id)?,
            duplicate_review_basis: note(self.duplicate_review_basis)?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualLinkInputDto {
    pub report_id: String,
    pub known_operation_id: String,
    pub known_trace_id: String,
    pub known_variant_id: String,
    pub verification: String,
}
impl HubActualLinkInputDto {
    pub fn command(self) -> Result<ActualDuplicateLink, HarvestCircleError> {
        Ok(ActualDuplicateLink {
            report: id(&self.report_id)?,
            known_operation: operation_id(&self.known_operation_id)?,
            known_trace: id(&self.known_trace_id)?,
            known_variant: id(&self.known_variant_id)?,
            verification: SourceProvenance::new(self.verification).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubActualReportFilterDto {
    Unresolved,
    All,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubActualReportQueryDto {
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub filter: HubActualReportFilterDto,
}
impl HubActualReportQueryDto {
    pub fn decode(self) -> Result<HubActualReportQuery, HarvestCircleError> {
        if self.limit == 0
            || self.limit > 64
            || self.offset > 4096
            || (self.offset > 0
                && (self.expected_stock_revision.is_none()
                    || self.expected_catalog_revision.is_none()))
        {
            return Err(invalid_input());
        }
        Ok(HubActualReportQuery {
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
            filter: match self.filter {
                HubActualReportFilterDto::Unresolved => HubActualReportFilter::Unresolved,
                HubActualReportFilterDto::All => HubActualReportFilter::All,
            },
        })
    }
}
