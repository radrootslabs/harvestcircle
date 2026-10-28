use super::*;
use crate::hub::{reference, stock};
use crate::{Database, db::map_transaction_error};
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchStamp, IdentificationStatus, IdentificationVersion,
    dispatch_result_digest,
};
use harvestcircle_domain::hub::reference::SourceProvenance;
use sqlx::Row;
use stock::read::{OperationRecord, optional_id, record_id, revision_field, text};

pub(super) struct Change {
    pub operation: OperationRecord,
    pub revision: HubRevision,
    pub expected: Option<HubRevision>,
    pub kind: HubDispatchKind,
    pub material: DurableRequestId,
    pub identification: Option<HubRecordId>,
    pub previous: Option<HubRecordId>,
    pub observation: SourceProvenance,
    pub digest: [u8; 32],
}
impl Change {
    fn stamp(&self) -> Result<DispatchStamp, SafeError> {
        Ok(DispatchStamp {
            operation: identities(&self.operation.id)?.operation,
            stock_revision: self.operation.revision,
            actor: self.operation.actor,
            recorded_at: self.operation.recorded_at,
            observation: self.observation.clone(),
        })
    }
}

pub(super) async fn head_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<Option<Change>, SafeError> {
    let op:Option<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_dispatch_changes WHERE preparation_id=? ORDER BY record_revision DESC LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    match op {
        Some(op) => Ok(Some(
            change_id_on(
                tx,
                hub,
                &DurableRequestId::parse(op).map_err(|_| corrupt())?,
            )
            .await?,
        )),
        None => Ok(None),
    }
}

async fn change_id_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: &DurableRequestId,
) -> Result<Change, SafeError> {
    let operation = stock::read::operation_on(tx, hub, id)
        .await?
        .ok_or_else(corrupt)?;
    change_on(tx, operation).await
}

async fn change_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: OperationRecord,
) -> Result<Change, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(preparation_id,1,17) AS preparation_id,record_revision,expected_record_revision,substr(kind,1,33) AS kind,",
        "substr(material_operation_id,1,37) AS material_operation_id,substr(identification_id,1,17) AS identification_id,substr(previous_identification_id,1,17) AS previous_identification_id,",
        "substr(CAST(observation AS BLOB),1,513) AS observation,substr(result_hash,1,33) AS result_hash FROM hub_dispatch_changes WHERE operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt)?;
    let kind: String = row.try_get("kind").map_err(|_| corrupt())?;
    let revision = revision_field(&row, "record_revision")?;
    let expected = row
        .try_get::<Option<i64>, _>("expected_record_revision")
        .map_err(|_| corrupt())?
        .map(stock::read::revision)
        .transpose()?;
    let material = DurableRequestId::parse(
        row.try_get::<String, _>("material_operation_id")
            .map_err(|_| corrupt())?,
    )
    .map_err(|_| corrupt())?;
    let identification = optional_id(&row, "identification_id")?;
    let previous = optional_id(&row, "previous_identification_id")?;
    let parsed = HubDispatchKind::parse(&kind)?;
    let derived = identities(&operation.id)?;
    if operation.result != record_id(&row, "preparation_id")?
        || operation.kind != kind
        || (parsed == HubDispatchKind::Prepare) != (revision == HubRevision::INITIAL)
        || (parsed == HubDispatchKind::Prepare) != (expected.is_none())
        || expected
            .is_some_and(|expected| expected.value().checked_add(1) != Some(revision.value()))
        || matches!(
            parsed,
            HubDispatchKind::Prepare | HubDispatchKind::Revise | HubDispatchKind::Replace
        ) != (material == operation.id)
        || matches!(parsed, HubDispatchKind::Issue | HubDispatchKind::Replace)
            != identification.is_some()
        || identification.is_some_and(|id| id != derived.identification)
        || matches!(
            parsed,
            HubDispatchKind::Replace | HubDispatchKind::MarkUnused
        ) != previous.is_some()
        || (parsed == HubDispatchKind::Prepare && operation.result != derived.preparation)
    {
        return Err(corrupt());
    }
    let digest: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt())?;
    Ok(Change {
        operation,
        revision,
        expected,
        kind: parsed,
        material,
        identification,
        previous,
        observation: SourceProvenance::new(text(&row, "observation")?).map_err(|_| corrupt())?,
        digest: digest.try_into().map_err(|_| corrupt())?,
    })
}

pub(super) async fn record_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<DispatchRecord, SafeError> {
    let head = head_on(tx, hub, id).await?.ok_or_else(invalid_input)?;
    Ok(original_change_on(tx, hub, head).await?.record)
}

pub(super) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubDispatchReceipt, SafeError> {
    let change = change_on(tx, operation).await?;
    original_change_on(tx, hub, change).await
}

