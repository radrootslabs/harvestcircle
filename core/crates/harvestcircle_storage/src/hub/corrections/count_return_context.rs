//! Immutable association facts are separate from freshly observed uncertainty.
//! Original recovery must authenticate their exact captured material, never read
//! today's Pending state or infer a saved execution grant.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedCountReturnAssociation;
use harvestcircle_domain::hub::{
    corrections::{
        CountReturnAssociation, CountReturnAssociationCommand, CountReturnAssociationContext,
        NormalReturnProvenance,
    },
    reference::{ReferenceCatalog, ReferenceValue},
    returns::ReturnPlan,
};
use std::sync::Arc;

pub(super) struct Facts {
    pub hub: HubId,
    pub stock: HubRevision,
    pub catalog: ReferenceCatalog,
    pub latest: UnixTimestamp,
    pub returned: ReturnPlan,
    pub return_digest: [u8; 32],
    pub explanation: Arc<super::count_record::HubCountExplanationReceipt>,
    pub known: super::known::HubCorrectionKnownDeparture,
    pub associations: Vec<CountReturnAssociation>,
    pub normal: Vec<NormalReturnProvenance>,
    pub prior_association: Option<HubRecordId>,
    pub prior_trace_correction_rows: usize,
}

pub(super) struct Context {
    pub facts: Facts,
    pub uncertain_submissions: bool,
}

impl Context {
    pub fn domain(&self) -> Result<CountReturnAssociationContext<'_>, SafeError> {
        let f = &self.facts;
        let shared = f
            .known
            .count_explanation
            .as_ref()
            .ok_or_else(corrupt_storage)?;
        Ok(CountReturnAssociationContext {
            hub: f.hub,
            stock_revision: f.stock,
            catalog: &f.catalog,
            latest_recorded_at: f.latest,
            returned: &f.returned,
            return_hub: f.hub,
            return_original_digest: f.return_digest,
            explanation: &f.explanation.original,
            account: &shared.account,
            recipient_original: &f.known.recipient_original,
            recipient_history: &f.known.recipient_history,
            associations: &f.associations,
            normal_returns: &f.normal,
            prior_trace_correction_rows: f.prior_trace_correction_rows,
            prior_association_for_return: f.prior_association,
            uncertain_submissions: self.uncertain_submissions,
        })
    }

    pub fn material(
        &self,
        id: HubRecordId,
    ) -> Result<HubSubmittedCountReturnAssociation, SafeError> {
        let f = &self.facts;
        let shared = f
            .known
            .count_explanation
            .as_ref()
            .ok_or_else(corrupt_storage)?;
        let variant = f
            .catalog
            .get(f.known.original.variant.id)
            .map_err(|_| corrupt_storage())?;
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(corrupt_storage());
        };
        let parties = f
            .known
            .as_domain()
            .associations_through(f.hub, f.stock, f.catalog.revision(), f.latest)
            .map_err(|_| corrupt_storage())?;
        let mut references = [product, variant.id, f.known.recipient_original.recipient.id]
            .into_iter()
            .chain(parties.parties())
            .map(|id| f.catalog.get(id).cloned().map_err(|_| corrupt_storage()))
            .collect::<Result<Vec<_>, _>>()?;
        references.sort_by_key(|r| r.id);
        references.dedup_by_key(|r| r.id);
        Ok(HubSubmittedCountReturnAssociation {
            id,
            returned: f.returned.clone(),
            return_original_digest: f.return_digest,
            explanation: f.explanation.original.clone(),
            account: shared.account.clone(),
            recipient_original: f.known.recipient_original.clone(),
            recipient_history: f.known.recipient_history.clone(),
            associations: f.associations.clone(),
            normal_returns: f.normal.clone(),
            prior_trace_correction_rows: f.prior_trace_correction_rows,
            references,
            prior_association_for_return: f.prior_association,
            latest_recorded_at: f.latest,
            uncertain_submissions: self.uncertain_submissions,
        })
    }
}

pub(super) async fn current_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    command: &CountReturnAssociationCommand,
    except: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    Box::pin(super::count_return_ledger::epoch_on(tx, hub)).await?;
    let state = Box::pin(stock::read::state_with_proofs_on(tx, hub)).await?;
    let catalog = Box::pin(reference::load_catalog_on(tx, hub)).await?;
    if state.revision != command.expected_stock_revision
        || catalog.revision() != command.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    let facts = Box::pin(facts_on(tx, hub, command)).await?;
    let uncertain_submissions = match except {
        Some(own) => crate::hub::submission::uncertainty_on(tx, own).await?,
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
    }
    .uncertain_intent;
    Ok(Context {
        facts,
        uncertain_submissions,
    })
}

