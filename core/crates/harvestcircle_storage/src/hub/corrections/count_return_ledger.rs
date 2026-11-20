//! Native association roots are distinct from immutable Return originals and
//! physical correction rows. Descriptions and seals alone grant no allowance.
use super::*;
use harvestcircle_domain::hub::corrections::{
    CountReturnAssociation, CountReturnAssociationCommand, count_return_association_id,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::hub) struct Epoch {
    pub stock: u64,
    pub catalog: u64,
}

pub(in crate::hub) async fn epoch_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Epoch, SafeError> {
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 22 {
        return Err(corrupt_storage());
    }
    let parent = super::count_mode::epoch_on(tx, hub).await?;
    let row = sqlx::query(concat!(
        "SELECT epoch,legacy_stock_revision,legacy_catalog_revision,",
        "(SELECT count(*) FROM hub_count_return_epochs) AS total,",
        "(SELECT revision FROM hub_stock_state WHERE hub_id=e.hub_id) AS current_stock,",
        "COALESCE((SELECT max(catalog_revision) FROM hub_reference_versions WHERE hub_id=e.hub_id),1) AS current_catalog,",
        "COALESCE((SELECT max(catalog_revision) FROM hub_stock_operations WHERE hub_id=e.hub_id AND stock_revision<=e.legacy_stock_revision),1) AS dependency,",
        "(SELECT count(*) FROM hub_stock_operations WHERE kind='count_return_associate' AND stock_revision<=e.legacy_stock_revision) AS invalid ",
        "FROM hub_count_return_epochs e WHERE hub_id=? LIMIT 1"
    )).bind(hub.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let epoch = Epoch {
        stock: number("legacy_stock_revision")?,
        catalog: number("legacy_catalog_revision")?,
    };
    if number("epoch")? != 1
        || number("total")? != 1
        || number("invalid")? != 0
        || epoch.stock < parent.stock
        || epoch.catalog < parent.catalog
        || epoch.stock > number("current_stock")?
        || epoch.catalog > number("current_catalog")?
        || number("dependency")? > epoch.catalog
    {
        return Err(corrupt_storage());
    }
    Ok(epoch)
}

pub(in crate::hub) struct Root {
    pub op: OperationRecord,
    pub command: CountReturnAssociationCommand,
    pub count: DurableRequestId,
    pub packs: u32,
    pub epoch: Epoch,
    pub capture: [u8; 32],
    pub result: [u8; 32],
    pub projection: Vec<u8>,
    pub projection_hash: [u8; 32],
}

pub(super) fn projection_bytes(record: &CountReturnAssociation) -> Result<Vec<u8>, SafeError> {
    record.validate().map_err(|_| corrupt_storage())?;
    let bytes = serde_json::to_vec(record).map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > 16 * 1024 {
        return Err(stock::capacity());
    }
    Ok(bytes)
}

pub(super) fn projection_seal(hub: HubId, root: &Root) -> Result<[u8; 32], SafeError> {
    if root.projection.is_empty() || root.projection.len() > 16 * 1024 {
        return Err(corrupt_storage());
    }
    let bytes = serde_json::to_vec(&(
        (
            hub,
            root.op.id.as_str(),
            &root.op.kind,
            root.op.actor,
            root.op.hash,
            root.op.revision,
            root.op.catalog_revision,
            root.op.result,
            root.op.recorded_at,
        ),
        (
            root.epoch.stock,
            root.epoch.catalog,
            root.count.as_str(),
            root.packs,
        ),
        (&root.command, root.result, root.capture, &root.projection),
    ))
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.count-return.association.projection.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(in crate::hub) async fn read_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<Root, SafeError> {
    if op.kind != "count_return_associate" {
        return Err(corrupt_storage());
    }
    let epoch = epoch_on(tx, hub).await?;
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    if op.revision.value() <= epoch.stock
        || op.catalog_revision.value() < epoch.catalog
        || op.result != count_return_association_id(own).map_err(|_| corrupt_storage())?
    {
        return Err(corrupt_storage());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(association_id,1,17) AS association_id,substr(hub_id,1,17) AS hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,",
        "substr(return_operation_id,1,37) AS returned,substr(explanation_operation_id,1,37) AS explained,substr(count_operation_id,1,37) AS counted,",
        "substr(account_previous_operation_id,1,37) AS account_previous,substr(previous_operation_id,1,37) AS previous,packs,",
        "payload_schema,substr(payload,1,65537) AS payload,projection_schema,substr(projection,1,16385) AS projection,",
        "substr(projection_hash,1,33) AS projection_hash,substr(result_hash,1,33) AS result_hash,substr(capture_hash,1,33) AS capture_hash ",
        "FROM hub_count_return_associations WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let bytes = |name| -> Result<[u8; 32], SafeError> {
        row.try_get::<Vec<u8>, _>(name)
            .map_err(|_| corrupt_storage())?
            .try_into()
            .map_err(|_| corrupt_storage())
    };
    let command = super::count_return_payload::decode(
        &row.try_get::<Vec<u8>, _>("payload")
            .map_err(|_| corrupt_storage())?,
    )?;
    let returned = crate::hub::actuals::common::operation(command.return_operation)
        .map_err(|_| corrupt_storage())?;
    let explained = crate::hub::actuals::common::operation(command.explanation_operation)
        .map_err(|_| corrupt_storage())?;
    let account_previous = command
        .expected_account_prior
        .map(crate::hub::actuals::common::operation)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    let previous = command
        .expected_returns_prior
        .map(crate::hub::actuals::common::operation)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    let count = DurableRequestId::parse(
        row.try_get::<String, _>("counted")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if number("epoch")? != 1
        || number("legacy_stock_revision")? != epoch.stock
        || number("legacy_catalog_revision")? != epoch.catalog
        || number("stock_revision")? != op.revision.value()
        || number("catalog_revision")? != op.catalog_revision.value()
        || number("payload_schema")? != 1
        || number("projection_schema")? != 1
        || stock::read::record_id(&row, "association_id")? != op.result
        || row
            .try_get::<Vec<u8>, _>("hub_id")
            .map_err(|_| corrupt_storage())?
            .as_slice()
            != hub.as_bytes()
        || row
            .try_get::<String, _>("returned")
            .map_err(|_| corrupt_storage())?
            != returned.as_str()
        || row
            .try_get::<String, _>("explained")
            .map_err(|_| corrupt_storage())?
            != explained.as_str()
        || row
            .try_get::<String, _>("account_previous")
            .map_err(|_| corrupt_storage())?
            != account_previous
                .as_ref()
                .ok_or_else(corrupt_storage)?
                .as_str()
        || row
            .try_get::<Option<String>, _>("previous")
            .map_err(|_| corrupt_storage())?
            .as_deref()
            != previous.as_ref().map(DurableRequestId::as_str)
        || command.expected_stock_revision.value().checked_add(1) != Some(op.revision.value())
        || command.expected_catalog_revision != op.catalog_revision
        || command.fingerprint(hub, op.actor, own) != op.hash
    {
        return Err(corrupt_storage());
    }
    let packs = u32::try_from(number("packs")?).map_err(|_| corrupt_storage())?;
    if !(1..=1_000_000).contains(&packs) {
        return Err(corrupt_storage());
    }
    let root = Root {
        op,
        command,
        count,
        packs,
        epoch,
        capture: bytes("capture_hash")?,
        result: bytes("result_hash")?,
        projection: row.try_get("projection").map_err(|_| corrupt_storage())?,
        projection_hash: bytes("projection_hash")?,
    };
    if projection_seal(hub, &root)? != root.projection_hash {
        return Err(corrupt_storage());
    }
    Ok(root)
}
