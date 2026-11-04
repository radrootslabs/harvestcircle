//! Primitive pre-effect snapshots restored against canonical historical records.
//! This codec has no request decoder, FreshAdmission constructor or effect path.
use super::additional_review::Context;
use super::*;
use harvestcircle_application::hub_submission::{HubSubmittedMaterial, HubSubmittedRequest};
use harvestcircle_domain::hub::{
    corrections::AdditionalQuantityCommand,
    movement::{PhysicalIdentification, PhysicalStockGroup, WorkAllocation, WorkProgress},
    portions::{HUB_LINEAGE_GROUPS, PhysicalLineage, PhysicalLineageNode},
    reference::{HubLabel, ReferenceKind, SourceProvenance, WholePacks},
    stock::QuantityWork,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 16 * 1024 * 1024;
#[cfg(test)]
mod tests;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    stock_revision: u64,
    catalog_revision: u64,
    latest_recorded_at: i64,
    group: Group,
    nodes: Vec<Node>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    id: String,
    location: String,
    revision: u64,
    packs: u32,
    causal_floor: Option<i64>,
    work: Vec<Work>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Work {
    id: String,
    revision: u64,
    completed: u32,
    retired: u32,
    remaining: u32,
    progress: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    parent: Option<String>,
    created_revision: u64,
    packs: u32,
}

pub(super) const fn progress(value: WorkProgress) -> &'static str {
    match value {
        WorkProgress::Pending => "pending",
        WorkProgress::Completed => "completed",
        WorkProgress::NoLongerRequired => "no_longer_required",
    }
}

fn bytes(data: &Data) -> Result<Vec<u8>, SafeError> {
    if data.schema != 1
        || data.nodes.is_empty()
        || data.nodes.len() > HUB_LINEAGE_GROUPS
        || data.group.work.len() > harvestcircle_domain::hub::movement::HUB_GROUP_REQUIREMENTS
    {
        return Err(corrupt_storage());
    }
    let bytes = serde_json::to_vec(data).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}

fn encode(context: &Context) -> Result<Vec<u8>, SafeError> {
    if context.uncertain_submissions || context.prior_report_review.is_some() {
        return Err(stock::conflict());
    }
    bytes(&Data {
        schema: 1,
        stock_revision: context.revision.value(),
        catalog_revision: context.catalog.revision().value(),
        latest_recorded_at: context.latest_recorded_at.as_seconds(),
        group: Group {
            id: context.group.id.to_hex(),
            location: context.group.location.to_hex(),
            revision: context.group.revision.value(),
            packs: context.group.packs.value(),
            causal_floor: context.group.causal_floor.map(UnixTimestamp::as_seconds),
            work: context
                .group
                .work
                .iter()
                .map(|a| Work {
                    id: a.requirement.id.to_hex(),
                    revision: a.requirement.revision.value(),
                    completed: a.requirement.quantity.completed().value(),
                    retired: a.requirement.quantity.no_longer_required().value(),
                    remaining: a.requirement.quantity.remaining().value(),
                    progress: progress(a.progress).into(),
                })
                .collect(),
        },
        nodes: context
            .lineage
            .nodes()
            .iter()
            .map(|n| Node {
                id: n.id.to_hex(),
                parent: n.parent.map(HubRecordId::to_hex),
                created_revision: n.created_revision.value(),
                packs: n.packs.value(),
            })
            .collect(),
    })
}

