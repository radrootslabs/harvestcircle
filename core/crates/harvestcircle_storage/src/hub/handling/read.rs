use super::super::stock;
use super::*;
use crate::db::corrupt_storage;
use harvestcircle_domain::hub::movement::{
    HUB_GROUP_REQUIREMENTS, HUB_REQUIREMENT_GROUPS, WorkAllocation, WorkProgress,
};
use harvestcircle_domain::hub::reference::{ReferenceCatalog, ReferenceKind, SourceProvenance};
use harvestcircle_domain::hub::stock::QuantityWork;
use sqlx::Row;
use stock::read::{optional_id, optional_record_on, record_id, revision_field, whole_packs};

pub(super) struct WorkDefinition {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub target: Option<ReferenceRecord>,
    pub required: WholePacks,
    pub created_operation: DurableRequestId,
}

impl WorkDefinition {
    pub fn at(
        &self,
        quantity: QuantityWork,
        revision: HubRevision,
    ) -> Result<WorkRequirement, SafeError> {
        if quantity.required() != self.required {
            return Err(corrupt_storage());
        }
        let work = WorkRequirement {
            id: self.id,
            trace: self.trace,
            variant: self.variant,
            target: self.target.clone(),
            quantity,
            revision,
        };
        work.validate().map_err(|_| corrupt_storage())?;
        Ok(work)
    }
}