async fn original_change_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    change: Change,
) -> Result<HubDispatchReceipt, SafeError> {
    let record = restore_on(tx, hub, &change).await?;
    let material = record
        .draft
        .as_ref()
        .or_else(|| record.versions.last().map(|version| &version.material))
        .ok_or_else(corrupt)?;
    let target = || -> Result<(HubRecordId, HubRevision), SafeError> {
        Ok((record.id, change.expected.ok_or_else(corrupt)?))
    };
    let action = match change.kind {
        HubDispatchKind::Prepare => DispatchAction::Prepare {
            recipient: material.recipient.id,
            lines: material.selection().map_err(|_| corrupt())?,
        },
        HubDispatchKind::Revise => {
            let (preparation, expected_revision) = target()?;
            DispatchAction::Revise {
                preparation,
                expected_revision,
                recipient: material.recipient.id,
                lines: material.selection().map_err(|_| corrupt())?,
            }
        }
        HubDispatchKind::Issue => {
            let (preparation, expected_revision) = target()?;
            DispatchAction::Issue {
                preparation,
                expected_revision,
            }
        }
        HubDispatchKind::Replace => {
            let (preparation, expected_revision) = target()?;
            DispatchAction::Replace {
                preparation,
                expected_revision,
                recipient: material.recipient.id,
                lines: material.selection().map_err(|_| corrupt())?,
            }
        }
        HubDispatchKind::MarkUnused => {
            let (preparation, expected_revision) = target()?;
            DispatchAction::MarkUnused {
                preparation,
                expected_revision,
            }
        }
    };
    let command = DispatchCommand {
        expected_stock_revision: HubRevision::from_value(
            change
                .operation
                .revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt)?,
        )
        .map_err(|_| corrupt())?,
        expected_catalog_revision: change.operation.catalog_revision,
        action,
        observation: change.observation.clone(),
    };
    if command.fingerprint(
        hub,
        change.operation.actor,
        identities(&change.operation.id)?,
    ) != change.operation.hash
        || dispatch_result_digest(&change.operation.hash, &record).map_err(|_| corrupt())?
            != change.digest
    {
        return Err(corrupt());
    }
    super::super::history::single_event_on(tx, &change.operation.id).await?;
    let event=sqlx::query(concat!(
        "SELECT sequence,substr(kind,1,33) AS kind,substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,packs,",
        "substr(from_location_id,1,17) AS from_location_id,from_location_revision,substr(to_location_id,1,17) AS to_location_id,to_location_revision,",
        "substr(occurrence_kind,1,9) AS occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s FROM hub_stock_events WHERE operation_id=? LIMIT 1"
    )).bind(change.operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt)?;
    let line = material.lines.first().ok_or_else(corrupt)?;
    if !(1..=32_768).contains(&event.try_get::<i64, _>("sequence").map_err(|_| corrupt())?)
        || event.try_get::<String, _>("kind").map_err(|_| corrupt())? != change.operation.kind
        || record_id(&event, "trace_id")? != line.trace.id
        || record_id(&event, "group_id")? != line.group
        || event.try_get::<i64, _>("packs").map_err(|_| corrupt())? != 0
        || optional_id(&event, "from_location_id")?.is_some()
        || optional_id(&event, "to_location_id")?.is_some()
        || event
            .try_get::<Option<i64>, _>("from_location_revision")
            .map_err(|_| corrupt())?
            .is_some()
        || event
            .try_get::<Option<i64>, _>("to_location_revision")
            .map_err(|_| corrupt())?
            .is_some()
        || stock::read::occurrence(&event)? != Occurrence::Unknown
    {
        return Err(corrupt());
    }
    Ok(HubDispatchReceipt {
        operation: change.operation.id,
        kind: change.kind,
        catalog_revision: change.operation.catalog_revision,
        record,
    })
}

