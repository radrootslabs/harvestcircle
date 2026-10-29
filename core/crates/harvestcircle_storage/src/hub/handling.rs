//! Current physical scope and work use the existing lifetime stock journal.

pub(crate) mod departure;
pub(in crate::hub) mod discrepancy;
pub(crate) mod dispatch;
mod history;
pub(in crate::hub) mod lineage;
pub(crate) mod portion_schema;
mod portions;
mod query;
pub(in crate::hub) mod read;
pub(crate) mod schema;
pub(crate) mod trace;
pub(crate) mod treatment;
mod work_write;
pub(in crate::hub) mod write;

fn integer(revision: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(revision.value()).map_err(|_| crate::db::corrupt_storage())
}

fn occurrence_fields(value: Occurrence) -> (&'static str, Option<i64>, Option<i64>) {
    match value {
        Occurrence::Unknown => ("unknown", None, None),
        Occurrence::Known(at) => ("known", Some(at.as_seconds()), Some(at.as_seconds())),
        Occurrence::Interval { earliest, latest } => (
            "interval",
            Some(earliest.as_seconds()),
            Some(latest.as_seconds()),
        ),
    }
}

use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::movement::{
    Discrepancy, HUB_GROUP_CAPACITY, HUB_WORK_CAPACITY, HandlingIdentities, MovementError,
    PhysicalIdentification, PhysicalStockGroup, WorkRequirement,
};
use harvestcircle_domain::hub::reference::{
    HUB_LABEL_UTF8_BYTES, HubLabel, HubRecordId, ReferenceRecord, WholePacks,
};
use harvestcircle_domain::hub::stock::ReceiptPlan;
use harvestcircle_domain::hub::stock::{HUB_STOCK_PAGE_ROWS, Occurrence, StockPopulation};
use harvestcircle_domain::hub::{HubId, HubRevision};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;