pub(super) async fn facts_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    command: &CountReturnAssociationCommand,
) -> Result<Facts, SafeError> {
    Box::pin(super::count_return_ledger::epoch_on(tx, hub)).await?;
    Box::pin(stock::read::journal_state_on(tx, hub)).await?;
    let cut = command.expected_stock_revision;
    let cat = command.expected_catalog_revision;
    let (count,maximum,dependency,latest): (i64,i64,i64,Option<i64>) = sqlx::query_as(concat!(
        "SELECT count(*),COALESCE(max(stock_revision),1),COALESCE(max(catalog_revision),1),max(recorded_at_unix_s) ",
        "FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
        .fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if u64::try_from(maximum).map_err(|_| corrupt_storage())? != cut.value()
        || u64::try_from(dependency).map_err(|_| corrupt_storage())? > cat.value()
    {
        return Err(stock::invalid_input());
    }
    if count.checked_add(1) != Some(maximum) {
        return Err(corrupt_storage());
    }
    let catalog = Box::pin(reference::load_catalog_through_on(tx, hub, cat)).await?;
    let explanation_id = crate::hub::actuals::common::operation(command.explanation_operation)
        .map_err(|_| stock::invalid_input())?;
    let op = Box::pin(stock::read::operation_on(tx, hub, &explanation_id))
        .await?
        .ok_or_else(stock::invalid_input)?;
    if op.kind != "correction_count_explain"
        || op.revision.value() > cut.value()
        || op.catalog_revision.value() > cat.value()
    {
        return Err(stock::invalid_input());
    }
    let explanation = Box::pin(tx.count_explanation(hub, op)).await?;
    let associations = Box::pin(super::count_return_history::associations_on(tx, hub, cut)).await?;
    let returns = Box::pin(super::count_return_history::returns_on(
        tx,
        hub,
        cut,
        command.return_operation,
    ))
    .await?;
    let returned = returns
        .records
        .into_iter()
        .find(|r| r.operation == command.return_operation)
        .ok_or_else(stock::invalid_input)?;
    if returned.original.catalog_revision.value() > cat.value() {
        return Err(stock::invalid_input());
    }
    let recipients = Box::pin(super::recipient_ledger::load_through_with_proofs_on(
        tx,
        hub,
        Some(cut),
    ))
    .await?;
    let mut known = Box::pin(super::count_known::components_on(
        tx,
        hub,
        explanation.original.known_event.trace,
        cut,
        &recipients.records,
        Some(&explanation_id),
    ))
    .await?;
    if known.len() != 1 {
        return Err(corrupt_storage());
    }
    let known = known.pop().ok_or_else(corrupt_storage)?;
    if known.original != explanation.original.known_event {
        return Err(corrupt_storage());
    }
    let prior_association = associations
        .iter()
        .find(|r| r.return_operation == command.return_operation)
        .map(|r| r.operation);
    let prior_trace_correction_rows =
        Box::pin(tx.correction_rows_on(hub, explanation.original.known_event.trace, cut)).await?;
    Ok(Facts {
        hub,
        stock: cut,
        catalog,
        latest: UnixTimestamp::from_seconds(latest.ok_or_else(corrupt_storage)?)
            .ok_or_else(corrupt_storage)?,
        returned: returned.original,
        return_digest: returned.digest,
        explanation,
        known,
        associations,
        normal: returns.normal,
        prior_association,
        prior_trace_correction_rows,
    })
}

pub(in crate::hub) async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &CountReturnAssociationCommand,
) -> Result<HubSubmittedCountReturnAssociation, SafeError> {
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
        return Err(stock::invalid_input());
    }
    let own =
        crate::hub::actuals::common::operation_id(operation).map_err(|_| stock::invalid_input())?;
    let mut reader = OriginalReader::new(tx);
    let context = Box::pin(current_on(&mut reader, hub, command, None)).await?;
    command
        .plan(&context.domain()?, own, actor, context.facts.latest)
        .map_err(error)?;
    context.material(own)
}

pub(super) fn error(
    error: harvestcircle_domain::hub::corrections::CountReturnAssociationError,
) -> SafeError {
    use harvestcircle_domain::hub::corrections::{
        CorrectionError, CountReturnAssociationError as E,
    };
    match error {
        E::InvalidHistory => corrupt_storage(),
        E::Capacity | E::Accounting(CorrectionError::Capacity) => stock::capacity(),
        E::StaleRevision | E::UncertainSubmission | E::AlreadyAssociated => stock::conflict(),
        E::InvalidFacts
        | E::WrongSource
        | E::ConflictingClaim
        | E::ContradictoryTiming
        | E::Accounting(_) => stock::invalid_input(),
    }
}