fn scope(hub: HubId, op: &OperationRecord, bytes: &[u8]) -> Result<[u8; 32], SafeError> {
    if op.kind != "correction_additional" || bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let header = serde_json::to_vec(&(
        hub,
        op.id.as_str(),
        op.actor,
        op.hash,
        op.catalog_revision,
        op.revision,
        op.result,
        op.recorded_at,
    ))
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.additional.frozen.v1\0");
    hash.update(
        u64::try_from(header.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(header);
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(super) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    context: &Context,
    op: &OperationRecord,
) -> Result<[u8; 32], SafeError> {
    if context.revision.value().checked_add(1) != Some(op.revision.value())
        || context.catalog.revision() != op.catalog_revision
        || context.latest_recorded_at > op.recorded_at
    {
        return Err(corrupt_storage());
    }
    let bytes = encode(context)?;
    let hash = scope(context.hub, op, &bytes)?;
    sqlx::query("INSERT INTO hub_correction_captures (operation_id,kind,payload_schema,payload,scope_hash) VALUES (?,'correction_additional',1,?,?)")
        .bind(op.id.as_str()).bind(bytes).bind(hash.as_slice()).execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    Ok(hash)
}

pub(super) async fn restore_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: &OperationRecord,
    command: &AdditionalQuantityCommand,
) -> Result<(Context, [u8; 32], [u8; 32]), SafeError> {
    let (data, hash) = read_data_on(tx, hub, op, command).await?;
    restore_data_on(tx, hub, op, command, data, hash).await
}

async fn read_data_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    command: &AdditionalQuantityCommand,
) -> Result<(Data, [u8; 32]), SafeError> {
    let row = sqlx::query("SELECT kind,payload_schema,substr(payload,1,16777217) AS payload,substr(scope_hash,1,33) AS scope_hash FROM hub_correction_captures WHERE operation_id=?")
        .bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<String, _>("kind")
        .map_err(|_| corrupt_storage())?
        != "correction_additional"
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let raw: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let stored: Vec<u8> = row.try_get("scope_hash").map_err(|_| corrupt_storage())?;
    let hash = scope(hub, op, &raw)?;
    if stored.as_slice() != hash.as_slice() {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(&raw).map_err(|_| corrupt_storage())?;
    if bytes(&data)? != raw
        || data.stock_revision.checked_add(1) != Some(op.revision.value())
        || data.stock_revision != command.facts.expected_stock_revision.value()
        || data.catalog_revision != op.catalog_revision.value()
        || data.catalog_revision != command.facts.expected_catalog_revision.value()
        || super::additional_payload::id(&data.group.id)? != command.balancing_group
        || data.group.revision != command.expected_balancing_revision.value()
        || data.group.revision == 0
        || data.group.revision > data.stock_revision
        || data.latest_recorded_at < 0
        || data.latest_recorded_at > op.recorded_at.as_seconds()
    {
        return Err(corrupt_storage());
    }
    Ok((data, hash))
}

async fn restore_data_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: &OperationRecord,
    command: &AdditionalQuantityCommand,
    data: Data,
    hash: [u8; 32],
) -> Result<(Context, [u8; 32], [u8; 32]), SafeError> {
    let canonical_bytes = bytes(&data)?;
    let cut = HubRevision::from_value(data.stock_revision).map_err(|_| corrupt_storage())?;
    let latest: i64 = sqlx::query_scalar("SELECT COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
        .fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if latest != data.latest_recorded_at || latest > op.recorded_at.as_seconds() {
        return Err(corrupt_storage());
    }
    let target = crate::hub::actuals::common::operation(command.facts.target_operation)
        .map_err(|_| corrupt_storage())?;
    let original = Box::pin(super::quantity::original_on(
        tx,
        hub,
        &target,
        command.facts.target_group,
        cut,
    ))
    .await?;
    let trace = original.original.trace;
    let variant = original.original.variant;
    let catalog = reference::load_catalog_through_on(tx, hub, op.catalog_revision).await?;
    let mut nodes = Vec::with_capacity(data.nodes.len());
    for node in data.nodes {
        let id = super::additional_payload::id(&node.id)?;
        nodes.push(PhysicalLineageNode {
            id,
            parent: node
                .parent
                .as_deref()
                .map(super::additional_payload::id)
                .transpose()?,
            trace,
            variant,
            created_revision: HubRevision::from_value(node.created_revision)
                .map_err(|_| corrupt_storage())?,
            packs: WholePacks::from_value(node.packs).map_err(|_| corrupt_storage())?,
            recovery: super::birth::recovery_at_on(tx, hub, id, cut).await?,
        });
    }
    let lineage =
        PhysicalLineage::new(hub, trace, variant, cut, nodes).map_err(|_| corrupt_storage())?;
    let births = Box::pin(handling::lineage::historical_births_on(tx, hub, &lineage)).await?;
    let selected = lineage
        .node(command.balancing_group)
        .ok_or_else(corrupt_storage)?;
    let group_cut = GroupCut {
        trace,
        variant,
        revision: cut,
        parent: selected.parent,
        packs: selected.packs,
        created_revision: selected.created_revision,
    };
    let group = restore_group_on(tx, hub, &group_cut, data.group, &catalog).await?;
    let report = crate::hub::actuals::reports::by_id_on(tx, hub, command.facts.report)
        .await?
        .original;
    let closed: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE report_id=? AND operation_id=? AND review_operation_id=?")
        .bind(report.id.as_bytes().as_slice()).bind(crate::hub::actuals::common::operation(report.operation).map_err(|_| corrupt_storage())?.as_str())
        .bind(op.id.as_str()).fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if closed != 1 || report.stock_revision.value() > cut.value() {
        return Err(corrupt_storage());
    }
    let population = handling::read::trace_on(tx, hub, trace).await?;
    let source = handling::actuals::source_on(
        tx,
        hub,
        &lineage,
        &population,
        &crate::hub::actuals::common::operation(command.source_operation)
            .map_err(|_| corrupt_storage())?,
        command.source_group,
    )
    .await?;
    let activity = Box::pin(handling::actuals::activity_on(tx, hub, &lineage, &source)).await?;
    let known = Box::pin(super::known::load_with_proofs_on(tx, hub, trace, cut)).await?;
    let target_event = known
        .iter()
        .position(|k| k.original.operation == command.facts.target_operation)
        .ok_or_else(corrupt_storage)?;
    let target_line = known[target_event]
        .quantities
        .iter()
        .position(|q| q.original().group == command.facts.target_group)
        .ok_or_else(corrupt_storage)?;
    let treatment = handling::treatment::read::historical_state_on(tx, hub, &lineage).await?;
    let context = Context {
        hub,
        revision: cut,
        catalog,
        report,
        prior_report_review: None,
        known,
        target_event,
        target_line,
        group,
        lineage,
        births,
        treatment,
        source,
        activity,
        latest_recorded_at: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        uncertain_submissions: false,
    };
    if encode(&context)? != canonical_bytes {
        return Err(corrupt_storage());
    }
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let material = [HubSubmittedMaterial::AdditionalQuantity(Box::new(
        context.material(own),
    ))];
    let admission = crate::hub::submission::verify_recorded_capture_on(
        tx,
        hub,
        op,
        HubSubmittedRequest::AdditionalQuantity(command),
        cut,
        op.catalog_revision,
        &material,
    )
    .await?;
    Ok((context, hash, admission))
}

/// A narrow immutable position/source projection. It intentionally performs no
/// full correction replan or current-state conservation. Authoritative callers
/// still require the complete original/capture audit.
pub(super) struct PhysicalBefore {
    pub group: PhysicalStockGroup,
    pub source: harvestcircle_domain::hub::actuals::HistoricalActualSource,
    pub frozen: [u8; 32],
    pub admission: [u8; 32],
}

pub(super) async fn physical_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    command: &AdditionalQuantityCommand,
    canonical: &super::quantity::CanonicalLine,
) -> Result<PhysicalBefore, SafeError> {
    let (data, frozen) = read_data_on(tx, hub, op, command).await?;
    let cut = HubRevision::from_value(data.stock_revision).map_err(|_| corrupt_storage())?;
    if canonical.original.operation != command.facts.target_operation
        || canonical.original.group != command.facts.target_group
        || canonical.original.stock_revision.value() > cut.value()
    {
        return Err(corrupt_storage());
    }
    // A shallow debit reader needs the exact selected position, not recursive
    // validation of every older recovered sibling. Full restore above retains
    // every node/birth and the mandatory complete immutable material comparison.
    let mut selected = None;
    for node in &data.nodes {
        if super::additional_payload::id(&node.id)? == command.balancing_group {
            if selected.is_some() {
                return Err(corrupt_storage());
            }
            selected = Some(node);
        }
    }
    let selected = selected.ok_or_else(corrupt_storage)?;
    let group_cut = GroupCut {
        trace: canonical.original.trace,
        variant: canonical.original.variant,
        revision: cut,
        parent: selected
            .parent
            .as_deref()
            .map(super::additional_payload::id)
            .transpose()?,
        packs: WholePacks::from_value(selected.packs).map_err(|_| corrupt_storage())?,
        created_revision: HubRevision::from_value(selected.created_revision)
            .map_err(|_| corrupt_storage())?,
    };
    let catalog = reference::load_catalog_through_on(tx, hub, op.catalog_revision).await?;
    let group = restore_group_on(tx, hub, &group_cut, data.group, &catalog).await?;
    let population = handling::read::trace_on(tx, hub, group_cut.trace).await?;
    let source = Box::pin(handling::actuals::source_original_on(
        tx,
        hub,
        &population,
        &crate::hub::actuals::common::operation(command.source_operation)
            .map_err(|_| corrupt_storage())?,
        command.source_group,
    ))
    .await?;
    if source.stock_revision.value() > cut.value()
        || source.recorded_at.as_seconds() > data.latest_recorded_at
        || source.catalog_revision.value() > op.catalog_revision.value()
    {
        return Err(corrupt_storage());
    }
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub,
        op,
        HubSubmittedRequest::AdditionalQuantity(command),
    )
    .await?;
    Ok(PhysicalBefore {
        group,
        source,
        frozen,
        admission,
    })
}

