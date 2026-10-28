use super::*;
use crate::hub::{reference, stock};
use harvestcircle_domain::hub::dispatch::{
    DispatchLineSnapshot, DispatchMaterial, DispatchSelection, DispatchSource,
};
use harvestcircle_domain::hub::reference::{
    ReferenceCatalog, SampleHandlingBasis, SourceProvenance,
};
use sqlx::Row;
use std::collections::BTreeMap;
use stock::read::{record_id, revision_field, text, whole_packs};

pub(super) async fn sources_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    catalog: &ReferenceCatalog,
    revision: HubRevision,
    selection: &DispatchSelection,
) -> Result<Vec<DispatchSource>, SafeError> {
    let mut sources = Vec::with_capacity(selection.lines().len());
    let mut traces = BTreeMap::new();
    for line in selection.lines() {
        let group = super::super::read::group_on(tx, hub, line.group, catalog).await?;
        if let std::collections::btree_map::Entry::Vacant(slot) = traces.entry(group.trace) {
            let loaded =
                super::super::lineage::load_on(tx, hub, group.trace, catalog, revision).await?;
            slot.insert(loaded.trace);
        }
        sources.push(DispatchSource {
            trace: traces.get(&group.trace).ok_or_else(corrupt)?.clone(),
            group,
        });
    }
    Ok(sources)
}

