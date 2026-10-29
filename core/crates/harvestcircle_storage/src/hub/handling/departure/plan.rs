//! Shared read/plan path. Preparing a departure performs no SQL mutations.
use super::super::{lineage, treatment};
use super::*;
use harvestcircle_domain::hub::departure::{
    DepartureCommand, DepartureContext, DepartureError, DepartureTreatment,
};
use harvestcircle_domain::hub::dispatch::{DispatchRecord, DispatchSource, IdentificationStatus};
use harvestcircle_domain::hub::reference::ReferenceCatalog;
use std::collections::BTreeMap;

pub(super) struct Context {
    pub state: stock::read::StockState,
    pub catalog: ReferenceCatalog,
    pub paper: DispatchRecord,
    pub departure: Option<HubDepartureUse>,
}
pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    preparation: HubRecordId,
) -> Result<Context, SafeError> {
    reference::validate_basis_on(tx, hub).await?;
    let context = Context {
        state: stock::read::state_on(tx, hub).await?,
        catalog: reference::load_catalog_on(tx, hub).await?,
        paper: super::super::dispatch::read::record_on(tx, hub, preparation).await?,
        departure: read::usage_on(tx, hub, preparation).await?,
    };
    if let Some(used) = &context.departure {
        let original = stock::read::operation_on(tx, hub, &used.operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        // A blocked review must not hide a damaged retained original.
        let _ = read::original_on(tx, hub, original).await?;
    }
    Ok(context)
}
pub(super) async fn prepare_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    ids: DepartureIdentities,
    command: &DepartureCommand,
    context: &Context,
    now: UnixTimestamp,
) -> Result<DepartureRecord, SafeError> {
    let state = &context.state;
    let catalog = &context.catalog;
    let paper = &context.paper;
    if context.departure.is_some() {
        return Err(unavailable());
    }
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
        || command.expected_preparation_revision != paper.revision
    {
        return Err(conflict());
    }
    let version = paper.versions.last().ok_or_else(unavailable)?;
    if paper.draft.is_some()
        || version.id != command.identification
        || version.status != IdentificationStatus::Issued
        || version.closed.is_some()
    {
        return Err(unavailable());
    }
    let mut traces = BTreeMap::new();
    for line in &version.material.lines {
        if let std::collections::btree_map::Entry::Vacant(slot) = traces.entry(line.trace.id) {
            slot.insert(lineage::load_on(tx, hub, line.trace.id, catalog, state.revision).await?);
        }
    }
    let mut sources = Vec::with_capacity(version.material.lines.len());
    for line in &version.material.lines {
        let loaded = traces.get(&line.trace.id).ok_or_else(corrupt_storage)?;
        loaded.graph.node(line.group).ok_or_else(corrupt_storage)?;
        // The shared current-group reader verifies an existing exit before
        // reporting unavailability, and never hides a damaged original.
        let group = super::super::read::group_on(tx, hub, line.group, catalog).await?;
        sources.push(DispatchSource {
            group,
            trace: loaded.trace.clone(),
        });
    }
    let mut treatments = Vec::with_capacity(traces.len());
    for loaded in traces.values() {
        treatments.push(DepartureTreatment {
            state: treatment::read::state_on(tx, hub, &loaded.graph).await?,
            lineage: loaded.graph.clone(),
        });
    }
    let plan = command
        .plan(
            &DepartureContext {
                hub,
                stock_revision: state.revision,
                catalog,
                paper,
                sources: &sources,
                treatments: &treatments,
                prior_departure: None,
            },
            ids,
            actor,
            now,
        )
        .map_err(departure_error)?;
    let events = u32::try_from(plan.physical_event_count()).map_err(|_| corrupt_storage())?;
    treatment::capacity::ensure_departure_on(tx, hub, events).await?;
    Ok(plan)
}
fn unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubConflict,
        SafeMessage::new(
            "The selected identification or stock is no longer available for departure. Review current records.",
        ),
    )
}
fn departure_error(error: DepartureError) -> SafeError {
    use harvestcircle_domain::hub::{dispatch::DispatchError, treatment::TreatmentError};
    match error {
        DepartureError::AlreadyUsed | DepartureError::IdentificationUnavailable => unavailable(),
        DepartureError::MaterialChanged
        | DepartureError::StaleRevision
        | DepartureError::Dispatch(DispatchError::StaleRevision) => SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "Departure material changed. Review current stock and replace identification where required.",
            ),
        ),
        DepartureError::Treatment(TreatmentError::ActionBlocked) => SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "Current sample acceptance or an active hold blocks normal departure. No stock was removed.",
            ),
        ),
        DepartureError::Capacity
        | DepartureError::RevisionExhausted
        | DepartureError::Treatment(TreatmentError::Capacity) => capacity(),
        DepartureError::Movement(harvestcircle_domain::hub::movement::MovementError::Stock(
            harvestcircle_domain::hub::stock::StockError::InvalidOccurrence,
        )) => SafeError::new(
            SafeErrorCode::HubInvalidInput,
            SafeMessage::new(
                "Review actual occurrence time and the selected stock's causal history.",
            ),
        ),
        _ => corrupt_storage(),
    }
}
