//! Live native facts shared by comparison and the owned fresh found executor.
//! An owned context is data; only the submission executor holds admission.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        CorrectionObservedFacts, FoundCorrectionCommand, FoundStockCommand, FoundStockContext,
        FoundStockPlan, PhysicalCorrectionError,
    },
    movement::{HUB_GROUP_CAPACITY, PhysicalIdentification},
    portions::PhysicalLineage,
    reference::{HubLabel, ReferenceCatalog, SourceProvenance},
    treatment::TreatmentState,
};

pub(super) struct Context {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: ReferenceCatalog,
    pub observation: HubCorrectionObservationReceipt,
    pub funding: super::target_funding::LoadedFunding,
    pub lineage: PhysicalLineage,
    pub treatment: TreatmentState,
    pub active_identifications: Vec<PhysicalIdentification>,
    pub group_count: usize,
    pub latest_recorded_at: UnixTimestamp,
    pub uncertain_submissions: bool,
}

impl Context {
    pub fn material(
        &self,
        operation: HubRecordId,
    ) -> harvestcircle_application::hub_submission::HubSubmittedFoundCorrection {
        harvestcircle_application::hub_submission::HubSubmittedFoundCorrection {
            id: operation,
            observation: self.observation.original.clone(),
            quantity: self.funding.quantity.clone(),
            debits: self.funding.debits.clone(),
            lineage: self.lineage.clone(),
            treatment: self.treatment.clone(),
            active_identifications: self.active_identifications.clone(),
            group_count: self.group_count,
            latest_recorded_at: self.latest_recorded_at,
            uncertain_submissions: self.uncertain_submissions,
        }
    }
    pub fn command(&self) -> Result<FoundStockCommand, SafeError> {
        let CorrectionObservedFacts::Found {
            target_operation,
            target_group,
            allocations,
            basis,
        } = &self.observation.original.observation.facts
        else {
            return Err(stock::invalid_input());
        };
        Ok(FoundStockCommand {
            target_operation: *target_operation,
            target_group: *target_group,
            expected_stock_revision: self.stock_revision,
            expected_catalog_revision: self.catalog.revision(),
            allocations: allocations.clone(),
            basis: basis.clone(),
        })
    }

    pub fn plan(
        &self,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<FoundStockPlan, PhysicalCorrectionError> {
        let command = self
            .command()
            .map_err(|_| PhysicalCorrectionError::InvalidScope)?;
        if self
            .group_count
            .checked_add(command.allocations.len())
            .is_none_or(|count| count > HUB_GROUP_CAPACITY)
        {
            return Err(PhysicalCorrectionError::Capacity);
        }
        command.plan(
            &FoundStockContext {
                family: self.funding.family,
                hub: self.hub,
                stock_revision: self.stock_revision,
                catalog: &self.catalog,
                latest_recorded_at: self.latest_recorded_at,
                target: &self.funding.quantity,
                funding: &self.funding.debits,
                lineage: &self.lineage,
                treatment: &self.treatment,
                active_identifications: &self.active_identifications,
                uncertain_submissions: self.uncertain_submissions,
            },
            operation,
            actor,
            now,
        )
    }
}

pub(super) async fn prepare_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &FoundCorrectionCommand,
    except_pending: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if state.revision != command.expected_stock_revision
        || catalog.revision() != command.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    let observed = crate::hub::actuals::common::operation(command.observation_operation)?;
    if observed == *operation {
        return Err(stock::invalid_input());
    }
    let original = stock::read::operation_on(tx, hub, &observed)
        .await?
        .ok_or_else(stock::invalid_input)?;
    let observation = super::original_on(tx, hub, original).await?;
    let context = load_on(
        tx,
        hub,
        state.revision,
        catalog,
        observation,
        except_pending,
    )
    .await?;
    // Checking at the earliest legal native time validates the captured factual
    // scope without inventing a future effect time or persisting a planned effect.
    context
        .plan(
            crate::hub::actuals::common::operation_id(operation)?,
            actor,
            context.latest_recorded_at,
        )
        .map_err(error)?;
    Ok(context)
}