pub(super) async fn insert_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    preparation: HubRecordId,
    material: &DispatchMaterial,
) -> Result<(), SafeError> {
    material.validate().map_err(|_| corrupt())?;
    sqlx::query(concat!(
        "INSERT INTO hub_dispatch_materials (operation_id,preparation_id,captured_stock_revision,catalog_revision,recipient_id,recipient_revision,basis_id,basis_version,site_timezone,observation,line_count) ",
        "VALUES (?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(operation.as_str()).bind(preparation.as_bytes().as_slice()).bind(integer(material.stock_revision)?).bind(integer(material.catalog_revision)?)
        .bind(material.recipient.id.as_bytes().as_slice()).bind(integer(material.recipient.revision)?).bind(material.basis.id()).bind(i64::from(material.basis.version()))
        .bind(material.basis.site_timezone()).bind(material.observation.as_str()).bind(i64::try_from(material.lines.len()).map_err(|_|corrupt())?)
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for (index, line) in material.lines.iter().enumerate() {
        sqlx::query(concat!(
            "INSERT INTO hub_dispatch_material_lines (operation_id,ordinal,group_id,group_revision,trace_id,packs,product_id,product_revision,variant_id,variant_revision,location_id,location_revision) ",
            "VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
        )).bind(operation.as_str()).bind(i64::try_from(index).map_err(|_|corrupt())?).bind(line.group.as_bytes().as_slice()).bind(integer(line.group_revision)?)
            .bind(line.trace.id.as_bytes().as_slice()).bind(i64::from(line.packs.value())).bind(line.product.id.as_bytes().as_slice()).bind(integer(line.product.revision)?)
            .bind(line.variant.id.as_bytes().as_slice()).bind(integer(line.variant.revision)?).bind(line.location.id.as_bytes().as_slice()).bind(integer(line.location.revision)?)
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    preparation: HubRecordId,
    operation: &DurableRequestId,
) -> Result<DispatchMaterial, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(preparation_id,1,17) AS preparation_id,captured_stock_revision,catalog_revision,substr(recipient_id,1,17) AS recipient_id,recipient_revision,",
        "substr(basis_id,1,65) AS basis_id,basis_version,substr(site_timezone,1,65) AS site_timezone,substr(CAST(observation AS BLOB),1,513) AS observation,line_count ",
        "FROM hub_dispatch_materials WHERE operation_id=? LIMIT 1"
    )).bind(operation.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt)?;
    let captured = revision_field(&row, "captured_stock_revision")?;
    let catalog = revision_field(&row, "catalog_revision")?;
    let original = stock::read::operation_on(tx, hub, operation)
        .await?
        .ok_or_else(corrupt)?;
    if record_id(&row, "preparation_id")? != preparation
        || original.result != preparation
        || !matches!(
            original.kind.as_str(),
            "dispatch_prepare" | "dispatch_revise" | "dispatch_replace"
        )
        || captured.value().checked_add(1) != Some(original.revision.value())
        || catalog != original.catalog_revision
    {
        return Err(corrupt());
    }
    let declared: i64 = row.try_get("line_count").map_err(|_| corrupt())?;
    if !(1..=64).contains(&declared) {
        return Err(corrupt());
    }
    let basis = SampleHandlingBasis::from_stored(
        &row.try_get::<String, _>("basis_id")
            .map_err(|_| corrupt())?,
        u32::try_from(
            row.try_get::<i64, _>("basis_version")
                .map_err(|_| corrupt())?,
        )
        .map_err(|_| corrupt())?,
        &row.try_get::<String, _>("site_timezone")
            .map_err(|_| corrupt())?,
    )
    .map_err(|_| corrupt())?;
    let recipient = reference::record_at_on(
        tx,
        hub,
        record_id(&row, "recipient_id")?,
        revision_field(&row, "recipient_revision")?,
    )
    .await?;
    let observation = SourceProvenance::new(text(&row, "observation")?).map_err(|_| corrupt())?;
    let rows=sqlx::query(concat!(
        "SELECT ordinal,substr(group_id,1,17) AS group_id,group_revision,substr(trace_id,1,17) AS trace_id,packs,",
        "substr(product_id,1,17) AS product_id,product_revision,substr(variant_id,1,17) AS variant_id,variant_revision,substr(location_id,1,17) AS location_id,location_revision ",
        "FROM hub_dispatch_material_lines WHERE operation_id=? ORDER BY ordinal LIMIT 65"
    )).bind(operation.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != declared as usize {
        return Err(corrupt());
    }
    let mut lines = Vec::with_capacity(rows.len());
    let mut traces = BTreeMap::new();
    for (index, row) in rows.into_iter().enumerate() {
        if row.try_get::<i64, _>("ordinal").map_err(|_| corrupt())? != index as i64 {
            return Err(corrupt());
        }
        let trace_id = record_id(&row, "trace_id")?;
        if let std::collections::btree_map::Entry::Vacant(slot) = traces.entry(trace_id) {
            let trace = super::super::read::trace_on(tx, hub, trace_id).await?;
            slot.insert(trace);
        }
        let trace = traces.get(&trace_id).ok_or_else(corrupt)?;
        let group = record_id(&row, "group_id")?;
        let identification = original_identity_on(tx, hub, trace, group, captured).await?;
        lines.push(DispatchLineSnapshot {
            group,
            group_revision: revision_field(&row, "group_revision")?,
            packs: whole_packs(row.try_get("packs").map_err(|_| corrupt())?)?,
            identification,
            product: reference::record_at_on(
                tx,
                hub,
                record_id(&row, "product_id")?,
                revision_field(&row, "product_revision")?,
            )
            .await?,
            variant: reference::record_at_on(
                tx,
                hub,
                record_id(&row, "variant_id")?,
                revision_field(&row, "variant_revision")?,
            )
            .await?,
            location: reference::record_at_on(
                tx,
                hub,
                record_id(&row, "location_id")?,
                revision_field(&row, "location_revision")?,
            )
            .await?,
            trace: trace.clone(),
        });
    }
    let material = DispatchMaterial {
        stock_revision: captured,
        catalog_revision: catalog,
        basis,
        recipient,
        lines,
        observation,
    };
    material.validate().map_err(|_| corrupt())?;
    Ok(material)
}

// An issued line selects one exact group, so original recovery verifies its
// creator rather than scanning unrelated lineage or reading current quantities.
async fn original_identity_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: &ReceiptTrace,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<Option<PhysicalIdentification>, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(g.trace_id,1,17) AS trace_id,substr(i.parent_group_id,1,17) AS parent_group_id,substr(i.created_operation_id,1,37) AS created_operation_id,",
        "parent.stock_revision AS parent_revision FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id ",
        "LEFT JOIN hub_group_identification p ON p.group_id=i.parent_group_id LEFT JOIN hub_stock_operations parent ON parent.operation_id=p.created_operation_id WHERE g.group_id=? LIMIT 1"
    )).bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt)?;
    if record_id(&row, "trace_id")? != trace.id {
        return Err(corrupt());
    }
    let parent = stock::read::optional_id(&row, "parent_group_id")?;
    let id = DurableRequestId::parse(
        row.try_get::<String, _>("created_operation_id")
            .map_err(|_| corrupt())?,
    )
    .map_err(|_| corrupt())?;
    let operation = stock::read::operation_on(tx, hub, &id)
        .await?
        .ok_or_else(corrupt)?;
    let parent_revision = row
        .try_get::<Option<i64>, _>("parent_revision")
        .map_err(|_| corrupt())?;
    if operation.revision.value() > cut.value()
        || parent.is_some() != parent_revision.is_some()
        || parent_revision
            .is_some_and(|revision| revision < 2 || revision >= operation.revision.value() as i64)
    {
        return Err(corrupt());
    }
    let (_, identity, _, _) =
        super::super::lineage::creation_on(tx, hub, trace, group, parent, operation).await?;
    Ok(identity)
}
