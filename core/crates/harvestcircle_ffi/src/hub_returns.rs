//! Returned custody has a separate origin and explicit unresolved historical linkage.
use crate::commands::HarvestCircleError;
use crate::hub_dispatch::HubDispatchLineDto;
use crate::hub_handling::{
    HubPhysicalGroupDto, HubPhysicalIdentificationDto, HubWorkRequirementDto,
};
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::{HubOccurrenceDto, HubSourceInputDto, HubStockPopulationDto};
use crate::hub_treatment::HubAcceptanceStatusDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::{
    reference::{SourceProvenance, WholePacks},
    returns::*,
};
use harvestcircle_storage::HubReturnReceipt;
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnClaimDto {
    pub departure_id: String,
    pub original_group_id: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub variant_id: String,
    pub location_id: String,
    pub returned_by_id: Option<String>,
    pub source: HubSourceInputDto,
    pub whole_packs: u32,
    pub identification: HubPhysicalIdentificationDto,
    pub planned_location_id: Option<String>,
    pub departure: Option<HubReturnClaimDto>,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubReturnInputDto {
    pub fn command(
        self,
        operation: &DurableRequestId,
    ) -> Result<ReturnCommand, HarvestCircleError> {
        let ids = ReturnIdentities::from_operation_bytes(id(&operation.as_str().replace('-', ""))?)
            .map_err(|_| invalid_input())?;
        let packs = WholePacks::from_value(self.whole_packs).map_err(|_| invalid_input())?;
        if packs.value() == 0 {
            return Err(invalid_input());
        }
        Ok(ReturnCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            variant: id(&self.variant_id)?,
            location: id(&self.location_id)?,
            returned_by: self.returned_by_id.as_deref().map(id).transpose()?,
            source: self.source.decode(ids.source)?,
            packs,
            identification: self.identification.decode()?,
            planned_location: self.planned_location_id.as_deref().map(id).transpose()?,
            departure: self
                .departure
                .map(|claim| {
                    Ok::<_, HarvestCircleError>(ReturnDepartureClaim {
                        departure: id(&claim.departure_id)?,
                        group: claim.original_group_id.as_deref().map(id).transpose()?,
                    })
                })
                .transpose()?,
            occurred: self.occurrence.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubReturnTimingDto {
    AfterOrSame,
    Uncertain,
    Contradictory,
}
impl From<ReturnTiming> for HubReturnTimingDto {
    fn from(value: ReturnTiming) -> Self {
        match value {
            ReturnTiming::AfterOrSame => Self::AfterOrSame,
            ReturnTiming::Uncertain => Self::Uncertain,
            ReturnTiming::Contradictory => Self::Contradictory,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubUnresolvedReturnLinkDto {
    OriginalGroupUnknown,
    NoMatchingVariant,
    ClaimedGroupMissing,
    VariantConflict,
    QuantityConflict,
    TimeConflict,
}
impl From<UnresolvedReturnLink> for HubUnresolvedReturnLinkDto {
    fn from(value: UnresolvedReturnLink) -> Self {
        match value {
            UnresolvedReturnLink::OriginalGroupUnknown => Self::OriginalGroupUnknown,
            UnresolvedReturnLink::NoMatchingVariant => Self::NoMatchingVariant,
            UnresolvedReturnLink::ClaimedGroupMissing => Self::ClaimedGroupMissing,
            UnresolvedReturnLink::VariantConflict => Self::VariantConflict,
            UnresolvedReturnLink::QuantityConflict => Self::QuantityConflict,
            UnresolvedReturnLink::TimeConflict => Self::TimeConflict,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubReturnResolutionDto {
    Unknown,
    Unrecorded,
    Unresolved { reason: HubUnresolvedReturnLinkDto },
    EstablishedScope,
}
impl From<ReturnLinkResolution> for HubReturnResolutionDto {
    fn from(value: ReturnLinkResolution) -> Self {
        match value {
            ReturnLinkResolution::Unknown => Self::Unknown,
            ReturnLinkResolution::Unrecorded => Self::Unrecorded,
            ReturnLinkResolution::Unresolved(reason) => Self::Unresolved {
                reason: reason.into(),
            },
            ReturnLinkResolution::EstablishedScope => Self::EstablishedScope,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnDepartureDto {
    pub departure_id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub identification_id: String,
    pub recipient: HubReferenceRecordDto,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnSourceDto {
    pub original: HubDispatchLineDto,
    pub previously_established_return_whole_packs: u32,
}
impl From<&ReturnSourceSnapshot> for HubReturnSourceDto {
    fn from(value: &ReturnSourceSnapshot) -> Self {
        let line = &value.original;
        Self {
            original: HubDispatchLineDto {
                group_id: line.group.to_hex(),
                group_revision: line.group_revision.value(),
                whole_packs: line.packs.value(),
                identification: line.identification.as_ref().map(Into::into),
                location: (&line.location).into(),
                product: (&line.product).into(),
                variant: (&line.variant).into(),
                trace: (&line.trace).into(),
            },
            previously_established_return_whole_packs: value.previously_established_returns.value(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnQuantityHeadDto {
    pub operation_id: String,
    pub stock_revision: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnLineAllowanceDto {
    pub original_group_id: String,
    pub effective_departure_whole_packs: u32,
    pub original_debit_return_bound_whole_packs: u32,
    pub quantity_head: Option<HubReturnQuantityHeadDto>,
    pub unresolved_returns: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnAllowanceCaptureDto {
    pub hub_id: String,
    pub departure_id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub lines: Vec<HubReturnLineAllowanceDto>,
}
impl From<&ReturnAllowanceCapture> for HubReturnAllowanceCaptureDto {
    fn from(value: &ReturnAllowanceCapture) -> Self {
        Self {
            hub_id: value
                .hub
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            departure_id: value.departure.to_hex(),
            operation_id: crate::hub_counts::operation_text(value.operation),
            stock_revision: value.stock_revision.value(),
            lines: value
                .lines
                .iter()
                .map(|line| HubReturnLineAllowanceDto {
                    original_group_id: line.original_group.to_hex(),
                    effective_departure_whole_packs: line.effective_departure.value(),
                    original_debit_return_bound_whole_packs: line
                        .original_debit_return_bound
                        .value(),
                    quantity_head: line.quantity_head.map(|head| HubReturnQuantityHeadDto {
                        operation_id: crate::hub_counts::operation_text(head.operation),
                        stock_revision: head.stock_revision.value(),
                    }),
                    unresolved_returns: line.unresolved_returns,
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReturnReceiptDto {
    pub id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub population: HubStockPopulationDto,
    pub returned_by: Option<HubReferenceRecordDto>,
    pub group: HubPhysicalGroupDto,
    pub intake_work: Option<HubWorkRequirementDto>,
    pub acceptance: HubAcceptanceStatusDto,
    pub claim: Option<HubReturnClaimDto>,
    pub departure: Option<HubReturnDepartureDto>,
    pub claimed_source: Option<HubReturnSourceDto>,
    pub plausible_sources: Vec<HubReturnSourceDto>,
    pub timing: HubReturnTimingDto,
    pub resolution: HubReturnResolutionDto,
    pub current_allowances: Option<HubReturnAllowanceCaptureDto>,
}
impl From<HubReturnReceipt> for HubReturnReceiptDto {
    fn from(value: HubReturnReceipt) -> Self {
        let plan = value.original;
        Self {
            id: plan.identities.returned.to_hex(),
            operation_id: value.operation.as_str().to_owned(),
            stock_revision: plan.stock_revision.value(),
            catalog_revision: plan.catalog_revision.value(),
            population: (&plan.population()).into(),
            returned_by: plan.intake.returned_by.as_ref().map(Into::into),
            group: (&plan.group).into(),
            intake_work: plan.intake_work.as_ref().map(Into::into),
            acceptance: plan.acceptance.into(),
            claim: plan.linkage.claim.map(|c| HubReturnClaimDto {
                departure_id: c.departure.to_hex(),
                original_group_id: c.group.map(|g| g.to_hex()),
            }),
            departure: plan.linkage.departure.map(|d| HubReturnDepartureDto {
                departure_id: d.departure.to_hex(),
                operation_id: crate::hub_counts::operation_text(d.operation),
                stock_revision: d.stock_revision.value(),
                catalog_revision: d.catalog_revision.value(),
                identification_id: d.identification.to_hex(),
                recipient: (&d.recipient).into(),
                actor_public_key: d.actor.to_hex(),
                recorded_at_unix_s: d.recorded_at.as_seconds(),
                occurrence: d.occurred.into(),
            }),
            claimed_source: plan.linkage.claimed_source.as_ref().map(Into::into),
            plausible_sources: plan
                .linkage
                .plausible_sources
                .iter()
                .map(Into::into)
                .collect(),
            timing: plan.linkage.timing.into(),
            resolution: plan.linkage.resolution.into(),
            current_allowances: plan.current_allowances.as_ref().map(Into::into),
        }
    }
}
