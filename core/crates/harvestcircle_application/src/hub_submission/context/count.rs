//! Explicit descriptive count contexts. There is no decoder, native grant or
//! saved-request execution path. Producers must establish canonical completeness.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{
        ActualKnownCountInventory, ActualPhysicalActivity, ActualReportRecord,
        CountExplanationSourceWitness, HistoricalActualSource,
    },
    corrections::{
        CorrectionObservationRecord, CountExplanationPlan, CountLossQuantityAccount,
        OriginalRecipientAttribution, RecipientAttributionCorrection,
    },
    counts::CountPostPlan,
    portions::PhysicalLineage,
    treatment::TreatmentState,
};

/// Native21 selects this material even for an empty inventory. The nested
/// physical review omits its physical20 inventory; mode comes from native
/// epoch/marker evidence, never from which optional JSON fields are present.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedCountAwareActualReview {
    pub review: HubSubmittedActualReview,
    pub inventory: ActualKnownCountInventory,
}

/// A selected event is not a complete inventory. Native code separately binds
/// a count source witness only for an immutable count explanation kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedCountAwareActualLink {
    pub link: HubSubmittedActualLink,
    pub count_source: Option<CountExplanationSourceWitness>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedCountExplanation {
    pub id: HubRecordId,
    pub count: CountPostPlan,
    pub account: CountLossQuantityAccount,
    pub report: ActualReportRecord,
    pub source: HistoricalActualSource,
    pub lineage: PhysicalLineage,
    pub activity: Vec<ActualPhysicalActivity>,
    pub treatment: TreatmentState,
    pub references: Vec<ReferenceRecord>,
    pub known: ActualKnownCountInventory,
    pub prior_report_review: Option<HubRecordId>,
    pub latest_recorded_at: UnixTimestamp,
    pub uncertain_submissions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedCountClassification {
    pub id: HubRecordId,
    pub observation: CorrectionObservationRecord,
    pub original: CountExplanationPlan,
    pub account: CountLossQuantityAccount,
    pub recipient_original: OriginalRecipientAttribution,
    pub recipient_history: Vec<RecipientAttributionCorrection>,
    pub report: Option<ActualReportRecord>,
    pub references: Vec<ReferenceRecord>,
    pub known: ActualKnownCountInventory,
    pub prior_report_review: Option<HubRecordId>,
    pub observation_successor: Option<HubRecordId>,
    pub observation_used_by: Option<HubRecordId>,
    pub latest_recorded_at: UnixTimestamp,
    pub uncertain_submissions: bool,
    pub unresolved_return_association: bool,
}
