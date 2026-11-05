//! Canonical whole-event recipient cuts, independent of physical projections.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRecipientAttribution;
use harvestcircle_domain::hub::{
    corrections::{
        AttributionCut, OriginalRecipientAttribution, RecipientAttributionCommand,
        RecipientAttributionContext, RecipientAttributionCorrection,
    },
    reference::ReferenceCatalog,
};
pub(super) struct Context {
    pub hub: HubId,
    pub revision: HubRevision,
    pub catalog: ReferenceCatalog,
    pub original: OriginalRecipientAttribution,
    pub history: Vec<RecipientAttributionCorrection>,
    pub linked_issued_versions: Vec<HubRecordId>,
    pub latest: UnixTimestamp,
    pub uncertain: bool,
}
impl Context {
    pub fn domain(&self) -> RecipientAttributionContext<'_> {
        RecipientAttributionContext {
            cut: AttributionCut {
                hub: self.hub,
                stock_revision: self.revision,
                catalog: &self.catalog,
                latest_recorded_at: self.latest,
                uncertain_submissions: self.uncertain,
            },
            original: &self.original,
            history: &self.history,
            linked_issued_versions: &self.linked_issued_versions,
        }
    }
    pub fn material(&self, id: HubRecordId) -> HubSubmittedRecipientAttribution {
        HubSubmittedRecipientAttribution {
            id,
            original: self.original.clone(),
            history: self.history.clone(),
            linked_issued_versions: self.linked_issued_versions.clone(),
            latest_recorded_at: self.latest,
            uncertain_submissions: self.uncertain,
        }
    }
}
pub(super) async fn at_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    records: &[RecipientAttributionCorrection],
    target: HubRecordId,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<Context, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(at_with_proofs_on(
        &mut reader,
        hub,
        records,
        target,
        revision,
        catalog,
    ))
    .await
}

pub(super) async fn at_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    records: &[RecipientAttributionCorrection],
    target: HubRecordId,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<Context, SafeError> {
    let operation = crate::hub::actuals::common::operation(target)?;
    let op = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(stock::invalid_input)?;
    if op.revision.value() > revision.value()
        || !matches!(op.kind.as_str(), "departure" | "actual_review")
    {
        return Err(stock::invalid_input());
    }
    if op.kind == "actual_review" && !Box::pin(tx.actual_is_departure(hub, &op)).await? {
        return Err(stock::invalid_input());
    }
    let original = Box::pin(super::known::recipient_on(tx, hub, &operation)).await?;
    let references = reference::load_catalog_through_on(tx, hub, catalog).await?;
    let history = records
        .iter()
        .filter(|r| {
            r.original.operation == target && r.stamp.stock_revision.value() <= revision.value()
        })
        .cloned()
        .collect();
    // Linked correction-document production remains closed. The original's full
    // version list is canonical; future links need their own qualified adapter.
    let documents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_corrections WHERE family='document'")
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if documents != 0 {
        return Err(corrupt_storage());
    }
    let linked_issued_versions = original.issued_versions.clone();
    let latest:i64=sqlx::query_scalar(concat!("SELECT max(latest) FROM (SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=? ","UNION ALL SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_reference_versions WHERE hub_id=? AND catalog_revision<=?)")).bind(hub.as_bytes().as_slice()).bind(i64::try_from(revision.value()).map_err(|_|corrupt_storage())?).bind(hub.as_bytes().as_slice()).bind(i64::try_from(catalog.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    Ok(Context {
        hub,
        revision,
        catalog: references,
        original,
        history,
        linked_issued_versions,
        latest: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        uncertain: false,
    })
}
pub(super) async fn current_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    command: &RecipientAttributionCommand,
    except: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
    {
        return Err(stock::conflict());
    }
    let ledger = Box::pin(super::recipient_ledger::load_on(tx, hub)).await?;
    let mut context = Box::pin(at_on(
        tx,
        hub,
        &ledger.records,
        command.departure_operation,
        state.revision,
        catalog.revision(),
    ))
    .await?;
    context.uncertain = match except {
        Some(id) => crate::hub::submission::uncertainty_on(tx, id).await?,
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
    }
    .uncertain_intent;
    Ok(context)
}
