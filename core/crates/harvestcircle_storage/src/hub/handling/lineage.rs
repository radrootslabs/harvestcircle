use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::portions::{
    HUB_LINEAGE_GROUPS, PhysicalLineage, PhysicalLineageNode,
};
use harvestcircle_domain::hub::reference::{ReferenceCatalog, ReferenceKind};
use sqlx::Row;
use stock::read::{optional_id, record_id, revision_field, whole_packs};

pub(in crate::hub) struct LoadedLineage {
    pub graph: PhysicalLineage,
    pub rows: Vec<HubLineageRow>,
    pub trace: StockPopulation,
}

impl Database {
    pub async fn hub_lineage(
        &self,
        actor: PublicKey,
        query: HubLineageQuery,
    ) -> Result<HubLineagePage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = reference::authorize_on(transaction, actor).await?;
                    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
                    let state = stock::read::state_on(transaction, hub.id()).await?;
                    if query
                        .expected_revision
                        .is_some_and(|expected| expected != state.revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|expected| expected != catalog.revision())
                    {
                        return Err(conflict());
                    }
                    let loaded =
                        load_on(transaction, hub.id(), query.trace, &catalog, state.revision)
                            .await?;
                    let total = u32::try_from(loaded.rows.len()).map_err(|_| corrupt_storage())?;
                    if query.offset > total {
                        return Err(invalid_input());
                    }
                    let product = catalog
                        .require_kind(loaded.trace.product.id, ReferenceKind::Product)
                        .map_err(|_| corrupt_storage())?
                        .clone();
                    let variant = catalog
                        .require_kind(loaded.trace.variant.id, ReferenceKind::Pack)
                        .map_err(|_| corrupt_storage())?
                        .clone();
                    let rows = loaded
                        .rows
                        .into_iter()
                        .skip(query.offset as usize)
                        .take(query.limit as usize)
                        .collect::<Vec<_>>();
                    Ok(HubLineagePage {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        offset: query.offset,
                        total,
                        more: query.offset
                            + u32::try_from(rows.len()).map_err(|_| corrupt_storage())?
                            < total,
                        product,
                        variant,
                        trace: loaded.trace,
                        rows,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(in crate::hub) async fn load_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace_id: HubRecordId,
    catalog: &ReferenceCatalog,
    revision: HubRevision,
) -> Result<LoadedLineage, SafeError> {
    reference::validate_basis_on(transaction, hub).await?;
    let trace = read::trace_on(transaction, hub, trace_id).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups WHERE trace_id=?")
        .bind(trace_id.as_bytes().as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    if !(1..=HUB_LINEAGE_GROUPS as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    let selected = sqlx::query(concat!(
        "SELECT substr(g.group_id,1,17) AS group_id,g.revision,substr(i.parent_group_id,1,17) AS parent_group_id,",
        "substr(i.created_operation_id,1,37) AS created_operation_id,COALESCE(p.packs,0) AS packs,substr(p.location_id,1,17) AS location_id,",
        "substr(CAST(i.physical_label AS BLOB),1,121) AS physical_label,substr(CAST(i.observation AS BLOB),1,513) AS physical_observation,",
        "(SELECT count(*) FROM hub_group_identification active WHERE active.physical_label=i.physical_label AND EXISTS(SELECT 1 FROM hub_stock_positions ap WHERE ap.group_id=active.group_id AND ap.packs>0)) AS active_label_count,",
        "(SELECT count(*) FROM hub_stock_positions invalid WHERE invalid.group_id=g.group_id AND (invalid.packs<0 OR invalid.packs>1000000)) AS invalid_positions ",
        "FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id ",
        "LEFT JOIN hub_stock_positions p ON p.group_id=g.group_id AND p.packs>0 WHERE g.trace_id=? ORDER BY g.group_id LIMIT 257"
    )).bind(trace_id.as_bytes().as_slice()).fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if selected.len() != count as usize {
        return Err(corrupt_storage());
    }
    let mut nodes = Vec::with_capacity(selected.len());
    let mut rows = Vec::with_capacity(selected.len());
    let mut total = WholePacks::from_value(0).map_err(|_| corrupt_storage())?;
    let mut labels = std::collections::BTreeSet::new();
    for row in selected {
        if row
            .try_get::<i64, _>("invalid_positions")
            .map_err(|_| corrupt_storage())?
            != 0
        {
            return Err(corrupt_storage());
        }
        let id = record_id(&row, "group_id")?;
        let parent = optional_id(&row, "parent_group_id")?;
        let packs = whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?;
        total = total.checked_add(packs).map_err(|_| corrupt_storage())?;
        let location = optional_id(&row, "location_id")?
            .map(|id| {
                catalog
                    .require_kind(id, ReferenceKind::Location)
                    .cloned()
                    .map_err(|_| corrupt_storage())
            })
            .transpose()?;
        if (packs.value() == 0) != location.is_none() {
            return Err(corrupt_storage());
        }
        let created_id = DurableRequestId::parse(
            row.try_get::<String, _>("created_operation_id")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let operation = stock::read::operation_on(transaction, hub, &created_id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let created_by = operation.actor;
        let created_at = operation.recorded_at;
        let created_revision = operation.revision;
        let (created_kind, identification, occurred, observation) =
            creation_on(transaction, hub, &trace, id, parent, operation).await?;
        let label: Option<Vec<u8>> = row
            .try_get("physical_label")
            .map_err(|_| corrupt_storage())?;
        let physical_observation: Option<Vec<u8>> = row
            .try_get("physical_observation")
            .map_err(|_| corrupt_storage())?;
        let stored_identity = match (label, physical_observation) {
            (None, None) => None,
            (Some(label), Some(observation)) => Some(PhysicalIdentification {
                label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                    .map_err(|_| corrupt_storage())?,
                observation: harvestcircle_domain::hub::reference::SourceProvenance::new(
                    String::from_utf8(observation).map_err(|_| corrupt_storage())?,
                )
                .map_err(|_| corrupt_storage())?,
            }),
            _ => return Err(corrupt_storage()),
        };
        if stored_identity != identification {
            return Err(corrupt_storage());
        }
        if packs.value() > 0
            && let Some(identity) = &identification
            && (row
                .try_get::<i64, _>("active_label_count")
                .map_err(|_| corrupt_storage())?
                != 1
                || !labels.insert(identity.label.as_str().to_owned()))
        {
            return Err(corrupt_storage());
        }
        nodes.push(PhysicalLineageNode {
            id,
            parent,
            trace: trace_id,
            variant: trace.variant.id,
            created_revision,
            packs,
        });
        rows.push(HubLineageRow {
            id,
            parent,
            identification,
            packs,
            revision: revision_field(&row, "revision")?,
            location,
            created_revision,
            created_kind,
            created_by,
            created_at,
            created_operation: created_id,
            occurred,
            observation,
        });
    }
    let counted_loss =
        crate::hub::counts::posting::loss_quantity_on(transaction, hub, trace_id).await?;
    if total
        .checked_add(super::departure::conservation::trace_on(transaction, hub, trace_id).await?)
        .and_then(|packs| packs.checked_add(counted_loss))
        .map_err(|_| corrupt_storage())?
        != trace.original_packs
    {
        return Err(corrupt_storage());
    }
    let graph = PhysicalLineage::new(hub, trace_id, trace.variant.id, revision, nodes)
        .map_err(|_| corrupt_storage())?;
    Ok(LoadedLineage { graph, rows, trace })
}

pub(super) async fn creation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: &StockPopulation,
    id: HubRecordId,
    parent: Option<HubRecordId>,
    operation: stock::read::OperationRecord,
) -> Result<
    (
        HubGroupCreationKind,
        Option<PhysicalIdentification>,
        Occurrence,
        harvestcircle_domain::hub::reference::SourceProvenance,
    ),
    SafeError,
> {
    Ok(match operation.kind.as_str() {
        "opening" if parent.is_none() => {
            let original = stock::opening::original_on(transaction, hub, operation).await?;
            if original.original.group.id != id || original.original.population != *trace {
                return Err(corrupt_storage());
            }
            (
                HubGroupCreationKind::CountedOpening,
                original.original.group.identification,
                trace.occurred,
                trace.observation.clone(),
            )
        }
        "count_post" if parent.is_none() => {
            let new = crate::hub::counts::posting::original_on(transaction, hub, operation)
                .await?
                .surplus
                .ok_or_else(corrupt_storage)?;
            if new.group.id != id || new.population != *trace {
                return Err(corrupt_storage());
            }
            (
                HubGroupCreationKind::CountedSurplus,
                new.group.identification,
                trace.occurred,
                trace.observation.clone(),
            )
        }
        "receipt" if parent.is_none() => {
            let original = stock::read::receipt_on(transaction, hub, operation).await?;
            if original.original.position.group != id || original.original.trace != *trace {
                return Err(corrupt_storage());
            }
            (
                HubGroupCreationKind::Receipt,
                None,
                trace.occurred,
                trace.observation.clone(),
            )
        }
        "move" if parent.is_some() => {
            let original = history::movement_on(transaction, hub, operation).await?;
            let HubHandlingResult::Moved(result) = original.result else {
                return Err(corrupt_storage());
            };
            if original.trace != trace.id
                || result.moved_group != id
                || Some(result.selected_group) != parent
            {
                return Err(corrupt_storage());
            }
            let identification = result.separated_portion.ok_or_else(corrupt_storage)?;
            (
                HubGroupCreationKind::Movement,
                Some(identification),
                result.occurred,
                result.observation,
            )
        }
        "split" if parent.is_some() => {
            let original = portions::original_on(transaction, hub, operation).await?;
            let HubHandlingResult::Separated(result) = original.result else {
                return Err(corrupt_storage());
            };
            if original.trace != trace.id
                || result.portion_group != id
                || Some(result.selected_group) != parent
            {
                return Err(corrupt_storage());
            }
            (
                HubGroupCreationKind::Separation,
                Some(result.identification),
                result.occurred,
                result.observation,
            )
        }
        _ => return Err(corrupt_storage()),
    })
}

/// Original recovery uses only creation history through its captured cut.
/// Current stock positions, group revisions and inventory counters are irrelevant.
pub(super) async fn historical_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace_id: HubRecordId,
    cut: HubRevision,
) -> Result<PhysicalLineage, SafeError> {
    let trace = read::trace_on(transaction, hub, trace_id).await?;
    let rows=sqlx::query(concat!(
        "SELECT substr(g.group_id,1,17) AS group_id,substr(i.parent_group_id,1,17) AS parent_group_id,substr(i.created_operation_id,1,37) AS created_operation_id,",
        "substr(CAST(i.physical_label AS BLOB),1,121) AS physical_label,substr(CAST(i.observation AS BLOB),1,513) AS physical_observation ",
        "FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id JOIN hub_stock_operations o ON o.operation_id=i.created_operation_id ",
        "WHERE g.trace_id=? AND o.hub_id=? AND o.stock_revision<=? ORDER BY g.group_id LIMIT 257"
    )).bind(trace_id.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(integer(cut)?).fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if rows.is_empty() || rows.len() > HUB_LINEAGE_GROUPS {
        return Err(corrupt_storage());
    }
    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        let id = record_id(&row, "group_id")?;
        let parent = optional_id(&row, "parent_group_id")?;
        let operation = DurableRequestId::parse(
            row.try_get::<String, _>("created_operation_id")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let operation = stock::read::operation_on(transaction, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let born = operation.revision;
        let (_, identification, _, _) =
            creation_on(transaction, hub, &trace, id, parent, operation).await?;
        let label: Option<Vec<u8>> = row
            .try_get("physical_label")
            .map_err(|_| corrupt_storage())?;
        let observation: Option<Vec<u8>> = row
            .try_get("physical_observation")
            .map_err(|_| corrupt_storage())?;
        let stored = match (label, observation) {
            (None, None) => None,
            (Some(label), Some(observation)) => Some(PhysicalIdentification {
                label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                    .map_err(|_| corrupt_storage())?,
                observation: harvestcircle_domain::hub::reference::SourceProvenance::new(
                    String::from_utf8(observation).map_err(|_| corrupt_storage())?,
                )
                .map_err(|_| corrupt_storage())?,
            }),
            _ => return Err(corrupt_storage()),
        };
        if stored != identification {
            return Err(corrupt_storage());
        }
        nodes.push(PhysicalLineageNode {
            id,
            parent,
            trace: trace_id,
            variant: trace.variant.id,
            created_revision: born,
            packs: WholePacks::from_value(0).expect("zero packs"),
        });
    }
    PhysicalLineage::new(hub, trace_id, trace.variant.id, cut, nodes).map_err(|_| corrupt_storage())
}
