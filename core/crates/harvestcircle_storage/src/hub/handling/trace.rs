//! Read-only original-backed population, physical and action history snapshots.
use super::*;
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error};
use crate::hub::{reference, stock};
use harvestcircle_domain::hub::reference::{ReferenceCatalog, SourceProvenance};
use harvestcircle_domain::hub::trace::{
    TRACE_PAGE_ROWS, TraceBalance, TraceSelection, TraceSnapshot,
};

mod history;
mod index;
mod snapshot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceQuery {
    pub selection: TraceSelection,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
}
impl Default for HubTraceQuery {
    fn default() -> Self {
        Self {
            selection: TraceSelection::default(),
            expected_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: TRACE_PAGE_ROWS,
        }
    }
}
impl HubTraceQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        self.selection.validate().map_err(|_| invalid_input())?;
        validate_page(
            self.expected_revision,
            self.expected_catalog_revision,
            self.offset,
            self.limit,
            harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY,
        )
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceRow {
    pub trace: ReceiptTrace,
    pub balance: TraceBalance,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTracePage {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub selection: TraceSelection,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    pub rows: Vec<HubTraceRow>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceSnapshotQuery {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}
impl HubTraceSnapshotQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        validate_page(
            self.expected_revision,
            self.expected_catalog_revision,
            0,
            1,
            1,
        )
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceHistoryQuery {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
    pub offset: u32,
    pub limit: u32,
}
impl HubTraceHistoryQuery {
    pub fn validate(&self) -> Result<(), SafeError> {
        validate_page(
            self.expected_revision,
            self.expected_catalog_revision,
            self.offset,
            self.limit,
            harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY,
        )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubTraceHistoryKind {
    Receipt,
    Handling(HubHandlingKind),
    Dispatch(dispatch::HubDispatchKind),
    Departure,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceHistoryEntry {
    pub operation: DurableRequestId,
    pub revision: HubRevision,
    pub kind: HubTraceHistoryKind,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    /// None is a bookkeeping action with no claimed physical occurrence.
    pub occurred: Option<Occurrence>,
    pub observation: SourceProvenance,
    pub result: HubRecordId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubTraceHistoryPage {
    pub trace: HubRecordId,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub offset: u32,
    pub total: u32,
    pub more: bool,
    /// Recording order; this never establishes the order of actual occurrences.
    pub rows: Vec<HubTraceHistoryEntry>,
}
fn validate_page(
    stock: Option<HubRevision>,
    catalog: Option<HubRevision>,
    offset: u32,
    limit: u32,
    maximum: usize,
) -> Result<(), SafeError> {
    if !(1..=TRACE_PAGE_ROWS).contains(&limit)
        || offset as usize > maximum
        || stock.is_some() != catalog.is_some()
        || (offset > 0 && stock.is_none())
    {
        return Err(invalid_input());
    }
    Ok(())
}
async fn context_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    expected: Option<HubRevision>,
    expected_catalog: Option<HubRevision>,
) -> Result<(HubId, stock::read::StockState, ReferenceCatalog), SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    if expected.is_some_and(|r| r != state.revision)
        || expected_catalog.is_some_and(|r| r != catalog.revision())
    {
        return Err(conflict());
    }
    Ok((hub.id(), state, catalog))
}

#[cfg(test)]
mod tests;
