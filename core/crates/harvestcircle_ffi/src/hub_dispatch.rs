//! Bounded native preparation data. Stored identification grants no departure.
use crate::commands::HarvestCircleError;
use crate::hub_handling::HubPhysicalIdentificationDto;
use crate::hub_reference::{HubHandlingBasisDto, HubReferenceRecordDto};
use crate::hub_stock::HubReceiptTraceDto;
use crate::hub_treatment_input::revision;
use harvestcircle_domain::hub::dispatch::{
    DispatchMaterial, DispatchRecord, DispatchStamp, IdentificationStatus, IdentificationVersion,
};
use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};
use harvestcircle_storage::{
    HubDispatchFilter, HubDispatchKind, HubDispatchPage, HubDispatchQuery, HubDispatchReceipt,
    HubDispatchRow, HubDispatchState, HubDispatchView,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubDispatchStateDto {
    Draft,
    Issued,
    Unused,
    Departed,
}
impl From<HubDispatchState> for HubDispatchStateDto {
    fn from(value: HubDispatchState) -> Self {
        match value {
            HubDispatchState::Draft => Self::Draft,
            HubDispatchState::Issued => Self::Issued,
            HubDispatchState::Unused => Self::Unused,
            HubDispatchState::Departed => Self::Departed,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubDispatchFilterDto {
    All,
    Draft,
    Issued,
    Unused,
    Departed,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchQueryDto {
    pub expected_stock_revision: Option<u64>,
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub filter: HubDispatchFilterDto,
    pub text: String,
}
impl HubDispatchQueryDto {
    pub fn decode(self) -> Result<HubDispatchQuery, HarvestCircleError> {
        let query = HubDispatchQuery {
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
            text: self.text,
            filter: match self.filter {
                HubDispatchFilterDto::All => HubDispatchFilter::All,
                HubDispatchFilterDto::Draft => HubDispatchFilter::Draft,
                HubDispatchFilterDto::Issued => HubDispatchFilter::Issued,
                HubDispatchFilterDto::Unused => HubDispatchFilter::Unused,
                HubDispatchFilterDto::Departed => HubDispatchFilter::Departed,
            },
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchStampDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub observation: String,
}
impl From<&DispatchStamp> for HubDispatchStampDto {
    fn from(value: &DispatchStamp) -> Self {
        let op = value.operation.to_hex();
        Self {
            operation_id: format!(
                "{}-{}-{}-{}-{}",
                &op[..8],
                &op[8..12],
                &op[12..16],
                &op[16..20],
                &op[20..]
            ),
            stock_revision: value.stock_revision.value(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            observation: value.observation.as_str().to_owned(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchLineDto {
    pub group_id: String,
    pub group_revision: u64,
    pub whole_packs: u32,
    pub identification: Option<HubPhysicalIdentificationDto>,
    pub location: HubReferenceRecordDto,
    pub product: HubReferenceRecordDto,
    pub variant: HubReferenceRecordDto,
    pub trace: HubReceiptTraceDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchMaterialDto {
    pub captured_stock_revision: u64,
    pub captured_catalog_revision: u64,
    pub basis: HubHandlingBasisDto,
    pub recipient: HubReferenceRecordDto,
    pub lines: Vec<HubDispatchLineDto>,
    pub whole_packs: u32,
    pub observation: String,
}
impl TryFrom<&DispatchMaterial> for HubDispatchMaterialDto {
    type Error = HarvestCircleError;
    fn try_from(value: &DispatchMaterial) -> Result<Self, Self::Error> {
        let whole_packs = value.total_packs().map_err(|_| invalid_stored())?;
        Ok(Self {
            captured_stock_revision: value.stock_revision.value(),
            captured_catalog_revision: value.catalog_revision.value(),
            basis: value.basis.into(),
            recipient: (&value.recipient).into(),
            lines: value
                .lines
                .iter()
                .map(|line| HubDispatchLineDto {
                    group_id: line.group.to_hex(),
                    group_revision: line.group_revision.value(),
                    whole_packs: line.packs.value(),
                    identification: line.identification.as_ref().map(Into::into),
                    location: (&line.location).into(),
                    product: (&line.product).into(),
                    variant: (&line.variant).into(),
                    trace: (&line.trace).into(),
                })
                .collect(),
            whole_packs,
            observation: value.observation.as_str().to_owned(),
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubIdentificationStatusDto {
    Issued,
    Superseded,
    Unused,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubIdentificationVersionDto {
    pub id: String,
    pub number: u32,
    pub previous_id: Option<String>,
    pub material: HubDispatchMaterialDto,
    pub issued: HubDispatchStampDto,
    pub status: HubIdentificationStatusDto,
    pub closed: Option<HubDispatchStampDto>,
}
impl TryFrom<&IdentificationVersion> for HubIdentificationVersionDto {
    type Error = HarvestCircleError;
    fn try_from(value: &IdentificationVersion) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id.to_hex(),
            number: value.number,
            previous_id: value.previous.map(|id| id.to_hex()),
            material: (&value.material).try_into()?,
            issued: (&value.issued).into(),
            status: match value.status {
                IdentificationStatus::Issued => HubIdentificationStatusDto::Issued,
                IdentificationStatus::Superseded => HubIdentificationStatusDto::Superseded,
                IdentificationStatus::Unused => HubIdentificationStatusDto::Unused,
            },
            closed: value.closed.as_ref().map(Into::into),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchRecordDto {
    pub id: String,
    pub hub_id: String,
    pub revision: u64,
    pub state: HubDispatchStateDto,
    pub created: HubDispatchStampDto,
    pub updated: HubDispatchStampDto,
    pub draft: Option<HubDispatchMaterialDto>,
    pub versions: Vec<HubIdentificationVersionDto>,
}
impl TryFrom<&DispatchRecord> for HubDispatchRecordDto {
    type Error = HarvestCircleError;
    fn try_from(value: &DispatchRecord) -> Result<Self, Self::Error> {
        value.validate().map_err(|_| invalid_stored())?;
        let state = if value.draft.is_some() {
            HubDispatchStateDto::Draft
        } else {
            match value.versions.last().ok_or_else(invalid_stored)?.status {
                IdentificationStatus::Issued => HubDispatchStateDto::Issued,
                IdentificationStatus::Unused => HubDispatchStateDto::Unused,
                IdentificationStatus::Superseded => return Err(invalid_stored()),
            }
        };
        Ok(Self {
            id: value.id.to_hex(),
            hub_id: harvestcircle_domain::hub::reference::HubRecordId::from_bytes(
                *value.hub.as_bytes(),
            )
            .map_err(|_| invalid_stored())?
            .to_hex(),
            revision: value.revision.value(),
            state,
            created: (&value.created).into(),
            updated: (&value.updated).into(),
            draft: value.draft.as_ref().map(TryInto::try_into).transpose()?,
            versions: value
                .versions
                .iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchRowDto {
    pub id: String,
    pub revision: u64,
    pub state: HubDispatchStateDto,
    pub recipient: HubReferenceRecordDto,
    pub whole_packs: u32,
    pub line_count: u32,
    pub version_count: u32,
    pub identification_id: Option<String>,
    pub updated: HubDispatchStampDto,
}
impl From<HubDispatchRow> for HubDispatchRowDto {
    fn from(value: HubDispatchRow) -> Self {
        Self {
            id: value.id.to_hex(),
            revision: value.revision.value(),
            state: value.state.into(),
            recipient: (&value.recipient).into(),
            whole_packs: value.whole_packs,
            line_count: value.line_count,
            version_count: value.version_count,
            identification_id: value.identification.map(|id| id.to_hex()),
            updated: (&value.updated).into(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchPageDto {
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubDispatchRowDto>,
}
impl From<HubDispatchPage> for HubDispatchPageDto {
    fn from(value: HubDispatchPage) -> Self {
        Self {
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            offset: value.offset,
            total: value.total,
            more: value.more,
            rows: value.rows.into_iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchViewDto {
    pub state: HubDispatchStateDto,
    pub departure: Option<crate::hub_departure::HubDepartureUseDto>,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub record: HubDispatchRecordDto,
}
impl TryFrom<HubDispatchView> for HubDispatchViewDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubDispatchView) -> Result<Self, Self::Error> {
        Ok(Self {
            state: value.state.into(),
            departure: value.departure.map(Into::into),
            stock_revision: value.revision.value(),
            catalog_revision: value.catalog_revision.value(),
            record: (&value.record).try_into()?,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubDispatchKindDto {
    Prepare,
    Revise,
    Issue,
    Replace,
    MarkUnused,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchReceiptDto {
    pub operation_id: String,
    pub kind: HubDispatchKindDto,
    pub catalog_revision: u64,
    pub record: HubDispatchRecordDto,
}
impl TryFrom<HubDispatchReceipt> for HubDispatchReceiptDto {
    type Error = HarvestCircleError;
    fn try_from(value: HubDispatchReceipt) -> Result<Self, Self::Error> {
        Ok(Self {
            operation_id: value.operation.as_str().to_owned(),
            kind: match value.kind {
                HubDispatchKind::Prepare => HubDispatchKindDto::Prepare,
                HubDispatchKind::Revise => HubDispatchKindDto::Revise,
                HubDispatchKind::Issue => HubDispatchKindDto::Issue,
                HubDispatchKind::Replace => HubDispatchKindDto::Replace,
                HubDispatchKind::MarkUnused => HubDispatchKindDto::MarkUnused,
            },
            catalog_revision: value.catalog_revision.value(),
            record: (&value.record).try_into()?,
        })
    }
}
fn invalid_stored() -> HarvestCircleError {
    HarvestCircleError::from(SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The stored preparation is invalid."),
    ))
}
