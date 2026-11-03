//! Read-only native originals. Neither a receipt nor a captured cut is an effect grant.
use super::*;
use harvestcircle_domain::hub::actuals::{
    ActualDuplicateLinkPlan, ActualNewDeparturePlan, ActualReportRecord,
    HistoricalHoldScopeAssessment,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualReportReceipt {
    pub operation: DurableRequestId,
    pub original: ActualReportRecord,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubActualReviewOriginal {
    Departure(Box<ActualNewDeparturePlan>),
    Duplicate(Box<ActualDuplicateLinkPlan>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualReviewReceipt {
    pub operation: DurableRequestId,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub original: HubActualReviewOriginal,
    /// Complete sealed native physical cut for a new reconciliation. A duplicate
    /// link never reconstructs or requires a current positive stock group.
    pub lineage: Option<PhysicalLineage>,
    pub historical_holds: Vec<HistoricalHoldScopeAssessment>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubActualReportFilter {
    Unresolved,
    All,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualReportQuery {
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
    pub filter: HubActualReportFilter,
}
impl Default for HubActualReportQuery {
    fn default() -> Self {
        Self {
            expected_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            filter: HubActualReportFilter::Unresolved,
        }
    }
}
impl HubActualReportQuery {
    pub(super) fn validate(&self) -> Result<(), SafeError> {
        if self.limit == 0
            || self.limit > 64
            || self.offset as usize > harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY
            || (self.offset > 0
                && (self.expected_revision.is_none() || self.expected_catalog_revision.is_none()))
        {
            return Err(stock::invalid_input());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualReportRow {
    pub report: HubActualReportReceipt,
    pub review_operation: Option<DurableRequestId>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualReportPage {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubActualReportRow>,
}

/// Current computed comparison only. It is distinct from a persisted receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualDeparturePreview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub plan: ActualNewDeparturePlan,
    pub historical_holds: Vec<HistoricalHoldScopeAssessment>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualLinkPreview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub plan: ActualDuplicateLinkPlan,
}