struct GroupCut {
    trace: HubRecordId,
    variant: HubRecordId,
    revision: HubRevision,
    parent: Option<HubRecordId>,
    packs: WholePacks,
    created_revision: HubRevision,
}

async fn restore_group_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: &GroupCut,
    data: Group,
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
) -> Result<PhysicalStockGroup, SafeError> {
    let id = super::additional_payload::id(&data.id)?;
    let row = sqlx::query(concat!(
        "SELECT substr(i.parent_group_id,1,17) AS parent,substr(CAST(i.physical_label AS BLOB),1,121) AS label,",
        "substr(CAST(i.observation AS BLOB),1,513) AS observation,substr(g.trace_id,1,17) AS trace_id,o.stock_revision AS created_revision ",
        "FROM hub_group_identification i JOIN hub_stock_groups g ON g.group_id=i.group_id JOIN hub_stock_operations o ON o.operation_id=i.created_operation_id WHERE i.group_id=? AND o.hub_id=?"
    ))
        .bind(id.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if stock::read::optional_id(&row, "parent")? != cut.parent
        || stock::read::record_id(&row, "trace_id")? != cut.trace
        || stock::read::revision_field(&row, "created_revision")? != cut.created_revision
        || cut.created_revision.value() > cut.revision.value()
    {
        return Err(corrupt_storage());
    }
    let label: Option<Vec<u8>> = row.try_get("label").map_err(|_| corrupt_storage())?;
    let observation: Option<Vec<u8>> = row.try_get("observation").map_err(|_| corrupt_storage())?;
    let identification = match (label, observation) {
        (None, None) => None,
        (Some(label), Some(observation)) => Some(PhysicalIdentification {
            label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(
                String::from_utf8(observation).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
        }),
        _ => return Err(corrupt_storage()),
    };
    let location = super::additional_payload::id(&data.location)?;
    let record = reference::record_at_catalog_on(tx, hub, location, catalog.revision()).await?;
    if record.value.kind() != ReferenceKind::Location {
        return Err(corrupt_storage());
    }
    let mut work = Vec::with_capacity(data.work.len());
    let mut ids = std::collections::BTreeSet::new();
    for snapshot in data.work {
        let work_id = super::additional_payload::id(&snapshot.id)?;
        if !ids.insert(work_id) {
            return Err(corrupt_storage());
        }
        let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_work_requirements w JOIN hub_stock_operations o ON o.operation_id=w.created_operation_id WHERE w.work_id=? AND o.hub_id=? AND o.stock_revision<=?")
            .bind(work_id.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.revision.value()).map_err(|_| corrupt_storage())?)
            .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        if exists != 1 {
            return Err(corrupt_storage());
        }
        let definition = handling::read::definition_on(tx, hub, work_id).await?;
        if definition.trace != cut.trace || definition.variant != cut.variant {
            return Err(corrupt_storage());
        }
        let packs = |n| WholePacks::from_value(n).map_err(|_| corrupt_storage());
        let quantity = QuantityWork::restore(
            definition.required,
            packs(snapshot.completed)?,
            packs(snapshot.retired)?,
            packs(snapshot.remaining)?,
        )
        .map_err(|_| corrupt_storage())?;
        let requirement = definition.at(
            quantity,
            HubRevision::from_value(snapshot.revision).map_err(|_| corrupt_storage())?,
        )?;
        let progress = match snapshot.progress.as_str() {
            "pending" => WorkProgress::Pending,
            "completed" => WorkProgress::Completed,
            "no_longer_required" => WorkProgress::NoLongerRequired,
            _ => return Err(corrupt_storage()),
        };
        work.push(WorkAllocation {
            requirement,
            progress,
        });
    }
    let causal_floor = data
        .causal_floor
        .map(|n| UnixTimestamp::from_seconds(n).ok_or_else(corrupt_storage))
        .transpose()?;
    if handling::read::causal_floor_through_on(tx, id, cut.revision).await? != causal_floor {
        return Err(corrupt_storage());
    }
    let group = PhysicalStockGroup {
        id,
        trace: cut.trace,
        variant: cut.variant,
        parent: cut.parent,
        identification,
        location,
        revision: HubRevision::from_value(data.revision).map_err(|_| corrupt_storage())?,
        packs: WholePacks::from_value(data.packs).map_err(|_| corrupt_storage())?,
        causal_floor,
        work,
    };
    if group.packs != cut.packs {
        return Err(corrupt_storage());
    }
    group.validate().map_err(|_| corrupt_storage())?;
    Ok(group)
}
