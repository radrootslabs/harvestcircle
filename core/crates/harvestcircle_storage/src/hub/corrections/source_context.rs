//! Canonical source input at a named native cut. Never restored from Pending.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedSourceAttribution;
use harvestcircle_domain::hub::{
    corrections::{
        AmendedSource, AttributionAffectedDeparture, AttributionCut, AttributionError,
        OriginalSourceAttribution, SourceAttributionCommand, SourceAttributionContext,
        SourceAttributionCorrection,
    },
    reference::{ReferenceCatalog, SourceAttribution},
};
use std::collections::BTreeMap;

pub(super) struct Context {
    pub hub: HubId,
    pub revision: HubRevision,
    pub catalog: ReferenceCatalog,
    pub original: OriginalSourceAttribution,
    pub history: Vec<SourceAttributionCorrection>,
    pub known_sources: Vec<SourceAttribution>,
    pub affected: Vec<AttributionAffectedDeparture>,
    pub issued_versions:
        Vec<harvestcircle_domain::hub::corrections::AttributionAffectedIssuedVersion>,
    pub latest: UnixTimestamp,
    pub uncertain: bool,
}
impl Context {
    pub fn domain(&self) -> SourceAttributionContext<'_> {
        SourceAttributionContext {
            cut: AttributionCut {
                hub: self.hub,
                stock_revision: self.revision,
                catalog: &self.catalog,
                latest_recorded_at: self.latest,
                uncertain_submissions: self.uncertain,
            },
            original: &self.original,
            history: &self.history,
            known_sources: &self.known_sources,
            affected: &self.affected,
            issued_versions: &self.issued_versions,
        }
    }
    pub fn material(&self, id: HubRecordId) -> HubSubmittedSourceAttribution {
        HubSubmittedSourceAttribution {
            id,
            original: self.original.clone(),
            history: self.history.clone(),
            known_sources: self.known_sources.clone(),
            affected: self.affected.clone(),
            issued_versions: self.issued_versions.clone(),
            latest_recorded_at: self.latest,
            uncertain_submissions: self.uncertain,
        }
    }
}

pub(super) async fn at_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    ledger: &super::source_ledger::Ledger,
    trace: HubRecordId,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<Context, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(at_with_proofs_on(
        &mut reader,
        hub,
        ledger,
        trace,
        revision,
        catalog,
    ))
    .await
}

pub(super) async fn at_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    ledger: &super::source_ledger::Ledger,
    trace: HubRecordId,
    revision: HubRevision,
    catalog: HubRevision,
) -> Result<Context, SafeError> {
    let original = ledger
        .populations
        .iter()
        .find(|p| p.population.id == trace && p.revision.value() <= revision.value())
        .ok_or_else(stock::invalid_input)?;
    let original = OriginalSourceAttribution {
        hub,
        operation: crate::hub::actuals::common::operation_id(&original.operation)?,
        trace,
        stock_revision: original.revision,
        catalog_revision: original.catalog_revision,
        recorded_at: original.recorded_at,
        actor: original.actor,
        source: AmendedSource {
            source: original.population.source.clone(),
            issuer: original.population.source_issuer.clone(),
        },
    };
    let references = reference::load_catalog_through_on(tx, hub, catalog).await?;
    let history = ledger
        .records
        .iter()
        .filter(|c| c.original.trace == trace && c.stamp.stock_revision.value() <= revision.value())
        .cloned()
        .collect();
    let mut origins = ledger
        .populations
        .iter()
        .filter(|p| p.revision.value() <= revision.value())
        .map(|p| (p.revision.value(), p.population.source.clone()))
        .chain(
            ledger
                .records
                .iter()
                .filter(|c| c.stamp.stock_revision.value() <= revision.value())
                .map(|c| (c.stamp.stock_revision.value(), c.after.source.clone())),
        )
        .collect::<Vec<_>>();
    origins.sort_by_key(|p| p.0);
    let mut sources = BTreeMap::new();
    for (_, attribution) in origins {
        if let SourceAttribution::Referenced { source, .. } = &attribution {
            sources.entry(source.id).or_insert(attribution);
        }
    }
    let affected = Box::pin(super::known::attribution_affected_on(
        tx, hub, trace, revision,
    ))
    .await?;
    let issued_versions = ledger
        .issued_versions
        .iter()
        .filter(|v| v.stock_revision.value() <= revision.value() && v.populations.contains(&trace))
        .cloned()
        .collect::<Vec<_>>();
    if issued_versions.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_IMPACTS {
        return Err(stock::capacity());
    }
    let latest:i64=sqlx::query_scalar(concat!(
        "SELECT max(latest) FROM (SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=? ",
        "UNION ALL SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_reference_versions WHERE hub_id=? AND catalog_revision<=?)"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(revision.value()).map_err(|_|corrupt_storage())?).bind(hub.as_bytes().as_slice()).bind(i64::try_from(catalog.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    Ok(Context {
        hub,
        revision,
        catalog: references,
        original,
        history,
        known_sources: sources.into_values().collect(),
        affected,
        issued_versions,
        latest: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        uncertain: false,
    })
}

pub(super) async fn current_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    command: &SourceAttributionCommand,
    except: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
    {
        return Err(stock::conflict());
    }
    let ledger = Box::pin(super::source_ledger::load_on(tx, hub)).await?;
    let mut context = Box::pin(at_on(
        tx,
        hub,
        &ledger,
        command.trace,
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

pub(super) fn error(error: AttributionError) -> SafeError {
    match error {
        AttributionError::Capacity | AttributionError::RevisionExhausted => stock::capacity(),
        AttributionError::StaleRevision | AttributionError::UncertainSubmission => {
            stock::conflict()
        }
        _ => stock::invalid_input(),
    }
}
