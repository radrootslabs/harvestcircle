//! Bounded physical portions and original creation lineage, without client authority.
use crate::commands::HarvestCircleError;
use crate::hub_handling::HubPhysicalIdentificationDto;
use crate::hub_reference::{HubReferenceRecordDto, invalid_input};
use crate::hub_stock::{HubOccurrenceDto, HubStockPopulationDto};
use harvestcircle_domain::hub::{HubRevision, reference::HubRecordId};
use harvestcircle_storage::{
    HubGroupCreationKind, HubLineagePage, HubLineageQuery, HubLineageRow, HubSplitResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubSplitResultDto {
    pub selected_group_id: String,
    pub expected_group_revision: u64,
    pub source_before_packs: u32,
    pub location: HubReferenceRecordDto,
    pub whole_packs: u32,
    pub portion_group_id: String,
    pub remaining_packs: u32,
    pub identification: HubPhysicalIdentificationDto,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl From<HubSplitResult> for HubSplitResultDto {
    fn from(value: HubSplitResult) -> Self {
        Self {
            selected_group_id: value.selected_group.to_hex(),
            expected_group_revision: value.expected_group_revision.value(),
            source_before_packs: value.source_before_packs.value(),
            location: (&value.location).into(),
            whole_packs: value.packs.value(),
            portion_group_id: value.portion_group.to_hex(),
            remaining_packs: value.remaining_packs.value(),
            identification: (&value.identification).into(),
            occurrence: value.occurred.into(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubLineageQueryDto {
    pub trace_id: String,
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
}
impl HubLineageQueryDto {
    pub fn decode(self) -> Result<HubLineageQuery, HarvestCircleError> {
        let query = HubLineageQuery {
            trace: HubRecordId::from_hex(&self.trace_id).map_err(|_| invalid_input())?,
            expected_revision: self
                .expected_stock_revision
                .map(HubRevision::from_value)
                .transpose()
                .map_err(|_| invalid_input())?,
            expected_catalog_revision: self
                .expected_catalog_revision
                .map(HubRevision::from_value)
                .transpose()
                .map_err(|_| invalid_input())?,
            offset: self.offset,
            limit: self.limit,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubGroupCreationKindDto {
    Receipt,
    CountedOpening,
    Movement,
    Separation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubLineageRowDto {
    pub id: String,
    pub parent_id: Option<String>,
    pub identification: Option<HubPhysicalIdentificationDto>,
    pub whole_packs: u32,
    pub revision: u64,
    pub location: Option<HubReferenceRecordDto>,
    pub created_stock_revision: u64,
    pub created_kind: HubGroupCreationKindDto,
    pub created_by: String,
    pub created_at_unix_s: i64,
    pub created_operation_id: String,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl From<HubLineageRow> for HubLineageRowDto {
    fn from(value: HubLineageRow) -> Self {
        Self {
            id: value.id.to_hex(),
            parent_id: value.parent.map(|id| id.to_hex()),
            identification: value.identification.as_ref().map(Into::into),
            whole_packs: value.packs.value(),
            revision: value.revision.value(),
            location: value.location.as_ref().map(Into::into),
            created_stock_revision: value.created_revision.value(),
            created_kind: match value.created_kind {
                HubGroupCreationKind::Receipt => HubGroupCreationKindDto::Receipt,
                HubGroupCreationKind::CountedOpening => HubGroupCreationKindDto::CountedOpening,
                HubGroupCreationKind::Movement => HubGroupCreationKindDto::Movement,
                HubGroupCreationKind::Separation => HubGroupCreationKindDto::Separation,
            },
            created_by: value.created_by.to_hex(),
            created_at_unix_s: value.created_at.as_seconds(),
            created_operation_id: value.created_operation.as_str().to_owned(),
            occurrence: value.occurred.into(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubLineagePageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub trace: HubStockPopulationDto,
    pub rows: Vec<HubLineageRowDto>,
}
impl From<HubLineagePage> for HubLineagePageDto {
    fn from(value: HubLineagePage) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            product: (&value.product).into(),
            variant: (&value.variant).into(),
            trace: (&value.trace).into(),
            rows: value.rows.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_lineage_transport_requires_bounded_pinned_continuations() {
        let query = HubLineageQueryDto {
            trace_id: "01".repeat(16),
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
        };
        assert!(query.clone().decode().is_ok());
        let pinned = HubLineageQueryDto {
            expected_stock_revision: Some(3),
            expected_catalog_revision: Some(6),
            offset: 1,
            ..query.clone()
        };
        assert!(pinned.clone().decode().is_ok());
        for invalid in [
            HubLineageQueryDto {
                trace_id: "bad".into(),
                ..query.clone()
            },
            HubLineageQueryDto {
                limit: 0,
                ..query.clone()
            },
            HubLineageQueryDto {
                limit: 65,
                ..query.clone()
            },
            HubLineageQueryDto {
                offset: 257,
                ..pinned.clone()
            },
            HubLineageQueryDto {
                expected_stock_revision: None,
                ..pinned.clone()
            },
            HubLineageQueryDto {
                expected_catalog_revision: None,
                ..pinned
            },
        ] {
            assert!(invalid.decode().is_err());
        }
    }
}
