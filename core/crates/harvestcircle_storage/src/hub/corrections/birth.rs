//! Shallow independently sealed birth projections. These readers never replan
//! prior corrections, execute observed input or reconstruct an admission grant.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualPhysicalBirth, ActualPhysicalBirthKind},
    corrections::{CorrectionObservedFacts, found_stock_group_id},
    movement::PhysicalStockGroup,
    portions::PhysicalRecoveryOrigin,
    reference::{ReferenceKind, ReferenceRecord, SourceProvenance},
    stock::Occurrence,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BIRTH_BYTES: usize = 16 * 1024;

/// Additional immutable output facts not already retained by the exact sealed
/// observation. Raw debit identifiers become evidence only after SQL correlation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BirthData {
    pub schema: u8,
    pub location_revision: u64,
    pub debit_packs: u32,
    pub debit_before_packs: u32,
    pub debit_before_group_revision: u64,
    pub debit_after_group_revision: u64,
    pub debit_result_hash: [u8; 32],
    pub debit_kind: String,
    pub debit_actor: String,
    pub debit_input_hash: [u8; 32],
    pub debit_catalog_revision: u64,
    pub debit_result: String,
    pub debit_recorded_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::hub) struct FoundBirthProjection {
    pub group: PhysicalStockGroup,
    pub location: ReferenceRecord,
    pub birth: ActualPhysicalBirth,
    pub recovery: PhysicalRecoveryOrigin,
    pub basis: SourceProvenance,
    pub cohort_evidence: SourceProvenance,
    pub observation_operation: HubRecordId,
    pub target_operation: HubRecordId,
    pub target_group: HubRecordId,
    pub allocation: u8,
}