async fn restore_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    head: &Change,
) -> Result<DispatchRecord, SafeError> {
    let id = head.operation.result;
    let history=sqlx::query(concat!(
        "SELECT count(*) AS n,COALESCE(max(c.record_revision),0) AS latest,COALESCE(max(o.stock_revision),0) AS stock,",
        "count(CASE WHEN o.hub_id<>? OR o.result_id<>c.preparation_id OR o.kind<>c.kind THEN 1 END) AS invalid ",
        "FROM hub_dispatch_changes c JOIN hub_stock_operations o ON o.operation_id=c.operation_id WHERE c.preparation_id=? AND c.record_revision<=?"
    )).bind(hub.as_bytes().as_slice()).bind(id.as_bytes().as_slice()).bind(integer(head.revision)?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if history.try_get::<i64, _>("n").map_err(|_| corrupt())? != integer(head.revision)?
        || history.try_get::<i64, _>("latest").map_err(|_| corrupt())? != integer(head.revision)?
        || history.try_get::<i64, _>("stock").map_err(|_| corrupt())?
            != integer(head.operation.revision)?
        || history
            .try_get::<i64, _>("invalid")
            .map_err(|_| corrupt())?
            != 0
    {
        return Err(corrupt());
    }
    let created:String=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_dispatch_changes WHERE preparation_id=? AND record_revision=1 LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt)?;
    let created = change_id_on(
        tx,
        hub,
        &DurableRequestId::parse(created).map_err(|_| corrupt())?,
    )
    .await?;
    let rows=sqlx::query(concat!(
        "SELECT substr(i.identification_id,1,17) AS identification_id,i.version_number,substr(i.previous_identification_id,1,17) AS previous_identification_id,",
        "substr(i.issued_operation_id,1,37) AS issued_operation_id,substr(i.material_operation_id,1,37) AS material_operation_id ",
        "FROM hub_dispatch_identifications i JOIN hub_stock_operations o ON o.operation_id=i.issued_operation_id WHERE i.preparation_id=? AND o.stock_revision<=? ORDER BY i.version_number LIMIT 33"
    )).bind(id.as_bytes().as_slice()).bind(integer(head.operation.revision)?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > 32 {
        return Err(corrupt());
    }
    let mut versions = Vec::with_capacity(rows.len());
    let mut last_material = None;
    for row in rows {
        let issued_id = DurableRequestId::parse(
            row.try_get::<String, _>("issued_operation_id")
                .map_err(|_| corrupt())?,
        )
        .map_err(|_| corrupt())?;
        let issued = change_id_on(tx, hub, &issued_id).await?;
        let identification = record_id(&row, "identification_id")?;
        let previous = optional_id(&row, "previous_identification_id")?;
        let material_id = DurableRequestId::parse(
            row.try_get::<String, _>("material_operation_id")
                .map_err(|_| corrupt())?,
        )
        .map_err(|_| corrupt())?;
        if !matches!(
            issued.kind,
            HubDispatchKind::Issue | HubDispatchKind::Replace
        ) || issued.operation.result != id
            || issued.identification != Some(identification)
            || issued.previous != previous
            || issued.material != material_id
        {
            return Err(corrupt());
        }
        let closed_ids:Vec<String>=sqlx::query_scalar(concat!(
            "SELECT substr(c.operation_id,1,37) FROM hub_dispatch_changes c JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
            "WHERE c.preparation_id=? AND c.previous_identification_id=? AND c.kind IN ('dispatch_replace','dispatch_unused') AND o.stock_revision<=? ORDER BY o.stock_revision LIMIT 2"
        )).bind(id.as_bytes().as_slice()).bind(identification.as_bytes().as_slice()).bind(integer(head.operation.revision)?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if closed_ids.len() > 1 {
            return Err(corrupt());
        }
        let (status, closed) = if let Some(closed) = closed_ids.first() {
            let close = change_id_on(
                tx,
                hub,
                &DurableRequestId::parse(closed.clone()).map_err(|_| corrupt())?,
            )
            .await?;
            let status = match close.kind {
                HubDispatchKind::Replace => IdentificationStatus::Superseded,
                HubDispatchKind::MarkUnused => IdentificationStatus::Unused,
                _ => return Err(corrupt()),
            };
            (status, Some(close.stamp()?))
        } else {
            (IdentificationStatus::Issued, None)
        };
        versions.push(IdentificationVersion {
            id: identification,
            number: u32::try_from(
                row.try_get::<i64, _>("version_number")
                    .map_err(|_| corrupt())?,
            )
            .map_err(|_| corrupt())?,
            previous,
            material: material::load_on(tx, hub, id, &material_id).await?,
            issued: issued.stamp()?,
            status,
            closed,
        });
        last_material = Some(material_id);
    }
    let draft = if matches!(
        head.kind,
        HubDispatchKind::Prepare | HubDispatchKind::Revise
    ) {
        Some(material::load_on(tx, hub, id, &head.material).await?)
    } else {
        let last = versions.last().ok_or_else(corrupt)?;
        if last_material.as_ref() != Some(&head.material)
            || (matches!(head.kind, HubDispatchKind::Issue | HubDispatchKind::Replace)
                && head.identification != Some(last.id))
            || (head.kind == HubDispatchKind::MarkUnused && head.previous != Some(last.id))
        {
            return Err(corrupt());
        }
        None
    };
    let record = DispatchRecord {
        id,
        hub,
        revision: head.revision,
        created: created.stamp()?,
        updated: head.stamp()?,
        draft,
        versions,
    };
    record.validate().map_err(|_| corrupt())?;
    Ok(record)
}

impl Database {
    pub async fn hub_dispatch_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubDispatchReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        Some(original) => Ok(Some(original_on(tx, hub.id(), original).await?)),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
    pub async fn hub_dispatch_record(
        &self,
        actor: PublicKey,
        id: HubRecordId,
    ) -> Result<HubDispatchView, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    let state = stock::read::state_on(tx, hub.id()).await?;
                    Ok(HubDispatchView {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        record: record_on(tx, hub.id(), id).await?,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

macro_rules! dispatch_rows {
    ($prefix:literal,$suffix:literal)=>{concat!($prefix,
        " FROM hub_dispatch_changes c JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "JOIN hub_dispatch_materials m ON m.operation_id=c.material_operation_id JOIN hub_reference_versions r ON r.record_id=m.recipient_id AND r.record_revision=m.recipient_revision ",
        "WHERE o.hub_id=? AND NOT EXISTS (SELECT 1 FROM hub_dispatch_changes newer WHERE newer.preparation_id=c.preparation_id AND newer.record_revision>c.record_revision) ",
        "AND (?=0 OR (?=1 AND c.kind IN ('dispatch_prepare','dispatch_revise')) OR (?=2 AND c.kind IN ('dispatch_issue','dispatch_replace')) OR (?=3 AND c.kind='dispatch_unused')) ",
        "AND (?='' OR instr(lower(r.label),lower(?))>0 OR instr(lower(m.observation),lower(?))>0)",$suffix)};
}
macro_rules! dispatch_filter {
    ($sql:expr,$hub:expr,$filter:expr,$query:expr) => {
        sqlx::query($sql)
            .bind($hub.as_bytes().as_slice())
            .bind($filter)
            .bind($filter)
            .bind($filter)
            .bind($filter)
            .bind(&$query.text)
            .bind(&$query.text)
            .bind(&$query.text)
    };
}

impl Database {
    pub async fn hub_dispatch_page(
        &self,
        actor: PublicKey,
        query: HubDispatchQuery,
    ) -> Result<HubDispatchPage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    let state = stock::read::state_on(tx, hub.id()).await?;
                    let _ = super::super::treatment::capacity::available_on(tx, hub.id()).await?;
                    if query
                        .expected_revision
                        .is_some_and(|revision| revision != state.revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|revision| revision != catalog.revision())
                    {
                        return Err(conflict());
                    }
                    let filter = match query.filter {
                        HubDispatchFilter::All => 0_i64,
                        HubDispatchFilter::Draft => 1,
                        HubDispatchFilter::Issued => 2,
                        HubDispatchFilter::Unused => 3,
                    };
                    let count = dispatch_filter!(
                        dispatch_rows!("SELECT count(*) AS n", ""),
                        hub.id(),
                        filter,
                        query
                    )
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
                    let total = u32::try_from(count.try_get::<i64, _>("n").map_err(|_| corrupt())?)
                        .map_err(|_| corrupt())?;
                    if total > 1_024 {
                        return Err(corrupt());
                    }
                    if query.offset > total {
                        return Err(invalid_input());
                    }
                    let ids = dispatch_filter!(
                        dispatch_rows!(
                            "SELECT substr(c.preparation_id,1,17) AS preparation_id",
                            " ORDER BY o.stock_revision DESC LIMIT ? OFFSET ?"
                        ),
                        hub.id(),
                        filter,
                        query
                    )
                    .bind(i64::from(query.limit))
                    .bind(i64::from(query.offset))
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
                    let mut rows = Vec::with_capacity(ids.len());
                    for id in ids {
                        let record =
                            record_on(tx, hub.id(), record_id(&id, "preparation_id")?).await?;
                        let material = record
                            .draft
                            .as_ref()
                            .or_else(|| record.versions.last().map(|version| &version.material))
                            .ok_or_else(corrupt)?;
                        let status = if record.draft.is_some() {
                            HubDispatchState::Draft
                        } else {
                            match record.versions.last().ok_or_else(corrupt)?.status {
                                IdentificationStatus::Issued => HubDispatchState::Issued,
                                IdentificationStatus::Unused => HubDispatchState::Unused,
                                IdentificationStatus::Superseded => return Err(corrupt()),
                            }
                        };
                        rows.push(HubDispatchRow {
                            id: record.id,
                            revision: record.revision,
                            state: status,
                            recipient: material.recipient.clone(),
                            whole_packs: material.total_packs().map_err(|_| corrupt())?,
                            line_count: u32::try_from(material.lines.len())
                                .map_err(|_| corrupt())?,
                            version_count: u32::try_from(record.versions.len())
                                .map_err(|_| corrupt())?,
                            identification: record.versions.last().map(|version| version.id),
                            updated: record.updated,
                        });
                    }
                    Ok(HubDispatchPage {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        offset: query.offset,
                        total,
                        more: (u64::from(query.offset) + rows.len() as u64) < u64::from(total),
                        rows,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
