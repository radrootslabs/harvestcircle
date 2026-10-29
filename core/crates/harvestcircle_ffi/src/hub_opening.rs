//! Counted entry transport keeps earlier history and actual present count distinct.
use crate::commands::HarvestCircleError;
use crate::hub_handling::{HubPhysicalGroupDto, HubPhysicalIdentificationDto};
use crate::hub_reference::invalid_input;
use crate::hub_stock::{HubOccurrenceDto, HubSourceInputDto, HubStockPopulationDto};
use crate::hub_treatment::HubAcceptanceStatusDto;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::{
    reference::{SourceProvenance, WholePacks},
    stock::{OpeningCommand, OpeningHistory, OpeningIdentities},
};
use harvestcircle_storage::HubOpeningReceipt;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubOpeningInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub variant_id: String,
    pub location_id: String,
    pub whole_packs: u32,
    pub identification: HubPhysicalIdentificationDto,
    pub source: HubSourceInputDto,
    /// Absent explicitly means unknown; no history is manufactured from recording time.
    pub history_reference: Option<String>,
    pub counted: HubOccurrenceDto,
    pub observation: String,
}
impl HubOpeningInputDto {
    pub fn command(
        self,
        operation: &DurableRequestId,
    ) -> Result<OpeningCommand, HarvestCircleError> {
        let identities =
            OpeningIdentities::from_operation_bytes(id(&operation.as_str().replace('-', ""))?)
                .map_err(|_| invalid_input())?;
        let packs = WholePacks::from_value(self.whole_packs).map_err(|_| invalid_input())?;
        if packs.value() == 0 {
            return Err(invalid_input());
        }
        Ok(OpeningCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            variant: id(&self.variant_id)?,
            location: id(&self.location_id)?,
            packs,
            identification: self.identification.decode()?,
            source: self.source.decode(identities.source)?,
            history: match self.history_reference {
                None => OpeningHistory::Unknown,
                Some(value) => OpeningHistory::ExternalReference(
                    SourceProvenance::new(value).map_err(|_| invalid_input())?,
                ),
            },
            counted: self.counted.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountedStockDto {
    pub population: HubStockPopulationDto,
    pub group: HubPhysicalGroupDto,
    pub acceptance: HubAcceptanceStatusDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubOpeningReceiptDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub original: HubCountedStockDto,
}
impl From<HubOpeningReceipt> for HubOpeningReceiptDto {
    fn from(value: HubOpeningReceipt) -> Self {
        let plan = value.original;
        Self {
            operation_id: value.operation.as_str().to_owned(),
            stock_revision: plan.stock_revision.value(),
            original: HubCountedStockDto {
                population: (&plan.population).into(),
                group: (&plan.group).into(),
                acceptance: plan.acceptance.into(),
            },
        }
    }
}
