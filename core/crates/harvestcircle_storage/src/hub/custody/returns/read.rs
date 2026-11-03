//! Generic population reads stop at this intake; full reads verify one old departure.
use super::*;
use harvestcircle_domain::hub::movement::PhysicalIdentification;
use harvestcircle_domain::hub::reference::{HubLabel, SourceProvenance};
use harvestcircle_domain::hub::returns::{
    ReturnDepartureClaim, return_population_digest, return_result_digest,
};
use harvestcircle_domain::hub::stock::{PopulationOrigin, StockPopulation};
use std::collections::BTreeMap;

pub(super) struct Own {
    pub ids: ReturnIdentities,
    pub command: ReturnCommand,
    pub catalog: ReferenceCatalog,
    pub population: StockPopulation,
    pub birth: harvestcircle_domain::hub::movement::PhysicalStockGroup,
    pub work: Option<harvestcircle_domain::hub::movement::WorkRequirement>,
    pub known: Option<HubRecordId>,
    pub established: Option<HubRecordId>,
    pub source_count: usize,
    pub resolution: String,
    pub reason: Option<String>,
    pub timing: String,
    pub result_hash: [u8; 32],
}
pub(super) async fn own_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Own, SafeError> {
    if op.kind != "return" {
        return Err(stock::conflict());
    }
    let ids = identities(&op.id).map_err(|_| corrupt_storage())?;
    let row=sqlx::query(concat!(
        "SELECT substr(return_id,1,17) AS return_id,substr(population_id,1,17) AS population_id,policy_version,",
        "substr(returned_by_id,1,17) AS returned_by_id,returned_by_revision,substr(claimed_departure_id,1,17) AS claimed_departure_id,substr(claimed_group_id,1,17) AS claimed_group_id,",
        "substr(known_departure_id,1,17) AS known_departure_id,substr(established_group_id,1,17) AS established_group_id,substr(resolution,1,33) AS resolution,substr(unresolved_reason,1,33) AS unresolved_reason,substr(timing,1,33) AS timing,source_count,substr(intake_work_id,1,17) AS intake_work_id,",
        "substr(population_hash,1,33) AS population_hash,substr(result_hash,1,33) AS result_hash FROM hub_returns WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if op.result != ids.returned
        || record_id(&row, "return_id")? != ids.returned
        || record_id(&row, "population_id")? != ids.population
        || row
            .try_get::<i64, _>("policy_version")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let fields = stock::read::population_fields_for_on(tx, hub, op, ids.population).await?;
    if fields.origin_kind != "return"
        || fields.group != ids.group
        || fields.supplier.is_some()
        || fields.producer.is_some()
    {
        return Err(corrupt_storage());
    }
    let birth=sqlx::query("SELECT substr(g.trace_id,1,17) AS trace_id,substr(i.parent_group_id,1,17) AS parent_id,substr(CAST(i.physical_label AS BLOB),1,121) AS label,substr(CAST(i.observation AS BLOB),1,513) AS identification_observation,substr(i.created_operation_id,1,37) AS created_operation FROM hub_group_identification i JOIN hub_stock_groups g ON g.group_id=i.group_id WHERE i.group_id=? LIMIT 1")
        .bind(ids.group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if record_id(&birth, "trace_id")? != ids.population
        || stock::read::optional_id(&birth, "parent_id")?.is_some()
        || birth
            .try_get::<String, _>("created_operation")
            .map_err(|_| corrupt_storage())?
            != op.id.as_str()
    {
        return Err(corrupt_storage());
    }
    let returned_by = stock::read::optional_id(&row, "returned_by_id")?;
    let by_revision: Option<i64> = row
        .try_get("returned_by_revision")
        .map_err(|_| corrupt_storage())?;
    let mut records: BTreeMap<_, _> = fields
        .catalog
        .records()
        .cloned()
        .map(|r| (r.id, r))
        .collect();
    match (returned_by, by_revision) {
        (None, None) => {}
        (Some(id), Some(revision)) => {
            let value = reference::record_at_catalog_on(tx, hub, id, op.catalog_revision).await?;
            if value.revision != stock::read::revision(revision)?
                || value.value.kind() != ReferenceKind::Party
            {
                return Err(corrupt_storage());
            }
            if let Some(previous) = records.insert(id, value.clone())
                && previous != value
            {
                return Err(corrupt_storage());
            }
        }
        _ => return Err(corrupt_storage()),
    }
    let catalog = ReferenceCatalog::restore(op.catalog_revision, records.into_values().collect())
        .map_err(|_| corrupt_storage())?;
    let claimed = stock::read::optional_id(&row, "claimed_departure_id")?;
    let claimed_group = stock::read::optional_id(&row, "claimed_group_id")?;
    let departure = match (claimed, claimed_group) {
        (None, None) => None,
        (Some(departure), group) => Some(ReturnDepartureClaim { departure, group }),
        _ => return Err(corrupt_storage()),
    };
    let command = ReturnCommand {
        expected_stock_revision: HubRevision::from_value(
            op.revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt_storage)?,
        )
        .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: op.catalog_revision,
        variant: fields.variant,
        location: fields.initial_location,
        returned_by,
        source: fields.source,
        packs: fields.packs,
        identification: PhysicalIdentification {
            label: HubLabel::new(stock::read::text(&birth, "label")?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(stock::read::text(
                &birth,
                "identification_observation",
            )?)
            .map_err(|_| corrupt_storage())?,
        },
        planned_location: fields.planned_location,
        departure,
        occurred: fields.occurred,
        observation: fields.observation,
    };
    if command.fingerprint(hub, op.actor, ids) != op.hash {
        return Err(corrupt_storage());
    }
    let work = stock::read::optional_id(&row, "intake_work_id")?;
    if work != command.planned_location.map(|_| ids.requirement) {
        return Err(corrupt_storage());
    }
    if let Some(work) = work {
        let saved=sqlx::query("SELECT substr(trace_id,1,17) AS trace_id,substr(variant_id,1,17) AS variant_id,substr(target_location_id,1,17) AS target_id,target_location_revision,required,substr(created_operation_id,1,37) AS created_operation FROM hub_work_requirements WHERE work_id=? LIMIT 1")
            .bind(work.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
        let target = catalog
            .require_kind(
                command.planned_location.ok_or_else(corrupt_storage)?,
                ReferenceKind::Location,
            )
            .map_err(|_| corrupt_storage())?;
        if record_id(&saved, "trace_id")? != ids.population
            || record_id(&saved, "variant_id")? != command.variant
            || record_id(&saved, "target_id")? != target.id
            || revision_field(&saved, "target_location_revision")? != target.revision
            || whole_packs(saved.try_get("required").map_err(|_| corrupt_storage())?)?
                != command.packs
            || saved
                .try_get::<String, _>("created_operation")
                .map_err(|_| corrupt_storage())?
                != op.id.as_str()
        {
            return Err(corrupt_storage());
        }
    }
    let known = stock::read::optional_id(&row, "known_departure_id")?;
    let source_count = usize::try_from(
        row.try_get::<i64, _>("source_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if source_count > 64
        || (known.is_none() && source_count != 0)
        || (known.is_some() && (source_count == 0 || known != claimed))
    {
        return Err(corrupt_storage());
    }
    // A later matching ID does not retroactively attribute an old unrecorded claim.
    if known.is_none()
        && let Some(claim) = claimed
    {
        let already_known:i64=sqlx::query_scalar("SELECT count(*) FROM hub_departures d JOIN hub_stock_operations o ON o.operation_id=d.operation_id WHERE d.departure_id=? AND o.stock_revision<?")
            .bind(claim.as_bytes().as_slice()).bind(integer(op.revision)?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if already_known != 0 {
            return Err(corrupt_storage());
        }
    }
    let mut intake_command = command.clone();
    intake_command.departure = None;
    let intake = intake_command
        .plan(
            &ReturnContext {
                hub,
                stock_revision: command.expected_stock_revision,
                catalog: &catalog,
                departure: None,
                established_return_totals: &[],
            },
            ids,
            op.actor,
            op.recorded_at,
        )
        .map_err(|_| corrupt_storage())?;
    let population = intake.population();
    if population.origin
        != (PopulationOrigin::Returned {
            returned: ids.returned,
        })
    {
        return Err(corrupt_storage());
    }
    let population_hash: Vec<u8> = row
        .try_get("population_hash")
        .map_err(|_| corrupt_storage())?;
    if return_population_digest(&op.hash, hub, op.catalog_revision, ids, &population)
        .map_err(|_| corrupt_storage())?
        .as_slice()
        != population_hash
    {
        return Err(corrupt_storage());
    }
    Ok(Own {
        ids,
        command,
        catalog,
        population,
        birth: intake.group,
        work: intake.intake_work,
        known,
        established: stock::read::optional_id(&row, "established_group_id")?,
        source_count,
        resolution: row.try_get("resolution").map_err(|_| corrupt_storage())?,
        reason: row
            .try_get("unresolved_reason")
            .map_err(|_| corrupt_storage())?,
        timing: row.try_get("timing").map_err(|_| corrupt_storage())?,
        result_hash: row
            .try_get::<Vec<u8>, _>("result_hash")
            .map_err(|_| corrupt_storage())?
            .try_into()
            .map_err(|_| corrupt_storage())?,
    })
}

pub(in crate::hub) async fn population_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<StockPopulation, SafeError> {
    Ok(own_on(tx, hub, &op).await?.population)
}

pub(in crate::hub) async fn birth_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<
    (
        StockPopulation,
        harvestcircle_domain::hub::movement::PhysicalStockGroup,
    ),
    SafeError,
> {
    let own = own_on(tx, hub, op).await?;
    Ok((own.population, own.birth))
}
pub(in crate::hub) async fn intake_work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Option<harvestcircle_domain::hub::movement::WorkRequirement>, SafeError> {
    Ok(own_on(tx, hub, op).await?.work)
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubReturnReceipt, SafeError> {
    let own = own_on(tx, hub, &op).await?;
    let departure = match own.known {
        None => None,
        Some(id) => Some(
            context::departure_on(tx, hub, id, own.command.expected_stock_revision)
                .await?
                .ok_or_else(corrupt_storage)?,
        ),
    };
    let rows=sqlx::query("SELECT ordinal,substr(departure_operation_id,1,37) AS departure_operation,substr(original_group_id,1,17) AS original_group_id,previously_returned_packs FROM hub_return_sources WHERE return_id=? ORDER BY ordinal LIMIT 65")
        .bind(own.ids.returned.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != own.source_count {
        return Err(corrupt_storage());
    }
    let mut totals = Vec::with_capacity(rows.len());
    for (n, row) in rows.iter().enumerate() {
        let d = departure.as_ref().ok_or_else(corrupt_storage)?;
        let lines = &d
            .paper
            .versions
            .last()
            .ok_or_else(corrupt_storage)?
            .material
            .lines;
        let line = lines.get(n).ok_or_else(corrupt_storage)?;
        let source_op = DurableRequestId::parse(
            row.try_get::<String, _>("departure_operation")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let source_id = HubRecordId::from_hex(&source_op.as_str().replace('-', ""))
            .map_err(|_| corrupt_storage())?;
        let group = record_id(row, "original_group_id")?;
        let prior = whole_packs(
            row.try_get("previously_returned_packs")
                .map_err(|_| corrupt_storage())?,
        )?;
        if row
            .try_get::<i64, _>("ordinal")
            .map_err(|_| corrupt_storage())?
            != i64::try_from(n).map_err(|_| corrupt_storage())?
            || source_id != d.operation
            || group != line.group
        {
            return Err(corrupt_storage());
        }
        let expected:i64=sqlx::query_scalar("SELECT COALESCE(sum(p.original_packs),0) FROM hub_returns prior JOIN hub_receipts p ON p.trace_id=prior.population_id JOIN hub_stock_operations o ON o.operation_id=prior.operation_id WHERE prior.known_departure_id=? AND prior.established_group_id=? AND prior.resolution='established' AND o.stock_revision<?")
            .bind(d.id.as_bytes().as_slice()).bind(group.as_bytes().as_slice()).bind(integer(op.revision)?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if prior != whole_packs(expected)? {
            return Err(corrupt_storage());
        }
        totals.push(EstablishedReturnTotal {
            original_group: group,
            packs: prior,
        });
    }
    let context = ReturnContext {
        hub,
        stock_revision: own.command.expected_stock_revision,
        catalog: &own.catalog,
        departure: departure.as_ref(),
        established_return_totals: &totals,
    };
    let plan = own
        .command
        .plan(&context, own.ids, op.actor, op.recorded_at)
        .map_err(|_| corrupt_storage())?;
    let (resolution, reason) = super::resolution(&plan.linkage.resolution);
    if plan.linkage.established_group() != own.established
        || resolution != own.resolution
        || reason != own.reason.as_deref()
        || super::timing(plan.linkage.timing) != own.timing
        || plan.population() != own.population
        || return_result_digest(&op.hash, &own.command, &context, &plan)
            .map_err(|_| corrupt_storage())?
            != own.result_hash
    {
        return Err(corrupt_storage());
    }
    Ok(HubReturnReceipt {
        operation: op.id,
        original: plan,
    })
}
fn integer(revision: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(revision.value()).map_err(|_| corrupt_storage())
}
