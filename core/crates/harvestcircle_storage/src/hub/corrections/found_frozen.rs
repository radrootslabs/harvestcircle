//! Immutable native pre-effect facts. This codec has no saved-request decoder,
//! admission constructor or effect execution path.
use super::found_context::Context;
use super::*;
use harvestcircle_domain::hub::{
    corrections::CorrectionObservedFacts,
    movement::{HUB_GROUP_CAPACITY, PhysicalIdentification},
    portions::{HUB_LINEAGE_GROUPS, PhysicalLineage, PhysicalLineageNode, PhysicalRecoveryOrigin},
    reference::{HubLabel, SourceProvenance, WholePacks},
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
    observation_operation: String,
    stock_revision: u64,
    catalog_revision: u64,
    latest_recorded_at: i64,
    group_count: usize,
    nodes: Vec<Node>,
    identities: Vec<Identity>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    parent: Option<String>,
    created_revision: u64,
    packs: u32,
    recovery: Option<Recovery>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    operation: String,
    revision: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    label: String,
    observation: String,
}

pub(super) fn encode(context: &Context) -> Result<Vec<u8>, SafeError> {
    if context.uncertain_submissions {
        return Err(stock::conflict());
    }
    let data = Data {
        schema: 1,
        observation_operation: context.observation.operation.as_str().into(),
        stock_revision: context.stock_revision.value(),
        catalog_revision: context.catalog.revision().value(),
        latest_recorded_at: context.latest_recorded_at.as_seconds(),
        group_count: context.group_count,
        nodes: context
            .lineage
            .nodes()
            .iter()
            .map(|node| Node {
                id: node.id.to_hex(),
                parent: node.parent.map(HubRecordId::to_hex),
                created_revision: node.created_revision.value(),
                packs: node.packs.value(),
                recovery: node.recovery.map(|r| Recovery {
                    operation: r.debit_operation.to_hex(),
                    revision: r.debit_revision.value(),
                }),
            })
            .collect(),
        identities: context
            .active_identifications
            .iter()
            .map(|identity| Identity {
                label: identity.label.as_str().into(),
                observation: identity.observation.as_str().into(),
            })
            .collect(),
    };
    encode_data(&data)
}

fn encode_data(data: &Data) -> Result<Vec<u8>, SafeError> {
    if data.schema != 1
        || data.nodes.is_empty()
        || data.nodes.len() > HUB_LINEAGE_GROUPS
        || data.group_count > HUB_GROUP_CAPACITY
        || data.identities.len() > data.group_count
        || data.nodes.len() > data.group_count
    {
        return Err(corrupt_storage());
    }
    let bytes = serde_json::to_vec(data).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(stock::capacity());
    }
    Ok(bytes)
}

fn scope(hub: HubId, op: &OperationRecord, bytes: &[u8]) -> Result<[u8; 32], SafeError> {
    if op.kind != "correction_found" || bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    #[derive(Serialize)]
    struct Header<'a> {
        hub: HubId,
        operation: &'a str,
        kind: &'a str,
        actor: PublicKey,
        input: [u8; 32],
        catalog: HubRevision,
        revision: HubRevision,
        result: HubRecordId,
        recorded_at: UnixTimestamp,
    }
    let header = serde_json::to_vec(&Header {
        hub,
        operation: op.id.as_str(),
        kind: &op.kind,
        actor: op.actor,
        input: op.hash,
        catalog: op.catalog_revision,
        revision: op.revision,
        result: op.result,
        recorded_at: op.recorded_at,
    })
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.found.frozen.v1\0");
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
    let bytes = encode(context)?;
    if context.stock_revision.value().checked_add(1) != Some(op.revision.value())
        || context.catalog.revision() != op.catalog_revision
        || context.latest_recorded_at > op.recorded_at
    {
        return Err(corrupt_storage());
    }
    let hash = scope(context.hub, op, &bytes)?;
    sqlx::query("INSERT INTO hub_correction_captures (operation_id,kind,payload_schema,payload,scope_hash) VALUES (?,'correction_found',1,?,?)")
        .bind(op.id.as_str()).bind(bytes).bind(hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(hash)
}

