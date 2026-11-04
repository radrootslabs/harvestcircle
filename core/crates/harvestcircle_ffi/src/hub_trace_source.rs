//! Original/current source meaning and manual follow-up scope, never authority.
use crate::hub_stock::HubSourceDto;
use harvestcircle_domain::hub::corrections::{CorrectionImpact, SourceAttributionCorrection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubCorrectionImpactKindDto {
    Population,
    Source,
    Departure,
    IssuedVersion,
    Recipient,
    UnresolvedSource,
    UnresolvedRecipient,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCorrectionImpactDto {
    pub kind: HubCorrectionImpactKindDto,
    pub target_id: String,
}
impl From<&CorrectionImpact> for HubCorrectionImpactDto {
    fn from(value: &CorrectionImpact) -> Self {
        let (kind, id) = match *value {
            CorrectionImpact::Population(id) => (HubCorrectionImpactKindDto::Population, id),
            CorrectionImpact::Source(id) => (HubCorrectionImpactKindDto::Source, id),
            CorrectionImpact::Departure(id) => (HubCorrectionImpactKindDto::Departure, id),
            CorrectionImpact::IssuedVersion(id) => (HubCorrectionImpactKindDto::IssuedVersion, id),
            CorrectionImpact::Recipient(id) => (HubCorrectionImpactKindDto::Recipient, id),
            CorrectionImpact::UnresolvedSource(id) => {
                (HubCorrectionImpactKindDto::UnresolvedSource, id)
            }
            CorrectionImpact::UnresolvedRecipient(id) => {
                (HubCorrectionImpactKindDto::UnresolvedRecipient, id)
            }
        };
        Self {
            kind,
            target_id: id.to_hex(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubTraceSourceCorrectionDto {
    pub operation_id: String,
    pub previous_operation_id: Option<String>,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub before: HubSourceDto,
    pub after: HubSourceDto,
    pub basis: String,
    pub impact: Vec<HubCorrectionImpactDto>,
}
impl From<&SourceAttributionCorrection> for HubTraceSourceCorrectionDto {
    fn from(value: &SourceAttributionCorrection) -> Self {
        Self {
            operation_id: crate::hub_trace::operation_text(value.stamp.operation),
            previous_operation_id: value.prior.map(crate::hub_trace::operation_text),
            stock_revision: value.stamp.stock_revision.value(),
            catalog_revision: value.stamp.catalog_revision.value(),
            actor_public_key: value.stamp.actor.to_hex(),
            recorded_at_unix_s: value.stamp.recorded_at.as_seconds(),
            before: HubSourceDto::from_attribution(
                &value.before.source,
                value.before.issuer.as_ref(),
            ),
            after: HubSourceDto::from_attribution(&value.after.source, value.after.issuer.as_ref()),
            basis: value.stamp.basis.as_str().into(),
            impact: value.impact.rows().iter().map(Into::into).collect(),
        }
    }
}