pub(super) async fn definition_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<WorkDefinition, SafeError> {
    let row = sqlx::query(concat!(
        "SELECT substr(w.work_id,1,17) AS work_id,substr(w.trace_id,1,17) AS trace_id,substr(w.variant_id,1,17) AS variant_id,substr(w.target_location_id,1,17) AS target_location_id,w.target_location_revision,w.required,substr(w.created_operation_id,1,37) AS created_operation_id,",
        "substr(o.kind,1,33) AS created_kind,substr(o.result_id,1,17) AS created_result,substr(r.variant_id,1,17) AS original_variant,r.original_packs,substr(r.planned_location_id,1,17) AS original_target,r.planned_location_revision AS original_target_revision ",
        "FROM hub_work_requirements w JOIN hub_stock_operations o ON o.operation_id=w.created_operation_id JOIN hub_receipts r ON r.trace_id=w.trace_id WHERE w.work_id=? LIMIT 1"
    ))
        .bind(id.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?.ok_or_else(invalid_input)?;
    let target = optional_record_on(
        transaction,
        hub,
        &row,
        "target_location_id",
        "target_location_revision",
    )
    .await?;
    if target
        .as_ref()
        .is_some_and(|target| target.value.kind() != ReferenceKind::Location)
    {
        return Err(corrupt_storage());
    }
    let trace = record_id(&row, "trace_id")?;
    let variant = record_id(&row, "variant_id")?;
    let required = whole_packs(row.try_get("required").map_err(|_| corrupt_storage())?)?;
    let created_operation = DurableRequestId::parse(
        row.try_get::<String, _>("created_operation_id")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if variant != record_id(&row, "original_variant")? {
        return Err(corrupt_storage());
    }
    match row
        .try_get::<String, _>("created_kind")
        .map_err(|_| corrupt_storage())?
        .as_str()
    {
        "receipt"
            if id == trace
                && record_id(&row, "created_result")? == trace
                && required
                    == whole_packs(
                        row.try_get("original_packs")
                            .map_err(|_| corrupt_storage())?,
                    )?
                && target.as_ref().map(|record| record.id)
                    == optional_id(&row, "original_target")?
                && target.as_ref().map(|record| {
                    i64::try_from(record.revision.value()).expect("checked hub revision")
                }) == row
                    .try_get::<Option<i64>, _>("original_target_revision")
                    .map_err(|_| corrupt_storage())? => {}
        "work_replan" | "work_new"
            if record_id(&row, "created_result")? == id
                && handling_ids(&created_operation)?.requirement == id => {}
        _ => return Err(corrupt_storage()),
    }
    Ok(WorkDefinition {
        id: record_id(&row, "work_id")?,
        trace,
        variant,
        target,
        required,
        created_operation,
    })
}

pub(in crate::hub) async fn work_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<WorkRequirement, SafeError> {
    let definition = definition_on(transaction, hub, id).await?;
    let row = sqlx::query("SELECT completed,no_longer_required,remaining,revision FROM hub_work_requirements WHERE work_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let quantity = QuantityWork::restore(
        definition.required,
        whole_packs(row.try_get("completed").map_err(|_| corrupt_storage())?)?,
        whole_packs(
            row.try_get("no_longer_required")
                .map_err(|_| corrupt_storage())?,
        )?,
        whole_packs(row.try_get("remaining").map_err(|_| corrupt_storage())?)?,
    )
    .map_err(|_| corrupt_storage())?;
    let scope = sqlx::query(concat!(
        "SELECT count(*) AS n,COALESCE(sum(a.scoped_packs),0) AS total,",
        "COALESCE(sum(CASE WHEN a.progress='pending' THEN a.scoped_packs ELSE 0 END),0) AS pending,",
        "COALESCE(sum(CASE WHEN a.progress='completed' THEN a.scoped_packs ELSE 0 END),0) AS completed,",
        "COALESCE(sum(CASE WHEN a.progress='no_longer_required' THEN a.scoped_packs ELSE 0 END),0) AS retired,",
        "count(CASE WHEN g.trace_id<>? OR r.variant_id<>? OR (a.progress='pending' AND a.scoped_packs<>(SELECT COALESCE(sum(p.packs),0) FROM hub_stock_positions p WHERE p.group_id=a.group_id)) THEN 1 END) AS invalid ",
        "FROM hub_group_work a JOIN hub_stock_groups g ON g.group_id=a.group_id JOIN hub_receipts r ON r.trace_id=g.trace_id WHERE a.work_id=?"
    )).bind(definition.trace.as_bytes().as_slice()).bind(definition.variant.as_bytes().as_slice()).bind(id.as_bytes().as_slice())
        .fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let count: i64 = scope.try_get("n").map_err(|_| corrupt_storage())?;
    if !(1..=HUB_REQUIREMENT_GROUPS as i64).contains(&count)
        || scope
            .try_get::<i64, _>("invalid")
            .map_err(|_| corrupt_storage())?
            != 0
        || whole_packs(scope.try_get("total").map_err(|_| corrupt_storage())?)?
            != quantity.required()
        || whole_packs(scope.try_get("pending").map_err(|_| corrupt_storage())?)?
            != quantity.remaining()
        || whole_packs(scope.try_get("completed").map_err(|_| corrupt_storage())?)?
            != quantity.completed()
        || whole_packs(scope.try_get("retired").map_err(|_| corrupt_storage())?)?
            != quantity.no_longer_required()
    {
        return Err(corrupt_storage());
    }
    definition.at(quantity, revision_field(&row, "revision")?)
}

pub(in crate::hub) async fn group_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
    catalog: &ReferenceCatalog,
) -> Result<PhysicalStockGroup, SafeError> {
    let row = sqlx::query(concat!(
        "SELECT substr(g.group_id,1,17) AS group_id,substr(g.trace_id,1,17) AS trace_id,substr(r.variant_id,1,17) AS variant_id,g.revision,",
        "substr(r.root_group_id,1,17) AS root_group_id,substr(i.parent_group_id,1,17) AS parent_group_id,",
        "substr(CAST(i.physical_label AS BLOB),1,121) AS physical_label,substr(CAST(i.observation AS BLOB),1,513) AS observation,",
        "o.stock_revision AS created_revision,substr(o.kind,1,33) AS created_kind,substr(i.created_operation_id,1,37) AS created_operation_id,substr(r.operation_id,1,37) AS receipt_operation_id,",
        "substr(md.selected_group_id,1,17) AS selected_parent,substr(md.moved_group_id,1,17) AS created_group,po.stock_revision AS parent_created_revision,substr(pg.trace_id,1,17) AS parent_trace ",
        "FROM hub_stock_groups g JOIN hub_receipts r ON r.trace_id=g.trace_id JOIN hub_group_identification i ON i.group_id=g.group_id ",
        "JOIN hub_stock_operations o ON o.operation_id=i.created_operation_id ",
        "LEFT JOIN hub_movement_details md ON md.operation_id=i.created_operation_id ",
        "LEFT JOIN hub_group_identification pi ON pi.group_id=i.parent_group_id LEFT JOIN hub_stock_groups pg ON pg.group_id=pi.group_id ",
        "LEFT JOIN hub_stock_operations po ON po.operation_id=pi.created_operation_id WHERE g.group_id=? LIMIT 1"
    )).bind(id.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?.ok_or_else(invalid_input)?;
    let trace = record_id(&row, "trace_id")?;
    let mass = sqlx::query("SELECT r.original_packs,COALESCE((SELECT sum(p.packs) FROM hub_stock_groups g JOIN hub_stock_positions p ON p.group_id=g.group_id WHERE g.trace_id=r.trace_id),0) AS on_hand FROM hub_receipts r WHERE r.trace_id=? LIMIT 1")
        .bind(trace.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if whole_packs(
        mass.try_get("original_packs")
            .map_err(|_| corrupt_storage())?,
    )? != whole_packs(mass.try_get("on_hand").map_err(|_| corrupt_storage())?)?
    {
        return Err(corrupt_storage());
    }
    let parent = optional_id(&row, "parent_group_id")?;
    let label: Option<Vec<u8>> = row
        .try_get("physical_label")
        .map_err(|_| corrupt_storage())?;
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
    let created: i64 = row
        .try_get("created_revision")
        .map_err(|_| corrupt_storage())?;
    let created_kind: String = row.try_get("created_kind").map_err(|_| corrupt_storage())?;
    match parent {
        None if record_id(&row, "root_group_id")? == id
            && created_kind == "receipt"
            && identification.is_none()
            && row
                .try_get::<String, _>("created_operation_id")
                .map_err(|_| corrupt_storage())?
                == row
                    .try_get::<String, _>("receipt_operation_id")
                    .map_err(|_| corrupt_storage())? => {}
        Some(parent)
            if optional_id(&row, "parent_trace")? == Some(trace)
                && row
                    .try_get::<Option<i64>, _>("parent_created_revision")
                    .map_err(|_| corrupt_storage())?
                    .is_some_and(|before| before < created)
                && created_kind == "move"
                && optional_id(&row, "selected_parent")? == Some(parent)
                && optional_id(&row, "created_group")? == Some(id) => {}
        _ => return Err(corrupt_storage()),
    }
    if let Some(identity) = &identification {
        let duplicates: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_group_identification i WHERE i.physical_label=? AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0)")
            .bind(identity.label.as_str()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
        if duplicates != 1 {
            return Err(corrupt_storage());
        }
    }
    let positions = sqlx::query("SELECT substr(location_id,1,17) AS location_id,packs FROM hub_stock_positions WHERE group_id=? AND packs>0 ORDER BY location_id LIMIT 2")
        .bind(id.as_bytes().as_slice()).fetch_all(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if positions.len() != 1 {
        return Err(corrupt_storage());
    }
    let location = record_id(&positions[0], "location_id")?;
    let packs = whole_packs(
        positions[0]
            .try_get("packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let selected = sqlx::query("SELECT substr(work_id,1,17) AS work_id,substr(progress,1,19) AS progress,scoped_packs FROM hub_group_work WHERE group_id=? ORDER BY work_id LIMIT 65")
        .bind(id.as_bytes().as_slice()).fetch_all(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if selected.len() > HUB_GROUP_REQUIREMENTS {
        return Err(corrupt_storage());
    }
    let mut work = Vec::with_capacity(selected.len());
    for row in selected {
        if whole_packs(row.try_get("scoped_packs").map_err(|_| corrupt_storage())?)? != packs {
            return Err(corrupt_storage());
        }
        let progress = match row
            .try_get::<String, _>("progress")
            .map_err(|_| corrupt_storage())?
            .as_str()
        {
            "pending" => WorkProgress::Pending,
            "completed" => WorkProgress::Completed,
            "no_longer_required" => WorkProgress::NoLongerRequired,
            _ => return Err(corrupt_storage()),
        };
        work.push(WorkAllocation {
            requirement: work_on(transaction, hub, record_id(&row, "work_id")?).await?,
            progress,
        });
    }
    let causal_floor = causal_floor_on(transaction, id).await?;
    let group = PhysicalStockGroup {
        id,
        trace,
        variant: record_id(&row, "variant_id")?,
        parent,
        identification,
        location,
        packs,
        revision: revision_field(&row, "revision")?,
        causal_floor,
        work,
    };
    group.validate().map_err(|_| corrupt_storage())?;
    catalog
        .require_kind(group.variant, ReferenceKind::Pack)
        .map_err(|_| corrupt_storage())?;
    catalog
        .require_kind(group.location, ReferenceKind::Location)
        .map_err(|_| corrupt_storage())?;
    Ok(group)
}

async fn causal_floor_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    group: HubRecordId,
) -> Result<Option<UnixTimestamp>, SafeError> {
    let row=sqlx::query(concat!(
        "WITH RECURSIVE lineage(group_id,ceiling,depth) AS (SELECT ?,9223372036854775807,0 UNION ALL ",
        "SELECT i.parent_group_id,min(l.ceiling,o.stock_revision-1),l.depth+1 FROM lineage l JOIN hub_group_identification i ON i.group_id=l.group_id ",
        "JOIN hub_stock_operations o ON o.operation_id=i.created_operation_id WHERE i.parent_group_id IS NOT NULL AND l.depth<256) ",
        "SELECT max(CASE WHEN e.occurrence_kind IN ('known','interval') THEN e.occurrence_start_unix_s END) AS floor,",
        "(SELECT count(*) FROM lineage l JOIN hub_group_identification i ON i.group_id=l.group_id WHERE l.depth=256 AND i.parent_group_id IS NOT NULL) AS truncated ",
        "FROM lineage l LEFT JOIN hub_stock_events e ON e.group_id=l.group_id AND e.kind IN ('receipt','move') ",
        "LEFT JOIN hub_stock_operations o ON o.operation_id=e.operation_id WHERE o.stock_revision<=l.ceiling OR o.stock_revision IS NULL"
    )).bind(group.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if row
        .try_get::<i64, _>("truncated")
        .map_err(|_| corrupt_storage())?
        != 0
    {
        return Err(corrupt_storage());
    }
    row.try_get::<Option<i64>, _>("floor")
        .map_err(|_| corrupt_storage())?
        .map(stock::read::timestamp)
        .transpose()
}

pub(super) async fn trace_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<ReceiptTrace, SafeError> {
    let operation: String = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_receipts WHERE trace_id=? LIMIT 1",
    )
    .bind(trace.as_bytes().as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(invalid_input)?;
    let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
    let operation = stock::read::operation_on(transaction, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    Ok(stock::read::receipt_on(transaction, hub, operation)
        .await?
        .original
        .trace)
}