pub(super) async fn restore_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: &OperationRecord,
    observation: HubCorrectionObservationReceipt,
) -> Result<(Context, [u8; 32], [u8; 32]), SafeError> {
    let row=sqlx::query("SELECT kind,payload_schema,substr(payload,1,16777217) AS payload,substr(scope_hash,1,33) AS scope_hash FROM hub_correction_captures WHERE operation_id=? LIMIT 1")
        .bind(op.id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<String, _>("kind")
        .map_err(|_| corrupt_storage())?
        != "correction_found"
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let stored: Vec<u8> = row.try_get("scope_hash").map_err(|_| corrupt_storage())?;
    let hash = scope(hub, op, &bytes)?;
    if stored.as_slice() != hash.as_slice() {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(&bytes).map_err(|_| corrupt_storage())?;
    if encode_data(&data).map_err(|_| corrupt_storage())? != bytes
        || data.stock_revision.checked_add(1) != Some(op.revision.value())
        || data.catalog_revision != op.catalog_revision.value()
        || data.observation_operation != observation.operation.as_str()
    {
        return Err(corrupt_storage());
    }
    let revision = HubRevision::from_value(data.stock_revision).map_err(|_| corrupt_storage())?;
    let latest:i64=sqlx::query_scalar("SELECT COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(revision.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if latest != data.latest_recorded_at
        || latest > op.recorded_at.as_seconds()
        || observation.original.stock_revision.value() > revision.value()
    {
        return Err(corrupt_storage());
    }
    let latest_recorded_at = UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?;
    let catalog = reference::load_catalog_through_on(tx, hub, op.catalog_revision).await?;
    let CorrectionObservedFacts::Found {
        target_operation,
        target_group,
        ..
    } = &observation.original.observation.facts
    else {
        return Err(corrupt_storage());
    };
    let target =
        crate::hub::actuals::common::operation(*target_operation).map_err(|_| corrupt_storage())?;
    let funding = Box::pin(super::target_funding::load_with_proofs_on(
        tx,
        hub,
        &target,
        *target_group,
        revision,
    ))
    .await?;
    let id = |s: &str| HubRecordId::from_hex(s).map_err(|_| corrupt_storage());
    let nodes = data
        .nodes
        .iter()
        .map(|node| {
            Ok(PhysicalLineageNode {
                id: id(&node.id)?,
                parent: node.parent.as_deref().map(id).transpose()?,
                trace: funding.quantity.original().trace,
                variant: funding.quantity.original().variant,
                created_revision: HubRevision::from_value(node.created_revision)
                    .map_err(|_| corrupt_storage())?,
                packs: WholePacks::from_value(node.packs).map_err(|_| corrupt_storage())?,
                recovery: node
                    .recovery
                    .as_ref()
                    .map(|r| {
                        Ok::<_, SafeError>(PhysicalRecoveryOrigin {
                            debit_operation: id(&r.operation)?,
                            debit_revision: HubRevision::from_value(r.revision)
                                .map_err(|_| corrupt_storage())?,
                        })
                    })
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, SafeError>>()?;
    let lineage = PhysicalLineage::new(
        hub,
        funding.quantity.original().trace,
        funding.quantity.original().variant,
        revision,
        nodes,
    )
    .map_err(|_| corrupt_storage())?;
    handling::lineage::historical_births_on(tx, hub, &lineage).await?;
    let treatment = handling::treatment::read::historical_state_on(tx, hub, &lineage).await?;
    let groups:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups g JOIN hub_group_identification birth ON birth.group_id=g.group_id JOIN hub_stock_operations original ON original.operation_id=birth.created_operation_id WHERE original.hub_id=? AND original.stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(revision.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if usize::try_from(groups).map_err(|_| corrupt_storage())? != data.group_count {
        return Err(corrupt_storage());
    }
    let mut active_identifications = Vec::with_capacity(data.identities.len());
    for identity in data.identities {
        let native:i64=sqlx::query_scalar("SELECT count(*) FROM hub_group_identification birth JOIN hub_stock_operations original ON original.operation_id=birth.created_operation_id WHERE original.hub_id=? AND original.stock_revision<=? AND birth.physical_label=? AND birth.observation=?")
            .bind(hub.as_bytes().as_slice()).bind(i64::try_from(revision.value()).map_err(|_|corrupt_storage())?).bind(&identity.label).bind(&identity.observation).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
        if native == 0 {
            return Err(corrupt_storage());
        }
        active_identifications.push(PhysicalIdentification {
            label: HubLabel::new(identity.label).map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(identity.observation)
                .map_err(|_| corrupt_storage())?,
        });
    }
    let context = Context {
        hub,
        stock_revision: revision,
        catalog,
        observation,
        funding,
        lineage,
        treatment,
        active_identifications,
        group_count: data.group_count,
        latest_recorded_at,
        uncertain_submissions: false,
    };
    // Per-entry historical existence alone cannot prove the old active set.
    // The complete set must match the immutable material captured by the native
    // admission query, including identities outside this trace. A resealed
    // sidecar with an omitted/reordered/replaced label cannot substitute it.
    let command = harvestcircle_domain::hub::corrections::FoundCorrectionCommand {
        observation_operation: context.observation.original.operation,
        expected_stock_revision: revision,
        expected_catalog_revision: op.catalog_revision,
    };
    let material = [
        harvestcircle_application::hub_submission::HubSubmittedMaterial::FoundCorrection(Box::new(
            context.material(
                crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?,
            ),
        )),
    ];
    let admission = crate::hub::submission::verify_recorded_capture_on(
        tx,
        hub,
        op,
        harvestcircle_application::hub_submission::HubSubmittedRequest::FoundCorrection(&command),
        revision,
        op.catalog_revision,
        &material,
    )
    .await?;
    Ok((context, hash, admission))
}