use super::stock::{capacity, conflict, invalid_input};
use crate::db::storage_unavailable;
pub use treatment::{
    HubHoldRow, HubTreatmentPage, HubTreatmentQuery, HubTreatmentResult, HubTreatmentScope,
    HubTreatmentSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubHandlingKind {
    Move,
    Split,
    Acceptance,
    PlaceHold,
    ReleaseHold,
    Replan,
    NewWork,
    OpenDiscrepancy,
    ResolveDiscrepancy,
}

impl HubHandlingKind {
    pub(super) fn parse(value: &str) -> Result<Self, SafeError> {
        match value {
            "move" => Ok(Self::Move),
            "split" => Ok(Self::Split),
            "acceptance" => Ok(Self::Acceptance),
            "hold_place" => Ok(Self::PlaceHold),
            "hold_release" => Ok(Self::ReleaseHold),
            "work_replan" => Ok(Self::Replan),
            "work_new" => Ok(Self::NewWork),
            "discrepancy_open" => Ok(Self::OpenDiscrepancy),
            "discrepancy_resolve" => Ok(Self::ResolveDiscrepancy),
            "receipt" | "opening" => Err(conflict()),
            _ => Err(crate::db::corrupt_storage()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubWorkChange {
    pub before: Option<WorkRequirement>,
    pub after: WorkRequirement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubMovementResult {
    pub selected_group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub source_before_packs: WholePacks,
    pub from: ReferenceRecord,
    pub to: ReferenceRecord,
    pub packs: WholePacks,
    pub moved_group: HubRecordId,
    pub remaining_packs: WholePacks,
    pub separated_portion: Option<PhysicalIdentification>,
    pub occurred: Occurrence,
    pub observation: harvestcircle_domain::hub::reference::SourceProvenance,
    pub work_changes: Vec<HubWorkChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubSplitResult {
    pub selected_group: HubRecordId,
    pub expected_group_revision: HubRevision,
    pub source_before_packs: WholePacks,
    pub location: ReferenceRecord,
    pub packs: WholePacks,
    pub portion_group: HubRecordId,
    pub remaining_packs: WholePacks,
    pub identification: PhysicalIdentification,
    pub occurred: Occurrence,
    pub observation: harvestcircle_domain::hub::reference::SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActionGroup {
    pub group: HubRecordId,
    pub before_revision: HubRevision,
    pub packs: WholePacks,
    pub location: ReferenceRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubWorkActionResult {
    pub replacement: WorkRequirement,
    pub previous: Option<HubWorkChange>,
    pub groups: Vec<HubActionGroup>,
    pub observation: harvestcircle_domain::hub::reference::SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubHandlingResult {
    Moved(HubMovementResult),
    Separated(HubSplitResult),
    Treatment(Box<HubTreatmentResult>),
    WorkChanged(HubWorkActionResult),
    DiscrepancyChanged(Discrepancy),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubHandlingReceipt {
    pub operation: DurableRequestId,
    pub kind: HubHandlingKind,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub trace: HubRecordId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub result: HubHandlingResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubGroupView {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub group: PhysicalStockGroup,
    pub location: ReferenceRecord,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub trace: StockPopulation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubGroupCreationKind {
    Receipt,
    CountedOpening,
    CountedSurplus,
    Movement,
    Separation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubLineageRow {
    pub id: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub identification: Option<PhysicalIdentification>,
    pub packs: WholePacks,
    pub revision: HubRevision,
    pub location: Option<ReferenceRecord>,
    pub created_revision: HubRevision,
    pub created_kind: HubGroupCreationKind,
    pub created_by: PublicKey,
    pub created_at: UnixTimestamp,
    pub created_operation: DurableRequestId,
    pub occurred: Occurrence,
    pub observation: harvestcircle_domain::hub::reference::SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubLineagePage {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub trace: StockPopulation,
    pub rows: Vec<HubLineageRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubLineageQuery {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
}

impl HubLineageQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        if self.limit == 0
            || self.limit as usize > HUB_STOCK_PAGE_ROWS
            || self.offset as usize > harvestcircle_domain::hub::portions::HUB_LINEAGE_GROUPS
        {
            return Err(invalid_input());
        }
        if self.offset != 0
            && (self.expected_revision.is_none() || self.expected_catalog_revision.is_none())
        {
            return Err(conflict());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubWorkRow {
    pub work: WorkRequirement,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub source_needs_review: bool,
    pub pending_groups: u32,
    pub origin: HubWorkOrigin,
    pub observation: harvestcircle_domain::hub::reference::SourceProvenance,
    pub created_by: PublicKey,
    pub created_at: UnixTimestamp,
    pub created_operation: DurableRequestId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubWorkOrigin {
    Intake,
    Replan,
    NewHandling,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDiscrepancyRow {
    pub value: Discrepancy,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub created_by: PublicKey,
    pub created_at: UnixTimestamp,
    pub created_operation: DurableRequestId,
    pub resolved_by: Option<PublicKey>,
    pub resolved_at: Option<UnixTimestamp>,
    pub resolved_operation: Option<DurableRequestId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubHandlingPage<T> {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<T>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubHandlingQuery {
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
    pub trace: Option<HubRecordId>,
    pub open_only: bool,
    pub text: String,
}

impl Default for HubHandlingQuery {
    fn default() -> Self {
        Self {
            expected_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            trace: None,
            open_only: true,
            text: String::new(),
        }
    }
}

impl HubHandlingQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        if self.limit == 0
            || self.limit as usize > HUB_STOCK_PAGE_ROWS
            || self.offset as usize > HUB_WORK_CAPACITY
            || self.text.len() > HUB_LABEL_UTF8_BYTES
        {
            return Err(invalid_input());
        }
        if !self.text.is_empty() {
            HubLabel::new(self.text.clone()).map_err(|_| invalid_input())?;
        }
        if self.offset != 0
            && (self.expected_revision.is_none() || self.expected_catalog_revision.is_none())
        {
            return Err(conflict());
        }
        Ok(())
    }
}

fn handling_ids(operation: &DurableRequestId) -> Result<HandlingIdentities, SafeError> {
    let id =
        HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| invalid_input())?;
    HandlingIdentities::from_operation_bytes(id).map_err(handling_error)
}

fn handling_error(error: MovementError) -> SafeError {
    match error {
        MovementError::StaleRevision
        | MovementError::InsufficientStock
        | MovementError::IdentityConflict
        | MovementError::NoRemainingWork
        | MovementError::AlreadyResolved => conflict(),
        MovementError::RevisionExhausted | MovementError::Capacity => capacity(),
        MovementError::PhysicalIdentificationRequired => SafeError::new(
            SafeErrorCode::HubInvalidInput,
            SafeMessage::new(
                "Identify a tagged or separately distinguishable portion before moving part of a stock group.",
            ),
        ),
        MovementError::Stock(harvestcircle_domain::hub::stock::StockError::InvalidOccurrence) => {
            SafeError::new(
                SafeErrorCode::HubInvalidInput,
                SafeMessage::new(
                    "The reported time contradicts physical history or the recording clock. Retain the observation for actual-event review.",
                ),
            )
        }
        _ => invalid_input(),
    }
}

pub(super) async fn seed_receipt_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &ReceiptPlan,
) -> Result<(), SafeError> {
    let group_count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    let work_count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_work_requirements")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    if group_count > HUB_GROUP_CAPACITY as i64 || work_count >= HUB_WORK_CAPACITY as i64 {
        return Err(capacity());
    }
    let target = plan.trace.planned_location.as_ref();
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,NULL,NULL,NULL,?)")
        .bind(plan.position.group.as_bytes().as_slice()).bind(operation.as_str())
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_work_requirements (work_id,trace_id,variant_id,target_location_id,target_location_revision,required,completed,no_longer_required,remaining,revision,created_operation_id) VALUES (?,?,?,?,?,?,0,0,?,1,?)")
        .bind(plan.trace.id.as_bytes().as_slice()).bind(plan.trace.id.as_bytes().as_slice())
        .bind(plan.position.variant.as_bytes().as_slice()).bind(target.map(|record| record.id.as_bytes().as_slice()))
        .bind(target.map(|record| i64::try_from(record.revision.value()).expect("checked hub revision")))
        .bind(i64::from(plan.put_away.required().value())).bind(i64::from(plan.put_away.remaining().value()))
        .bind(operation.as_str()).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) VALUES (?,?,'pending',?)")
        .bind(plan.position.group.as_bytes().as_slice()).bind(plan.trace.id.as_bytes().as_slice())
        .bind(i64::from(plan.position.packs.value())).execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod portion_tests;

#[cfg(test)]
mod historical_fixture;
#[cfg(test)]
pub(crate) use historical_fixture::{seed_schema7_handling, seed_schema7_receipt};
