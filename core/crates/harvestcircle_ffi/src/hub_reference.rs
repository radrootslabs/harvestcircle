//! Bounded transport for native-authorized reference data. No client owner field.

use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::HubRevision;
use harvestcircle_domain::hub::reference::{
    HUB_LABEL_UTF8_BYTES, HUB_REFERENCE_CAPACITY, HUB_REFERENCE_PAGE_ROWS, HubLabel, HubRecordId,
    LocationPurpose, NominalGrams, ReferenceCatalog, ReferenceCommand, ReferenceKind,
    ReferenceMutation, ReferenceRecord, ReferenceValue, SampleHandlingBasis,
};
use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};
use harvestcircle_storage::HubReferenceReceipt;

use crate::commands::HarvestCircleError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubReferenceKindDto {
    Product,
    Pack,
    Party,
    Location,
}

impl From<HubReferenceKindDto> for ReferenceKind {
    fn from(value: HubReferenceKindDto) -> Self {
        match value {
            HubReferenceKindDto::Product => Self::Product,
            HubReferenceKindDto::Pack => Self::Pack,
            HubReferenceKindDto::Party => Self::Party,
            HubReferenceKindDto::Location => Self::Location,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubLocationPurposeDto {
    Receiving,
    Storage,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferenceInputDto {
    pub kind: HubReferenceKindDto,
    pub label: String,
    pub product_id: Option<String>,
    pub nominal_grams: Option<u32>,
    pub location_purpose: Option<HubLocationPurposeDto>,
}

impl HubReferenceInputDto {
    pub(crate) fn decode(self) -> Result<ReferenceValue, HarvestCircleError> {
        let label = HubLabel::new(self.label).map_err(|_| invalid_input())?;
        match (
            self.kind,
            self.product_id,
            self.nominal_grams,
            self.location_purpose,
        ) {
            (HubReferenceKindDto::Product, None, None, None) => {
                Ok(ReferenceValue::Product { label })
            }
            (HubReferenceKindDto::Party, None, None, None) => Ok(ReferenceValue::Party { label }),
            (HubReferenceKindDto::Pack, Some(product), grams, None) => Ok(ReferenceValue::Pack {
                product: HubRecordId::from_hex(&product).map_err(|_| invalid_input())?,
                label,
                nominal_grams: grams
                    .map(|n| NominalGrams::from_value(n).map_err(|_| invalid_input()))
                    .transpose()?,
            }),
            (HubReferenceKindDto::Location, None, None, Some(purpose)) => {
                Ok(ReferenceValue::Location {
                    label,
                    purpose: match purpose {
                        HubLocationPurposeDto::Receiving => LocationPurpose::Receiving,
                        HubLocationPurposeDto::Storage => LocationPurpose::Storage,
                        HubLocationPurposeDto::Other => LocationPurpose::Other,
                    },
                })
            }
            _ => Err(invalid_input()),
        }
    }
}

impl From<&ReferenceValue> for HubReferenceInputDto {
    fn from(value: &ReferenceValue) -> Self {
        let (kind, product_id, nominal_grams, location_purpose) = match value {
            ReferenceValue::Product { .. } => (HubReferenceKindDto::Product, None, None, None),
            ReferenceValue::Party { .. } => (HubReferenceKindDto::Party, None, None, None),
            ReferenceValue::Pack {
                product,
                nominal_grams,
                ..
            } => (
                HubReferenceKindDto::Pack,
                Some(product.to_hex()),
                nominal_grams.map(NominalGrams::value),
                None,
            ),
            ReferenceValue::Location { purpose, .. } => (
                HubReferenceKindDto::Location,
                None,
                None,
                Some(match purpose {
                    LocationPurpose::Receiving => HubLocationPurposeDto::Receiving,
                    LocationPurpose::Storage => HubLocationPurposeDto::Storage,
                    LocationPurpose::Other => HubLocationPurposeDto::Other,
                }),
            ),
        };
        Self {
            kind,
            label: value.label().as_str().to_owned(),
            product_id,
            nominal_grams,
            location_purpose,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferenceRecordDto {
    pub id: String,
    pub revision: u64,
    pub value: HubReferenceInputDto,
}

impl From<&ReferenceRecord> for HubReferenceRecordDto {
    fn from(record: &ReferenceRecord) -> Self {
        Self {
            id: record.id.to_hex(),
            revision: record.revision.value(),
            value: (&record.value).into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferenceEditDto {
    pub expected_catalog_revision: u64,
    pub record_id: Option<String>,
    pub expected_record_revision: Option<u64>,
    pub value: HubReferenceInputDto,
}

impl HubReferenceEditDto {
    pub(crate) fn command(
        self,
        operation: &DurableRequestId,
    ) -> Result<ReferenceCommand, HarvestCircleError> {
        let expected_catalog_revision =
            HubRevision::from_value(self.expected_catalog_revision).map_err(|_| invalid_input())?;
        let value = self.value.decode()?;
        let mutation = match (self.record_id, self.expected_record_revision) {
            (None, None) => ReferenceMutation::Create {
                id: HubRecordId::from_hex(&operation.as_str().replace('-', ""))
                    .map_err(|_| invalid_input())?,
                value,
            },
            (Some(id), Some(revision)) => ReferenceMutation::Revise {
                id: HubRecordId::from_hex(&id).map_err(|_| invalid_input())?,
                expected_record_revision: HubRevision::from_value(revision)
                    .map_err(|_| invalid_input())?,
                value,
            },
            _ => return Err(invalid_input()),
        };
        Ok(ReferenceCommand {
            expected_catalog_revision,
            mutation,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferenceQueryDto {
    pub expected_catalog_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub kind: Option<HubReferenceKindDto>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferencePageDto {
    pub handling_basis: HubHandlingBasisDto,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub records: Vec<HubReferenceRecordDto>,
}

impl HubReferenceQueryDto {
    pub(crate) fn validate(&self) -> Result<(), HarvestCircleError> {
        if !(1..=HUB_REFERENCE_PAGE_ROWS)
            .contains(&usize::try_from(self.limit).map_err(|_| invalid_input())?)
            || usize::try_from(self.offset).map_err(|_| invalid_input())? > HUB_REFERENCE_CAPACITY
            || self.text.len() > HUB_LABEL_UTF8_BYTES
        {
            return Err(invalid_input());
        }
        if let Some(revision) = self.expected_catalog_revision {
            HubRevision::from_value(revision).map_err(|_| invalid_input())?;
        }
        if !self.text.is_empty() {
            HubLabel::new(self.text.clone()).map_err(|_| invalid_input())?;
        }
        if self.offset != 0 && self.expected_catalog_revision.is_none() {
            return Err(stale_query());
        }
        Ok(())
    }

    pub(crate) fn page(
        self,
        catalog: &ReferenceCatalog,
    ) -> Result<HubReferencePageDto, HarvestCircleError> {
        self.validate()?;
        if self
            .expected_catalog_revision
            .is_some_and(|r| r != catalog.revision().value())
        {
            return Err(stale_query());
        }
        let search = self.text.to_lowercase();
        let records = catalog
            .records()
            .filter(|r| {
                self.kind.is_none_or(|kind| r.value.kind() == kind.into())
                    && r.value.label().as_str().to_lowercase().contains(&search)
            })
            .collect::<Vec<_>>();
        let offset = usize::try_from(self.offset).map_err(|_| invalid_input())?;
        if offset > records.len() {
            return Err(invalid_input());
        }
        let end =
            (offset + usize::try_from(self.limit).map_err(|_| invalid_input())?).min(records.len());
        Ok(HubReferencePageDto {
            handling_basis: catalog.handling_basis().into(),
            catalog_revision: catalog.revision().value(),
            offset: self.offset,
            total: u32::try_from(records.len()).map_err(|_| invalid_input())?,
            more: end < records.len(),
            records: records[offset..end]
                .iter()
                .map(|r| HubReferenceRecordDto::from(*r))
                .collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHandlingBasisDto {
    pub id: String,
    pub version: u32,
    pub label: String,
    pub site_timezone: String,
}

impl From<SampleHandlingBasis> for HubHandlingBasisDto {
    fn from(basis: SampleHandlingBasis) -> Self {
        Self {
            id: basis.id().to_owned(),
            version: basis.version(),
            label: basis.label().to_owned(),
            site_timezone: basis.site_timezone().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReferenceReceiptDto {
    pub operation_id: String,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub catalog_revision: u64,
    pub previous: Option<HubReferenceRecordDto>,
    pub current: HubReferenceRecordDto,
}

impl From<HubReferenceReceipt> for HubReferenceReceiptDto {
    fn from(receipt: HubReferenceReceipt) -> Self {
        Self {
            operation_id: receipt.operation.as_str().to_owned(),
            actor_public_key: receipt.actor.to_hex(),
            recorded_at_unix_s: receipt.recorded_at.as_seconds(),
            catalog_revision: receipt.change.catalog_revision.value(),
            previous: receipt.change.previous.as_ref().map(Into::into),
            current: (&receipt.change.current).into(),
        }
    }
}

pub(crate) fn invalid_input() -> HarvestCircleError {
    SafeError::new(
        SafeErrorCode::HubInvalidInput,
        SafeMessage::new(
            "The reference input or query is invalid or exceeds its supported bounds.",
        ),
    )
    .into()
}

fn stale_query() -> HarvestCircleError {
    SafeError::new(
        SafeErrorCode::HubConflict,
        SafeMessage::new("The reference catalog changed. Reload the first page before continuing."),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> HubReferenceInputDto {
        HubReferenceInputDto {
            kind: HubReferenceKindDto::Party,
            label: "Fictional orchard".to_owned(),
            product_id: None,
            nominal_grams: None,
            location_purpose: None,
        }
    }

    fn query() -> HubReferenceQueryDto {
        HubReferenceQueryDto {
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            kind: None,
            text: String::new(),
        }
    }

    #[test]
    fn input_is_closed_and_typed_before_any_native_effect() {
        let value = input().decode().unwrap();
        assert_eq!(HubReferenceInputDto::from(&value), input());
        assert!(
            HubReferenceInputDto {
                label: "x".repeat(121),
                ..input()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubReferenceInputDto {
                nominal_grams: Some(1),
                ..input()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubReferenceInputDto {
                kind: HubReferenceKindDto::Pack,
                ..input()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubReferenceInputDto {
                kind: HubReferenceKindDto::Location,
                ..input()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubReferenceInputDto {
                kind: HubReferenceKindDto::Pack,
                product_id: Some("1".repeat(32)),
                nominal_grams: Some(0),
                ..input()
            }
            .decode()
            .is_err()
        );
        assert!(
            HubReferenceInputDto {
                kind: HubReferenceKindDto::Pack,
                product_id: Some("../path".to_owned()),
                ..input()
            }
            .decode()
            .is_err()
        );
    }

    #[test]
    fn new_record_identity_is_minted_from_the_original_operation() {
        let operation = DurableRequestId::parse("01900000-0000-7000-8000-000000000001").unwrap();
        let edit = HubReferenceEditDto {
            expected_catalog_revision: 1,
            record_id: None,
            expected_record_revision: None,
            value: input(),
        };
        let ReferenceMutation::Create { id, .. } =
            edit.clone().command(&operation).unwrap().mutation
        else {
            panic!("expected create")
        };
        assert_eq!(id.to_hex(), "01900000000070008000000000000001");
        assert!(
            HubReferenceEditDto {
                record_id: Some(id.to_hex()),
                ..edit.clone()
            }
            .command(&operation)
            .is_err()
        );
        assert!(
            HubReferenceEditDto {
                expected_record_revision: Some(1),
                ..edit.clone()
            }
            .command(&operation)
            .is_err()
        );
        assert!(
            HubReferenceEditDto {
                expected_catalog_revision: 0,
                ..edit.clone()
            }
            .command(&operation)
            .is_err()
        );
        assert!(
            HubReferenceEditDto {
                record_id: Some(id.to_hex()),
                expected_record_revision: Some(1),
                ..edit
            }
            .command(&operation)
            .is_ok()
        );
    }

    #[test]
    fn pages_are_bounded_and_continuation_requires_the_same_catalog_revision() {
        let mut catalog = ReferenceCatalog::default();
        for number in 1u16..=65 {
            let mut bytes = [0; 16];
            bytes[14..].copy_from_slice(&number.to_be_bytes());
            catalog
                .create(
                    catalog.revision(),
                    HubRecordId::from_bytes(bytes).unwrap(),
                    input().decode().unwrap(),
                )
                .unwrap();
        }
        let first = query().page(&catalog).unwrap();
        assert_eq!(first.records.len(), 64);
        assert_eq!(first.total, 65);
        assert!(first.more);
        let last = HubReferenceQueryDto {
            expected_catalog_revision: Some(first.catalog_revision),
            offset: 64,
            ..query()
        }
        .page(&catalog)
        .unwrap();
        assert_eq!(last.records.len(), 1);
        assert!(!last.more);
        assert_ne!(first.records[0].id, last.records[0].id);
        assert!(
            HubReferenceQueryDto {
                offset: 64,
                ..query()
            }
            .page(&catalog)
            .is_err()
        );
        assert!(
            HubReferenceQueryDto {
                expected_catalog_revision: Some(1),
                offset: 64,
                ..query()
            }
            .page(&catalog)
            .is_err()
        );
        assert!(
            HubReferenceQueryDto {
                limit: 65,
                ..query()
            }
            .validate()
            .is_err()
        );
        assert!(
            HubReferenceQueryDto {
                limit: 0,
                ..query()
            }
            .validate()
            .is_err()
        );
        assert!(
            HubReferenceQueryDto {
                offset: 513,
                ..query()
            }
            .validate()
            .is_err()
        );
        assert!(
            HubReferenceQueryDto {
                text: "x".repeat(121),
                ..query()
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            HubReferenceQueryDto {
                kind: Some(HubReferenceKindDto::Product),
                ..query()
            }
            .page(&catalog)
            .unwrap()
            .total,
            0
        );
        assert_eq!(
            HubReferenceQueryDto {
                text: "ORCHARD".to_owned(),
                ..query()
            }
            .page(&catalog)
            .unwrap()
            .total,
            65
        );
    }
}
