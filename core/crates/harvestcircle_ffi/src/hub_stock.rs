//! Closed, bounded inventory transport. The native session supplies actor and time.
use crate::commands::HarvestCircleError;
use crate::hub_reference::{HubHandlingBasisDto, HubReferenceRecordDto};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::HubRevision;
use harvestcircle_domain::hub::reference::{
    HubRecordId, SourceAttribution, SourceCertainty, SourceCode, SourceProvenance, SourceReference,
    SourceReferenceKind, WholePacks,
};
use harvestcircle_domain::hub::stock::{
    Occurrence, QuantityWork, ReceiptCommand, ReceiptIdentities, ReceiptTrace, StockPosition,
};
use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};
use harvestcircle_storage::{HubStockPage, HubStockQuery, HubStockReceipt, HubStockRow};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubSourceCertaintyDto {
    Unknown,
    Possible,
    Established,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubSourceKindDto {
    ProducerBatch,
    SupplierReference,
    Other,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubOccurrenceKindDto {
    Known,
    Interval,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubOccurrenceDto {
    pub kind: HubOccurrenceKindDto,
    pub start_unix_s: Option<i64>,
    pub end_unix_s: Option<i64>,
}
impl HubOccurrenceDto {
    pub(crate) fn decode(self) -> Result<Occurrence, HarvestCircleError> {
        let at = |n| UnixTimestamp::from_seconds(n).ok_or_else(invalid_input);
        match (self.kind, self.start_unix_s, self.end_unix_s) {
            (HubOccurrenceKindDto::Unknown, None, None) => Ok(Occurrence::Unknown),
            (HubOccurrenceKindDto::Known, Some(start), None) => Ok(Occurrence::Known(at(start)?)),
            (HubOccurrenceKindDto::Interval, Some(start), Some(end)) if start <= end => {
                Ok(Occurrence::Interval {
                    earliest: at(start)?,
                    latest: at(end)?,
                })
            }
            _ => Err(invalid_input()),
        }
    }
}
impl From<Occurrence> for HubOccurrenceDto {
    fn from(value: Occurrence) -> Self {
        match value {
            Occurrence::Unknown => Self {
                kind: HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            Occurrence::Known(at) => Self {
                kind: HubOccurrenceKindDto::Known,
                start_unix_s: Some(at.as_seconds()),
                end_unix_s: None,
            },
            Occurrence::Interval { earliest, latest } => Self {
                kind: HubOccurrenceKindDto::Interval,
                start_unix_s: Some(earliest.as_seconds()),
                end_unix_s: Some(latest.as_seconds()),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubSourceInputDto {
    /// None for a new explicitly described source; a reused ID must already exist.
    pub existing_source_id: Option<String>,
    pub issuer_id: Option<String>,
    pub kind: Option<HubSourceKindDto>,
    pub code: Option<String>,
    pub provenance: Option<String>,
    pub certainty: HubSourceCertaintyDto,
}
impl HubSourceInputDto {
    fn decode(self, ids: ReceiptIdentities) -> Result<SourceAttribution, HarvestCircleError> {
        if self.certainty == HubSourceCertaintyDto::Unknown {
            return if self.existing_source_id.is_none()
                && self.issuer_id.is_none()
                && self.kind.is_none()
                && self.code.is_none()
                && self.provenance.is_none()
            {
                Ok(SourceAttribution::Unknown)
            } else {
                Err(invalid_input())
            };
        }
        let source_id = match self.existing_source_id {
            Some(value) => {
                let id = record_id(&value)?;
                // Reuse cannot nominate this operation's not-yet-created source identity.
                if id == ids.source {
                    return Err(invalid_input());
                }
                id
            }
            None => ids.source,
        };
        Ok(SourceAttribution::Referenced {
            source: SourceReference {
                id: source_id,
                issuer: self.issuer_id.as_deref().map(record_id).transpose()?,
                kind: match self.kind.ok_or_else(invalid_input)? {
                    HubSourceKindDto::ProducerBatch => SourceReferenceKind::ProducerBatch,
                    HubSourceKindDto::SupplierReference => SourceReferenceKind::SupplierReference,
                    HubSourceKindDto::Other => SourceReferenceKind::Other,
                },
                code: SourceCode::new(self.code.ok_or_else(invalid_input)?)
                    .map_err(|_| invalid_input())?,
                provenance: SourceProvenance::new(self.provenance.ok_or_else(invalid_input)?)
                    .map_err(|_| invalid_input())?,
            },
            certainty: match self.certainty {
                HubSourceCertaintyDto::Possible => SourceCertainty::ReportedPossible,
                HubSourceCertaintyDto::Established => SourceCertainty::Established,
                HubSourceCertaintyDto::Unknown => return Err(invalid_input()),
            },
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubSourceDto {
    pub id: Option<String>,
    pub issuer: Option<HubReferenceRecordDto>,
    pub kind: Option<HubSourceKindDto>,
    pub code: Option<String>,
    pub provenance: Option<String>,
    pub certainty: HubSourceCertaintyDto,
}
impl HubSourceDto {
    fn from_trace(trace: &ReceiptTrace) -> Self {
        match &trace.source {
            SourceAttribution::Unknown => Self {
                id: None,
                issuer: None,
                kind: None,
                code: None,
                provenance: None,
                certainty: HubSourceCertaintyDto::Unknown,
            },
            SourceAttribution::Referenced { source, certainty } => Self {
                id: Some(source.id.to_hex()),
                issuer: trace.source_issuer.as_ref().map(Into::into),
                kind: Some(match source.kind {
                    SourceReferenceKind::ProducerBatch => HubSourceKindDto::ProducerBatch,
                    SourceReferenceKind::SupplierReference => HubSourceKindDto::SupplierReference,
                    SourceReferenceKind::Other => HubSourceKindDto::Other,
                }),
                code: Some(source.code.as_str().to_owned()),
                provenance: Some(source.provenance.as_str().to_owned()),
                certainty: match certainty {
                    SourceCertainty::ReportedPossible => HubSourceCertaintyDto::Possible,
                    SourceCertainty::Established => HubSourceCertaintyDto::Established,
                },
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReceiptInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub variant_id: String,
    pub supplier_id: Option<String>,
    pub producer_id: Option<String>,
    pub source: HubSourceInputDto,
    pub whole_packs: u32,
    pub receiving_id: String,
    pub planned_location_id: Option<String>,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubReceiptInputDto {
    pub fn command(
        self,
        operation: &DurableRequestId,
    ) -> Result<ReceiptCommand, HarvestCircleError> {
        let operation_bytes = record_id(&operation.as_str().replace('-', ""))?;
        let ids = ReceiptIdentities::from_operation_bytes(operation_bytes)
            .map_err(|_| invalid_input())?;
        let packs = WholePacks::from_value(self.whole_packs).map_err(|_| invalid_input())?;
        if packs.value() == 0 {
            return Err(invalid_input());
        }
        Ok(ReceiptCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            variant: record_id(&self.variant_id)?,
            supplier: self.supplier_id.as_deref().map(record_id).transpose()?,
            producer: self.producer_id.as_deref().map(record_id).transpose()?,
            source: self.source.decode(ids)?,
            packs,
            receiving: record_id(&self.receiving_id)?,
            planned_location: self
                .planned_location_id
                .as_deref()
                .map(record_id)
                .transpose()?,
            occurred: self.occurrence.decode()?,
            observation: SourceProvenance::new(self.observation).map_err(|_| invalid_input())?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubStockQueryDto {
    pub expected_catalog_revision: Option<u64>,
    pub expected_stock_revision: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub location_id: Option<String>,
    pub trace_id: Option<String>,
    pub text: String,
}
impl HubStockQueryDto {
    pub fn decode(self) -> Result<HubStockQuery, HarvestCircleError> {
        let query = HubStockQuery {
            expected_catalog_revision: self.expected_catalog_revision.map(revision).transpose()?,
            expected_revision: self.expected_stock_revision.map(revision).transpose()?,
            offset: self.offset,
            limit: self.limit,
            location: self.location_id.as_deref().map(record_id).transpose()?,
            trace: self.trace_id.as_deref().map(record_id).transpose()?,
            text: self.text,
        };
        query.validate().map_err(HarvestCircleError::from)?;
        Ok(query)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubReceiptTraceDto {
    pub id: String,
    pub variant: HubReferenceRecordDto,
    pub product: HubReferenceRecordDto,
    pub supplier: Option<HubReferenceRecordDto>,
    pub producer: Option<HubReferenceRecordDto>,
    pub source: HubSourceDto,
    pub source_needs_review: bool,
    pub original_whole_packs: u32,
    pub receiving: HubReferenceRecordDto,
    pub planned_location: Option<HubReferenceRecordDto>,
    pub occurrence: HubOccurrenceDto,
    pub recorded_at_unix_s: i64,
    pub actor_public_key: String,
    pub observation: String,
}
impl From<&ReceiptTrace> for HubReceiptTraceDto {
    fn from(trace: &ReceiptTrace) -> Self {
        Self {
            id: trace.id.to_hex(),
            variant: (&trace.variant).into(),
            product: (&trace.product).into(),
            supplier: trace.supplier.as_ref().map(Into::into),
            producer: trace.producer.as_ref().map(Into::into),
            source: HubSourceDto::from_trace(trace),
            source_needs_review: trace.source_needs_review(),
            original_whole_packs: trace.original_packs.value(),
            receiving: (&trace.receiving).into(),
            planned_location: trace.planned_location.as_ref().map(Into::into),
            occurrence: trace.occurred.into(),
            recorded_at_unix_s: trace.recorded_at.as_seconds(),
            actor_public_key: trace.actor.to_hex(),
            observation: trace.observation.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubStockPositionDto {
    pub group_id: String,
    pub trace_id: String,
    pub variant_id: String,
    pub location_id: String,
    pub whole_packs: u32,
    pub revision: u64,
}
impl From<&StockPosition> for HubStockPositionDto {
    fn from(position: &StockPosition) -> Self {
        Self {
            group_id: position.group.to_hex(),
            trace_id: position.trace.to_hex(),
            variant_id: position.variant.to_hex(),
            location_id: position.location.to_hex(),
            whole_packs: position.packs.value(),
            revision: position.revision.value(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubQuantityWorkDto {
    pub required: u32,
    pub completed: u32,
    pub no_longer_required: u32,
    pub remaining: u32,
    pub revision: u64,
    pub target: Option<HubReferenceRecordDto>,
}
impl HubQuantityWorkDto {
    pub(crate) fn from_work(
        work: QuantityWork,
        revision: HubRevision,
        target: Option<&harvestcircle_domain::hub::reference::ReferenceRecord>,
    ) -> Self {
        Self {
            required: work.required().value(),
            completed: work.completed().value(),
            no_longer_required: work.no_longer_required().value(),
            remaining: work.remaining().value(),
            revision: revision.value(),
            target: target.map(Into::into),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubStockRowDto {
    pub product: HubReferenceRecordDto,
    pub position: HubStockPositionDto,
    pub location: HubReferenceRecordDto,
    pub trace: HubReceiptTraceDto,
    pub put_away: HubQuantityWorkDto,
}
impl From<&HubStockRow> for HubStockRowDto {
    fn from(row: &HubStockRow) -> Self {
        Self {
            product: (&row.product).into(),
            position: (&row.position).into(),
            location: (&row.location).into(),
            trace: (&row.trace).into(),
            put_away: HubQuantityWorkDto::from_work(
                row.put_away,
                row.work_revision,
                row.work_target.as_ref(),
            ),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubStockPageDto {
    pub handling_basis: HubHandlingBasisDto,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubStockRowDto>,
}
impl From<HubStockPage> for HubStockPageDto {
    fn from(page: HubStockPage) -> Self {
        Self {
            handling_basis: page.handling_basis.into(),
            stock_revision: page.revision.value(),
            catalog_revision: page.catalog_revision.value(),
            offset: page.offset,
            total: page.total,
            more: page.more,
            rows: page.rows.iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubStockReceiptDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub trace: HubReceiptTraceDto,
    pub initial_position: HubStockPositionDto,
    pub initial_put_away: HubQuantityWorkDto,
}
impl From<HubStockReceipt> for HubStockReceiptDto {
    fn from(receipt: HubStockReceipt) -> Self {
        Self {
            operation_id: receipt.operation.as_str().to_owned(),
            stock_revision: receipt.original.stock_revision.value(),
            trace: (&receipt.original.trace).into(),
            initial_position: (&receipt.original.position).into(),
            initial_put_away: HubQuantityWorkDto::from_work(
                receipt.original.put_away,
                HubRevision::INITIAL,
                receipt.original.trace.planned_location.as_ref(),
            ),
        }
    }
}
fn record_id(value: &str) -> Result<HubRecordId, HarvestCircleError> {
    HubRecordId::from_hex(value).map_err(|_| invalid_input())
}
fn revision(value: u64) -> Result<HubRevision, HarvestCircleError> {
    HubRevision::from_value(value).map_err(|_| invalid_input())
}
fn invalid_input() -> HarvestCircleError {
    SafeError::new(
        SafeErrorCode::HubInvalidInput,
        SafeMessage::new("Review the bounded receipt fields, original packs and source certainty."),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unknown_source() -> HubSourceInputDto {
        HubSourceInputDto {
            existing_source_id: None,
            issuer_id: None,
            kind: None,
            code: None,
            provenance: None,
            certainty: HubSourceCertaintyDto::Unknown,
        }
    }
    fn input() -> HubReceiptInputDto {
        HubReceiptInputDto {
            expected_stock_revision: 1,
            expected_catalog_revision: 5,
            variant_id: "01".repeat(16),
            supplier_id: None,
            producer_id: None,
            source: unknown_source(),
            whole_packs: 20,
            receiving_id: "02".repeat(16),
            planned_location_id: Some("03".repeat(16)),
            occurrence: HubOccurrenceDto {
                kind: HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            observation: "Counted twenty original packs".to_owned(),
        }
    }
    fn operation(n: u8) -> DurableRequestId {
        DurableRequestId::parse(format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
    }
    #[test]
    fn receipt_input_preserves_unknowns_and_rejects_invalid_shapes_before_native_work() {
        let command = input().command(&operation(1)).unwrap();
        assert_eq!(command.source, SourceAttribution::Unknown);
        assert_eq!(command.occurred, Occurrence::Unknown);
        let invalid = [
            HubReceiptInputDto {
                whole_packs: 0,
                ..input()
            },
            HubReceiptInputDto {
                whole_packs: 1_000_001,
                ..input()
            },
            HubReceiptInputDto {
                expected_stock_revision: 0,
                ..input()
            },
            HubReceiptInputDto {
                expected_catalog_revision: u64::MAX,
                ..input()
            },
            HubReceiptInputDto {
                variant_id: "not-an-id".into(),
                ..input()
            },
            HubReceiptInputDto {
                observation: "x".repeat(513),
                ..input()
            },
            HubReceiptInputDto {
                source: HubSourceInputDto {
                    code: Some("must not invent known source".into()),
                    ..unknown_source()
                },
                ..input()
            },
            HubReceiptInputDto {
                occurrence: HubOccurrenceDto {
                    kind: HubOccurrenceKindDto::Known,
                    start_unix_s: Some(-1),
                    end_unix_s: None,
                },
                ..input()
            },
            HubReceiptInputDto {
                occurrence: HubOccurrenceDto {
                    kind: HubOccurrenceKindDto::Interval,
                    start_unix_s: Some(20),
                    end_unix_s: Some(10),
                },
                ..input()
            },
        ];
        for bad in invalid {
            assert!(bad.command(&operation(1)).is_err());
        }
    }
    #[test]
    fn new_source_identity_is_native_minted_and_existing_selection_cannot_create_it() {
        let source = HubSourceInputDto {
            existing_source_id: None,
            issuer_id: Some("04".repeat(16)),
            kind: Some(HubSourceKindDto::ProducerBatch),
            code: Some("BATCH-01".into()),
            provenance: Some("Original fictional label".into()),
            certainty: HubSourceCertaintyDto::Established,
        };
        let one = HubReceiptInputDto {
            source: source.clone(),
            ..input()
        }
        .command(&operation(1))
        .unwrap();
        let two = HubReceiptInputDto {
            source: source.clone(),
            ..input()
        }
        .command(&operation(2))
        .unwrap();
        let SourceAttribution::Referenced { source: first, .. } = one.source else {
            panic!()
        };
        let SourceAttribution::Referenced { source: second, .. } = two.source else {
            panic!()
        };
        assert_ne!(first.id, second.id);
        assert_eq!(
            first.issuer,
            Some(HubRecordId::from_hex(&"04".repeat(16)).unwrap())
        );
        let reuse = HubSourceInputDto {
            existing_source_id: Some(first.id.to_hex()),
            ..source
        };
        assert!(
            HubReceiptInputDto {
                source: reuse.clone(),
                ..input()
            }
            .command(&operation(1))
            .is_err()
        );
        let command = HubReceiptInputDto {
            source: reuse,
            ..input()
        }
        .command(&operation(2))
        .unwrap();
        let SourceAttribution::Referenced { source: reused, .. } = command.source else {
            panic!()
        };
        assert_eq!(reused.id, first.id);
    }
    #[test]
    fn stock_query_limits_and_occurrence_roundtrip_are_closed() {
        let base = HubStockQueryDto {
            expected_catalog_revision: None,
            expected_stock_revision: None,
            offset: 0,
            limit: 64,
            location_id: None,
            trace_id: None,
            text: String::new(),
        };
        assert!(base.clone().decode().is_ok());
        for invalid in [
            HubStockQueryDto {
                limit: 65,
                ..base.clone()
            },
            HubStockQueryDto {
                offset: 1,
                ..base.clone()
            },
            HubStockQueryDto {
                text: "x".repeat(121),
                ..base.clone()
            },
        ] {
            assert!(invalid.decode().is_err());
        }
        for occurrence in [
            Occurrence::Unknown,
            Occurrence::Known(UnixTimestamp::from_seconds(10).unwrap()),
            Occurrence::Interval {
                earliest: UnixTimestamp::from_seconds(10).unwrap(),
                latest: UnixTimestamp::from_seconds(20).unwrap(),
            },
        ] {
            assert_eq!(
                HubOccurrenceDto::from(occurrence).decode().unwrap(),
                occurrence
            );
        }
    }
}
