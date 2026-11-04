//! Original/current recipient meaning and complete manual scope; no mutation grant.
use crate::{hub_reference::HubReferenceRecordDto, hub_trace_source::HubCorrectionImpactDto};
use harvestcircle_domain::hub::corrections::{AmendedRecipient, RecipientAttributionCorrection};
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceRecipientCorrectionDto {
    pub operation_id: String,
    pub previous_operation_id: Option<String>,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub original_departure_operation_id: String,
    pub original_recipient: HubReferenceRecordDto,
    pub original_population_ids: Vec<String>,
    pub original_issued_version_ids: Vec<String>,
    /// None is explicitly unknown; it never means "use the old recipient".
    pub before: Option<HubReferenceRecordDto>,
    pub after: Option<HubReferenceRecordDto>,
    pub basis: String,
    pub impact: Vec<HubCorrectionImpactDto>,
}
impl From<&RecipientAttributionCorrection> for HubTraceRecipientCorrectionDto {
    fn from(value: &RecipientAttributionCorrection) -> Self {
        let recipient = |r: &AmendedRecipient| match r {
            AmendedRecipient::Unknown => None,
            AmendedRecipient::Known(party) => Some(HubReferenceRecordDto::from(party)),
        };
        Self {
            operation_id: crate::hub_trace::operation_text(value.stamp.operation),
            previous_operation_id: value.prior.map(crate::hub_trace::operation_text),
            stock_revision: value.stamp.stock_revision.value(),
            catalog_revision: value.stamp.catalog_revision.value(),
            actor_public_key: value.stamp.actor.to_hex(),
            recorded_at_unix_s: value.stamp.recorded_at.as_seconds(),
            original_departure_operation_id: crate::hub_trace::operation_text(
                value.original.operation,
            ),
            original_recipient: (&value.original.recipient).into(),
            original_population_ids: value
                .original
                .populations
                .iter()
                .map(|id| id.to_hex())
                .collect(),
            original_issued_version_ids: value
                .original
                .issued_versions
                .iter()
                .map(|id| id.to_hex())
                .collect(),
            before: recipient(&value.before),
            after: recipient(&value.after),
            basis: value.stamp.basis.as_str().into(),
            impact: value.impact.rows().iter().map(Into::into).collect(),
        }
    }
}