pub(super) fn seal(
    op: &OperationRecord,
    hub: HubId,
    data: &BirthData,
    birth: &FoundBirthProjection,
) -> Result<[u8; 32], SafeError> {
    let operation =
        crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    if data.schema != 1
        || !matches!(
            data.debit_kind.as_str(),
            "departure" | "actual_review" | "correction_additional"
        )
        || data.debit_actor.len() != 64
        || PublicKey::from_hex(&data.debit_actor).is_err()
        || data.debit_result.len() != 32
        || HubRecordId::from_hex(&data.debit_result).is_err()
        || data.location_revision == 0
        || data.debit_catalog_revision == 0
        || data.debit_recorded_at < 0
        || data.debit_recorded_at > op.recorded_at.as_seconds()
        || data.debit_packs == 0
        || data.debit_packs > data.debit_before_packs
        || data.debit_before_packs > 1_000_000
        || data.debit_before_group_revision == 0
        || data.debit_before_group_revision.checked_add(1) != Some(data.debit_after_group_revision)
        || data.debit_before_group_revision >= birth.recovery.debit_revision.value()
        || op.kind != "correction_found"
        || birth.birth.hub != hub
        || birth.birth.operation != operation
        || birth.birth.actor != op.actor
        || birth.birth.stock_revision != op.revision
        || birth.birth.recorded_at != op.recorded_at
        || birth.birth.occurred != Occurrence::Known(op.recorded_at)
        || birth.birth.kind
            != (ActualPhysicalBirthKind::FoundRemainder {
                allocation: birth.allocation,
            })
        || birth.group.id
            != found_stock_group_id(operation, birth.allocation).map_err(|_| corrupt_storage())?
        || birth.group.revision != HubRevision::INITIAL
        || !birth.group.work.is_empty()
        || birth.group.identification.is_none()
        || birth.group.packs.value() == 0
        || birth.group.packs.value() > data.debit_packs
        || birth.group.causal_floor != Some(op.recorded_at)
        || birth.group.parent.is_none()
        || birth.group.parent != birth.birth.parent
        || birth.group.id != birth.birth.group
        || birth.group.trace != birth.birth.trace
        || birth.group.variant != birth.birth.variant
        || birth.group.packs != birth.birth.packs
        || birth.group.location != birth.location.id
        || birth.location.value.kind() != ReferenceKind::Location
        || birth.location.revision.value() != data.location_revision
        || birth.recovery.debit_operation == operation
        || birth.recovery.debit_revision.value() >= op.revision.value()
    {
        return Err(corrupt_storage());
    }
    birth.group.validate().map_err(|_| corrupt_storage())?;
    #[derive(Serialize)]
    struct Projection<'a> {
        operation: &'a str,
        kind: &'a str,
        result: HubRecordId,
        hub: HubId,
        actor: PublicKey,
        stock_revision: HubRevision,
        catalog_revision: HubRevision,
        recorded_at: UnixTimestamp,
        data: &'a BirthData,
        birth: &'a FoundBirthProjection,
    }
    let bytes = serde_json::to_vec(&Projection {
        operation: op.id.as_str(),
        kind: &op.kind,
        result: op.result,
        hub,
        actor: op.actor,
        stock_revision: op.revision,
        catalog_revision: op.catalog_revision,
        recorded_at: op.recorded_at,
        data,
        birth,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > 32 * 1024 {
        return Err(corrupt_storage());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.found.birth.projection.v1\0");
    hash.update(op.hash);
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(in crate::hub) async fn birth_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    child: HubRecordId,
) -> Result<FoundBirthProjection, SafeError> {
    if op.kind != "correction_found" {
        return Err(corrupt_storage());
    }
    let row=sqlx::query(concat!(
        "SELECT a.allocation,substr(a.debit_operation_id,1,37) AS debit_operation,substr(a.debit_group_id,1,17) AS debit_group,",
        "a.debit_stock_revision,substr(a.child_group_id,1,17) AS child_group,a.packs,a.birth_schema,substr(a.birth_payload,1,16385) AS birth_payload,",
        "substr(a.projection_hash,1,33) AS projection_hash,substr(c.target_operation_id,1,37) AS target_operation,",
        "substr(c.subject_id,1,17) AS target_group,substr(c.observation_operation_id,1,37) AS observation_operation,",
        "substr(c.correction_id,1,17) AS correction_id,substr(g.trace_id,1,17) AS trace ",
        "FROM hub_correction_found_allocations a JOIN hub_corrections c ON c.operation_id=a.operation_id ",
        "JOIN hub_stock_groups g ON g.group_id=a.child_group_id WHERE a.operation_id=? AND a.child_group_id=? AND c.family='found' LIMIT 1"
    )).bind(op.id.as_str()).bind(child.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if stock::read::record_id(&row, "correction_id")? != op.result
        || stock::read::record_id(&row, "child_group")? != child
        || row
            .try_get::<i64, _>("birth_schema")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row
        .try_get("birth_payload")
        .map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > BIRTH_BYTES {
        return Err(corrupt_storage());
    }
    let data: BirthData = serde_json::from_slice(&bytes).map_err(|_| corrupt_storage())?;
    if data.schema != 1 || serde_json::to_vec(&data).map_err(|_| corrupt_storage())? != bytes {
        return Err(corrupt_storage());
    }
    let allocation = u8::try_from(
        row.try_get::<i64, _>("allocation")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let operation =
        crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    if child != found_stock_group_id(operation, allocation).map_err(|_| corrupt_storage())? {
        return Err(corrupt_storage());
    }
    let parse_operation = |name| -> Result<DurableRequestId, SafeError> {
        DurableRequestId::parse(
            row.try_get::<String, _>(name)
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())
    };
    let observation_id = parse_operation("observation_operation")?;
    let observed_op = stock::read::operation_on(tx, hub, &observation_id)
        .await?
        .ok_or_else(corrupt_storage)?;
    if observed_op.revision.value() >= op.revision.value()
        || observed_op.recorded_at > op.recorded_at
    {
        return Err(corrupt_storage());
    }
    let observed = super::original_on(tx, hub, observed_op).await?.original;
    let CorrectionObservedFacts::Found {
        target_operation,
        target_group,
        allocations,
        basis,
    } = &observed.observation.facts
    else {
        return Err(corrupt_storage());
    };
    let (count,first,last):(i64,Option<i64>,Option<i64>)=sqlx::query_as("SELECT count(*),min(allocation),max(allocation) FROM hub_correction_found_allocations WHERE operation_id=?")
        .bind(op.id.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if count != i64::try_from(allocations.len()).map_err(|_| corrupt_storage())?
        || first != Some(0)
        || last != Some(count - 1)
    {
        return Err(corrupt_storage());
    }
    let input = allocations
        .get(usize::from(allocation))
        .ok_or_else(corrupt_storage)?;
    let target_id = parse_operation("target_operation")?;
    let debit_id = parse_operation("debit_operation")?;
    let parent = stock::read::record_id(&row, "debit_group")?;
    let trace = stock::read::record_id(&row, "trace")?;
    let debit_revision = stock::read::revision_field(&row, "debit_stock_revision")?;
    if crate::hub::actuals::common::operation_id(&target_id).map_err(|_| corrupt_storage())?
        != *target_operation
        || stock::read::record_id(&row, "target_group")? != *target_group
        || input.debit.group != parent
        || crate::hub::actuals::common::operation_id(&debit_id).map_err(|_| corrupt_storage())?
            != input.debit.operation
        || input.packs.value() == 0
        || input.packs.value() > data.debit_packs
        || input.packs
            != stock::read::whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?
    {
        return Err(corrupt_storage());
    }
    let native=sqlx::query(concat!(
        "SELECT substr(parent.trace_id,1,17) AS parent_trace,substr(population.variant_id,1,17) AS variant,",
        "substr(identity.parent_group_id,1,17) AS parent,substr(identity.created_operation_id,1,37) AS created_operation,",
        "substr(CAST(identity.physical_label AS BLOB),1,121) AS label,substr(CAST(identity.observation AS BLOB),1,513) AS observation,",
        "created.stock_revision AS parent_created_revision ",
        "FROM hub_group_identification identity JOIN hub_stock_groups parent ON parent.group_id=identity.parent_group_id ",
        "JOIN hub_receipts population ON population.trace_id=parent.trace_id ",
        "JOIN hub_group_identification old ON old.group_id=parent.group_id JOIN hub_stock_operations created ON created.operation_id=old.created_operation_id ",
        "WHERE identity.group_id=? LIMIT 1"
    )).bind(child.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if stock::read::record_id(&native, "parent")? != parent
        || stock::read::record_id(&native, "parent_trace")? != trace
        || native
            .try_get::<String, _>("created_operation")
            .map_err(|_| corrupt_storage())?
            != op.id.as_str()
        || stock::read::text(&native, "label")? != input.identification.label.as_str()
        || stock::read::text(&native, "observation")? != input.identification.observation.as_str()
        || stock::read::revision_field(&native, "parent_created_revision")?.value()
            >= debit_revision.value()
        || debit_revision.value() >= op.revision.value()
    {
        return Err(corrupt_storage());
    }
    let variant = stock::read::record_id(&native, "variant")?;
    verify_debit_on(
        tx,
        DebitBinding {
            hub,
            operation: &debit_id,
            revision: debit_revision,
            group: parent,
            trace,
            variant,
            target: &target_id,
            target_group: *target_group,
            born_at: op.recorded_at,
        },
        &data,
    )
    .await?;
    let location =
        reference::record_at_catalog_on(tx, hub, input.location, op.catalog_revision).await?;
    if location.value.kind() != ReferenceKind::Location
        || location.revision.value() != data.location_revision
    {
        return Err(corrupt_storage());
    }
    let recovery = PhysicalRecoveryOrigin {
        debit_operation: input.debit.operation,
        debit_revision,
    };
    let group = PhysicalStockGroup {
        id: child,
        trace,
        variant,
        parent: Some(parent),
        identification: Some(input.identification.clone()),
        location: input.location,
        packs: input.packs,
        revision: HubRevision::INITIAL,
        causal_floor: Some(op.recorded_at),
        work: Vec::new(),
    };
    group.validate().map_err(|_| corrupt_storage())?;
    let result = FoundBirthProjection {
        group,
        location,
        birth: ActualPhysicalBirth {
            kind: ActualPhysicalBirthKind::FoundRemainder { allocation },
            operation,
            hub,
            trace,
            variant,
            group: child,
            parent: Some(parent),
            stock_revision: op.revision,
            packs: input.packs,
            actor: op.actor,
            occurred: Occurrence::Known(op.recorded_at),
            recorded_at: op.recorded_at,
        },
        recovery,
        basis: basis.clone(),
        cohort_evidence: input.cohort_evidence.clone(),
        observation_operation: observed.operation,
        target_operation: *target_operation,
        target_group: *target_group,
        allocation,
    };
    let saved: Vec<u8> = row
        .try_get("projection_hash")
        .map_err(|_| corrupt_storage())?;
    if seal(op, hub, &data, &result)?.as_slice() != saved.as_slice() {
        return Err(corrupt_storage());
    }
    Ok(result)
}

/// Current and historical cuts use immutable birth metadata, never current
/// quantities, a later correction head or a stored request as execution intent.
pub(in crate::hub) async fn recovery_at_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<Option<PhysicalRecoveryOrigin>, SafeError> {
    let created:String=sqlx::query_scalar("SELECT substr(created_operation_id,1,37) FROM hub_group_identification WHERE group_id=? LIMIT 1")
        .bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let id = DurableRequestId::parse(created).map_err(|_| corrupt_storage())?;
    let op = stock::read::operation_on(tx, hub, &id)
        .await?
        .ok_or_else(corrupt_storage)?;
    if op.revision.value() > cut.value() {
        return Err(corrupt_storage());
    }
    if op.kind != "correction_found" {
        return Ok(None);
    }
    Ok(Some(birth_on(tx, hub, &op, group).await?.recovery))
}

struct DebitBinding<'a> {
    hub: HubId,
    operation: &'a DurableRequestId,
    revision: HubRevision,
    group: HubRecordId,
    trace: HubRecordId,
    variant: HubRecordId,
    target: &'a DurableRequestId,
    target_group: HubRecordId,
    born_at: UnixTimestamp,
}
async fn verify_debit_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    binding: DebitBinding<'_>,
    data: &BirthData,
) -> Result<(), SafeError> {
    let DebitBinding {
        hub,
        operation: debit,
        revision,
        group,
        trace,
        variant,
        target,
        target_group,
        born_at,
    } = binding;
    let op = stock::read::operation_on(tx, hub, debit)
        .await?
        .ok_or_else(corrupt_storage)?;
    if op.revision != revision
        || op.kind != data.debit_kind
        || op.actor.to_hex() != data.debit_actor
        || op.hash != data.debit_input_hash
        || op.catalog_revision.value() != data.debit_catalog_revision
        || op.result.to_hex() != data.debit_result
        || op.recorded_at.as_seconds() != data.debit_recorded_at
        || op.recorded_at > born_at
    {
        return Err(corrupt_storage());
    }
    let row=match op.kind.as_str() {
        "departure" if debit==target && group==target_group=>sqlx::query("SELECT line.packs,line.packs AS before_packs,line.before_group_revision,line.after_group_revision,substr(original.result_hash,1,33) AS result_hash,substr(original.departure_id,1,17) AS native_result,substr(population.variant_id,1,17) AS native_variant FROM hub_departure_lines line JOIN hub_departures original ON original.operation_id=line.operation_id JOIN hub_receipts population ON population.trace_id=line.trace_id WHERE line.operation_id=? AND line.group_id=? AND line.trace_id=? LIMIT 1").bind(debit.as_str()).bind(group.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).fetch_optional(&mut *tx).await,
        "actual_review" if debit==target && group==target_group=>sqlx::query("SELECT packs,before_packs,before_group_revision,after_group_revision,substr(result_hash,1,33) AS result_hash,substr(report_id,1,17) AS native_result,substr(variant_id,1,17) AS native_variant FROM hub_actual_reviews WHERE operation_id=? AND group_id=? AND trace_id=? AND review_kind='departure' LIMIT 1").bind(debit.as_str()).bind(group.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).fetch_optional(&mut *tx).await,
        "correction_additional"=>sqlx::query("SELECT debit.packs,debit.before_packs,debit.before_group_revision,debit.after_group_revision,substr(debit.projection_hash,1,33) AS result_hash,substr(correction.correction_id,1,17) AS native_result,substr(debit.variant_id,1,17) AS native_variant FROM hub_correction_physical_debits debit JOIN hub_corrections correction ON correction.operation_id=debit.operation_id WHERE debit.operation_id=? AND debit.group_id=? AND debit.trace_id=? AND debit.target_operation_id=? AND debit.target_group_id=? AND correction.family='additional' AND correction.target_operation_id=debit.target_operation_id AND correction.subject_id=debit.target_group_id LIMIT 1").bind(debit.as_str()).bind(group.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).bind(target.as_str()).bind(target_group.as_bytes().as_slice()).fetch_optional(&mut *tx).await,
        _=>return Err(corrupt_storage()),
    }.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let packs = stock::read::whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?;
    let before =
        stock::read::whole_packs(row.try_get("before_packs").map_err(|_| corrupt_storage())?)?;
    let hash: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if stock::read::record_id(&row, "native_result")? != op.result
        || stock::read::record_id(&row, "native_variant")? != variant
        || stock::read::revision_field(&row, "before_group_revision")?.value()
            != data.debit_before_group_revision
        || stock::read::revision_field(&row, "after_group_revision")?.value()
            != data.debit_after_group_revision
    {
        return Err(corrupt_storage());
    }
    if packs.value() == 0
        || packs.value() != data.debit_packs
        || before.value() != data.debit_before_packs
        || before.value() < packs.value()
        || hash.as_slice() != data.debit_result_hash.as_slice()
    {
        return Err(corrupt_storage());
    }
    Ok(())
}