pub(super) fn error(value: PhysicalCorrectionError) -> SafeError {
    match value {
        PhysicalCorrectionError::Capacity => stock::capacity(),
        PhysicalCorrectionError::UnsupportedFamily
        | PhysicalCorrectionError::InvalidScope
        | PhysicalCorrectionError::InvalidQuantity => stock::invalid_input(),
        PhysicalCorrectionError::InvalidHistory
        | PhysicalCorrectionError::Duplicate
        | PhysicalCorrectionError::ReturnConflict
        | PhysicalCorrectionError::UnresolvedReturnScope
        | PhysicalCorrectionError::StaleRevision
        | PhysicalCorrectionError::UncertainSubmission
        | PhysicalCorrectionError::IdentityConflict
        | PhysicalCorrectionError::Treatment => stock::conflict(),
    }
}

/// The optional exception is restricted to an executor's own freshly admitted
/// operation. Comparison/capture supply None and retain every pending outcome.
pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    stock_revision: HubRevision,
    catalog: ReferenceCatalog,
    observation: HubCorrectionObservationReceipt,
    except_pending: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let CorrectionObservedFacts::Found {
        target_operation,
        target_group,
        ..
    } = &observation.original.observation.facts
    else {
        return Err(stock::invalid_input());
    };
    let current:i64=sqlx::query_scalar(concat!(
        "SELECT NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=?) ",
        "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=?)"
    )).bind(observation.operation.as_str()).bind(observation.operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if current != 1 {
        return Err(stock::conflict());
    }
    // Raw observed identifiers may name no UUIDv7 operation. Preserve the fact;
    // failure to qualify its target never means its retained payload is corrupt.
    let target = crate::hub::actuals::common::operation(*target_operation)?;
    let funding =
        super::target_funding::load_on(tx, hub, &target, *target_group, stock_revision).await?;
    let loaded = handling::lineage::load_on(
        tx,
        hub,
        funding.quantity.original().trace,
        &catalog,
        stock_revision,
    )
    .await?;
    let treatment = handling::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    let (group_count, active_identifications) = identifications_on(tx).await?;
    let latest: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=?",
    )
    .bind(hub.as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let latest_recorded_at = UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?;
    let uncertainty = match except_pending {
        Some(operation) => crate::hub::submission::uncertainty_on(tx, operation).await?,
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
    };
    let actual: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_actual_report_reviews WHERE review_operation_id IS NULL",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let open = super::reservations_on(tx).await?;
    if open.operations == 0 || actual < 0 {
        return Err(corrupt_storage());
    }
    Ok(Context {
        hub,
        stock_revision,
        catalog,
        observation,
        funding,
        lineage: loaded.graph,
        treatment,
        active_identifications,
        group_count,
        latest_recorded_at,
        uncertain_submissions: uncertainty.uncertain_intent || actual != 0 || open.operations > 1,
    })
}

async fn identifications_on(
    tx: &mut ServiceSqliteTransaction<'_>,
) -> Result<(usize, Vec<PhysicalIdentification>), SafeError> {
    let groups: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    let groups = usize::try_from(groups).map_err(|_| corrupt_storage())?;
    if groups > HUB_GROUP_CAPACITY {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query(concat!(
        "SELECT substr(CAST(i.physical_label AS BLOB),1,121) AS label,substr(CAST(i.observation AS BLOB),1,513) AS observation ",
        "FROM hub_group_identification i WHERE i.physical_label IS NOT NULL AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0) ORDER BY i.group_id LIMIT ?"
    )).bind(i64::try_from(HUB_GROUP_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > groups {
        return Err(corrupt_storage());
    }
    let result = rows
        .into_iter()
        .map(|row| {
            Ok(PhysicalIdentification {
                label: HubLabel::new(stock::read::text(&row, "label")?)
                    .map_err(|_| corrupt_storage())?,
                observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
                    .map_err(|_| corrupt_storage())?,
            })
        })
        .collect::<Result<Vec<_>, SafeError>>()?;
    Ok((groups, result))
}
