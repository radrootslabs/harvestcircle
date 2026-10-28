use super::*;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::{Database, hub::reference};
use harvestcircle_domain::hub::HubId;
use harvestcircle_domain::hub::reference::{
    ReferenceCatalog, SourceAttribution, SourceCertainty, SourceCode, SourceProvenance,
    SourceReference, SourceReferenceKind, WholePacks,
};
use harvestcircle_domain::hub::stock::{HUB_STOCK_EVENT_CAPACITY, Occurrence, ReceiptCommand};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::{Row, sqlite::SqliteRow};
use std::collections::BTreeMap;

pub(in crate::hub) struct StockState {
    pub revision: HubRevision,
    pub event_count: u32,
}

pub(in crate::hub) struct OperationRecord {
    pub id: DurableRequestId,
    pub kind: String,
    pub actor: PublicKey,
    pub hash: [u8; 32],
    pub catalog_revision: HubRevision,
    pub revision: HubRevision,
    pub result: HubRecordId,
    pub recorded_at: UnixTimestamp,
}

impl Database {
    pub async fn hub_receipt_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubStockReceipt>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = reference::authorize_on(transaction, actor).await?;
                    reference::validate_basis_on(transaction, hub.id()).await?;
                    match operation_on(transaction, hub.id(), &operation).await? {
                        Some(operation) => {
                            Ok(Some(receipt_on(transaction, hub.id(), operation).await?))
                        }
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_stock_page(
        &self,
        actor: PublicKey,
        query: HubStockQuery,
    ) -> Result<HubStockPage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = reference::authorize_on(transaction, actor).await?;
                    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
                    let state = state_on(transaction, hub.id()).await?;
                    if query.expected_revision.is_some_and(|r| r != state.revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|r| r != catalog.revision())
                    {
                        return Err(conflict());
                    }
                    page_on(transaction, hub.id(), catalog, state, query).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(in crate::hub) async fn state_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<StockState, SafeError> {
    let row = sqlx::query(
        "SELECT revision, event_count FROM hub_stock_state WHERE singleton=1 AND hub_id=? LIMIT 1",
    )
    .bind(hub.as_bytes().as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(corrupt_storage)?;
    let revision = revision(row.try_get("revision").map_err(|_| corrupt_storage())?)?;
    let event_count = u32::try_from(
        row.try_get::<i64, _>("event_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let operations = sqlx::query("SELECT count(*) AS n, COALESCE(max(stock_revision),1) AS latest, count(CASE WHEN kind NOT IN ('receipt','move','split','acceptance','hold_place','hold_release','work_replan','work_new','discrepancy_open','discrepancy_resolve') THEN 1 END) AS unknown FROM hub_stock_operations WHERE hub_id=?")
        .bind(hub.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let events = sqlx::query("SELECT count(*) AS n, COALESCE(max(sequence),0) AS latest, count(CASE WHEN kind NOT IN ('receipt','move','split','acceptance','hold_place','hold_release','work_replan','work_new','discrepancy_open','discrepancy_resolve') THEN 1 END) AS unknown FROM hub_stock_events")
        .fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let op_count: i64 = operations.try_get("n").map_err(|_| corrupt_storage())?;
    let op_latest: i64 = operations
        .try_get("latest")
        .map_err(|_| corrupt_storage())?;
    let ev_count: i64 = events.try_get("n").map_err(|_| corrupt_storage())?;
    let ev_latest: i64 = events.try_get("latest").map_err(|_| corrupt_storage())?;
    if !(0..=HUB_STOCK_EVENT_CAPACITY as i64).contains(&op_count)
        || !(0..=HUB_STOCK_EVENT_CAPACITY as i64).contains(&ev_count)
        || op_latest != op_count + 1
        || op_latest as u64 != revision.value()
        || ev_count != ev_latest
        || ev_count != i64::from(event_count)
        || operations
            .try_get::<i64, _>("unknown")
            .map_err(|_| corrupt_storage())?
            != 0
        || events
            .try_get::<i64, _>("unknown")
            .map_err(|_| corrupt_storage())?
            != 0
    {
        return Err(corrupt_storage());
    }
    let mismatch: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_events e LEFT JOIN hub_stock_operations o ON o.operation_id=e.operation_id WHERE o.operation_id IS NULL OR e.kind<>o.kind")
        .fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if mismatch != 0 {
        return Err(corrupt_storage());
    }
    let cardinality: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_operations o WHERE (SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=o.operation_id)<>1")
        .fetch_one(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if cardinality != 0 {
        return Err(corrupt_storage());
    }

    let counts = sqlx::query("SELECT (SELECT count(*) FROM hub_receipts) AS traces,(SELECT count(*) FROM hub_stock_groups) AS groups,(SELECT count(*) FROM hub_work_requirements) AS work,(SELECT count(*) FROM hub_discrepancies) AS discrepancies")
        .fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    for (field, maximum) in [
        ("traces", HUB_TRACE_CAPACITY),
        (
            "groups",
            harvestcircle_domain::hub::movement::HUB_GROUP_CAPACITY,
        ),
        (
            "work",
            harvestcircle_domain::hub::movement::HUB_WORK_CAPACITY,
        ),
        (
            "discrepancies",
            harvestcircle_domain::hub::movement::HUB_DISCREPANCY_CAPACITY,
        ),
    ] {
        let count: i64 = counts.try_get(field).map_err(|_| corrupt_storage())?;
        if count < 0 || count > maximum as i64 {
            return Err(corrupt_storage());
        }
    }
    Ok(StockState {
        revision,
        event_count,
    })
}

pub(in crate::hub) async fn operation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
) -> Result<Option<OperationRecord>, SafeError> {
    let row = sqlx::query("SELECT substr(operation_id,1,37) AS operation_id, substr(actor_public_key,1,33) AS actor, substr(kind,1,33) AS kind, substr(intent_hash,1,33) AS hash, catalog_revision, stock_revision, substr(result_id,1,17) AS result, recorded_at_unix_s FROM hub_stock_operations WHERE hub_id=? AND operation_id=? LIMIT 1")
        .bind(hub.as_bytes().as_slice()).bind(operation.as_str()).fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    row.map(|row| {
        let kind: String = row.try_get("kind").map_err(|_| corrupt_storage())?;
        if !known_kind(&kind) {
            return Err(corrupt_storage());
        }
        let actor: Vec<u8> = row.try_get("actor").map_err(|_| corrupt_storage())?;
        let hash: Vec<u8> = row.try_get("hash").map_err(|_| corrupt_storage())?;
        let stock_revision = revision(
            row.try_get("stock_revision")
                .map_err(|_| corrupt_storage())?,
        )?;
        if stock_revision.value() < 2 {
            return Err(corrupt_storage());
        }
        Ok(OperationRecord {
            kind,
            id: DurableRequestId::parse(
                row.try_get::<String, _>("operation_id")
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
            actor: PublicKey::from_bytes(actor.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            hash: hash.try_into().map_err(|_| corrupt_storage())?,
            catalog_revision: revision(
                row.try_get("catalog_revision")
                    .map_err(|_| corrupt_storage())?,
            )?,
            revision: stock_revision,
            result: record_id(&row, "result")?,
            recorded_at: timestamp(
                row.try_get("recorded_at_unix_s")
                    .map_err(|_| corrupt_storage())?,
            )?,
        })
    })
    .transpose()
}

pub(in crate::hub) async fn receipt_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubStockReceipt, SafeError> {
    if operation.kind != "receipt" {
        return Err(conflict());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(r.trace_id,1,17) AS trace_id, substr(r.root_group_id,1,17) AS group_id, ",
        "substr(r.variant_id,1,17) AS variant_id, r.variant_revision, substr(r.product_id,1,17) AS product_id, r.product_revision, ",
        "substr(r.supplier_id,1,17) AS supplier_id, r.supplier_revision, substr(r.producer_id,1,17) AS producer_id, r.producer_revision, ",
        "substr(r.receiving_id,1,17) AS receiving_id, r.receiving_revision, substr(r.planned_location_id,1,17) AS planned_location_id, r.planned_location_revision, ",
        "substr(r.source_id,1,17) AS source_id, substr(r.source_certainty,1,12) AS source_certainty, r.source_issuer_revision, ",
        "substr(s.issuer_id,1,17) AS issuer_id, substr(s.kind,1,20) AS source_kind, substr(CAST(s.source_code AS BLOB),1,129) AS source_code, ",
        "substr(CAST(s.provenance AS BLOB),1,513) AS provenance, r.original_packs, substr(r.occurrence_kind,1,9) AS occurrence_kind, ",
        "r.occurrence_start_unix_s, r.occurrence_end_unix_s, substr(CAST(r.observation AS BLOB),1,513) AS observation ",
        "FROM hub_receipts r LEFT JOIN hub_source_references s ON s.source_id=r.source_id ",
        "WHERE r.trace_id=? AND r.operation_id=? LIMIT 1"
    )).bind(operation.result.as_bytes().as_slice()).bind(operation.id.as_str())
        .fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let mut records = BTreeMap::new();
    let mut remember = |record: ReferenceRecord| -> Result<HubRecordId, SafeError> {
        let id = record.id;
        if let Some(previous) = records.insert(id, record.clone())
            && previous != record
        {
            return Err(corrupt_storage());
        }
        Ok(id)
    };
    let variant = reference::record_at_on(
        transaction,
        hub,
        record_id(&row, "variant_id")?,
        revision_field(&row, "variant_revision")?,
    )
    .await?;
    let product = reference::record_at_on(
        transaction,
        hub,
        record_id(&row, "product_id")?,
        revision_field(&row, "product_revision")?,
    )
    .await?;
    remember(product)?;
    let variant = remember(variant)?;
    let supplier = optional_record_on(transaction, hub, &row, "supplier_id", "supplier_revision")
        .await?
        .map(&mut remember)
        .transpose()?;
    let producer = optional_record_on(transaction, hub, &row, "producer_id", "producer_revision")
        .await?
        .map(&mut remember)
        .transpose()?;
    let receiving = remember(
        reference::record_at_on(
            transaction,
            hub,
            record_id(&row, "receiving_id")?,
            revision_field(&row, "receiving_revision")?,
        )
        .await?,
    )?;
    let planned_location = optional_record_on(
        transaction,
        hub,
        &row,
        "planned_location_id",
        "planned_location_revision",
    )
    .await?
    .map(&mut remember)
    .transpose()?;
    let source = source_from_row(&row)?;
    let source_issuer = optional_record_on(
        transaction,
        hub,
        &row,
        "issuer_id",
        "source_issuer_revision",
    )
    .await?;
    if let Some(issuer) = source_issuer {
        remember(issuer)?;
    }
    let catalog =
        ReferenceCatalog::restore(operation.catalog_revision, records.into_values().collect())
            .map_err(|_| corrupt_storage())?;
    let command = ReceiptCommand {
        expected_stock_revision: HubRevision::from_value(operation.revision.value() - 1)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: operation.catalog_revision,
        variant,
        supplier,
        producer,
        source,
        packs: whole_packs(
            row.try_get("original_packs")
                .map_err(|_| corrupt_storage())?,
        )?,
        receiving,
        planned_location,
        occurred: occurrence(&row)?,
        observation: SourceProvenance::new(text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    let trace = record_id(&row, "trace_id")?;
    let group = record_id(&row, "group_id")?;
    let ids = receipt_ids(&operation.id).map_err(|_| corrupt_storage())?;
    if ids.trace != trace || ids.group != group {
        return Err(corrupt_storage());
    }
    let original = command
        .plan(
            &catalog,
            command.expected_stock_revision,
            trace,
            group,
            operation.actor,
            operation.recorded_at,
        )
        .map_err(|_| corrupt_storage())?;
    if trace != operation.result
        || command.fingerprint(hub, operation.actor, trace, group) != operation.hash
    {
        return Err(corrupt_storage());
    }
    Ok(HubStockReceipt {
        operation: operation.id,
        original,
    })
}

pub(in crate::hub) async fn optional_record_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    row: &SqliteRow,
    id: &str,
    rev: &str,
) -> Result<Option<ReferenceRecord>, SafeError> {
    let id = optional_id(row, id)?;
    let rev: Option<i64> = row.try_get(rev).map_err(|_| corrupt_storage())?;
    match (id, rev) {
        (None, None) => Ok(None),
        (Some(id), Some(rev)) => Ok(Some(
            reference::record_at_on(transaction, hub, id, revision(rev)?).await?,
        )),
        _ => Err(corrupt_storage()),
    }
}

pub(in crate::hub) fn source_from_row(row: &SqliteRow) -> Result<SourceAttribution, SafeError> {
    let id = optional_id(row, "source_id")?;
    let certainty: Option<String> = row
        .try_get("source_certainty")
        .map_err(|_| corrupt_storage())?;
    match (id, certainty.as_deref()) {
        (None, None) => Ok(SourceAttribution::Unknown),
        (Some(id), Some(certainty)) => {
            let certainty = match certainty {
                "possible" => SourceCertainty::ReportedPossible,
                "established" => SourceCertainty::Established,
                _ => return Err(corrupt_storage()),
            };
            let kind: String = row.try_get("source_kind").map_err(|_| corrupt_storage())?;
            let kind = match kind.as_str() {
                "producer_batch" => SourceReferenceKind::ProducerBatch,
                "supplier_reference" => SourceReferenceKind::SupplierReference,
                "other" => SourceReferenceKind::Other,
                _ => return Err(corrupt_storage()),
            };
            Ok(SourceAttribution::Referenced {
                source: SourceReference {
                    id,
                    issuer: optional_id(row, "issuer_id")?,
                    kind,
                    code: SourceCode::new(text(row, "source_code")?)
                        .map_err(|_| corrupt_storage())?,
                    provenance: SourceProvenance::new(text(row, "provenance")?)
                        .map_err(|_| corrupt_storage())?,
                },
                certainty,
            })
        }
        _ => Err(corrupt_storage()),
    }
}

pub(in crate::hub) fn occurrence(row: &SqliteRow) -> Result<Occurrence, SafeError> {
    let kind: String = row
        .try_get("occurrence_kind")
        .map_err(|_| corrupt_storage())?;
    let start: Option<i64> = row
        .try_get("occurrence_start_unix_s")
        .map_err(|_| corrupt_storage())?;
    let end: Option<i64> = row
        .try_get("occurrence_end_unix_s")
        .map_err(|_| corrupt_storage())?;
    match (kind.as_str(), start, end) {
        ("unknown", None, None) => Ok(Occurrence::Unknown),
        ("known", Some(start), Some(end)) if start == end => {
            Ok(Occurrence::Known(timestamp(start)?))
        }
        ("interval", Some(start), Some(end)) if start <= end => Ok(Occurrence::Interval {
            earliest: timestamp(start)?,
            latest: timestamp(end)?,
        }),
        _ => Err(corrupt_storage()),
    }
}

pub(in crate::hub) fn revision(value: i64) -> Result<HubRevision, SafeError> {
    HubRevision::from_value(u64::try_from(value).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())
}
pub(in crate::hub) fn revision_field(
    row: &SqliteRow,
    field: &str,
) -> Result<HubRevision, SafeError> {
    revision(row.try_get(field).map_err(|_| corrupt_storage())?)
}
pub(in crate::hub) fn timestamp(value: i64) -> Result<UnixTimestamp, SafeError> {
    UnixTimestamp::from_seconds(value).ok_or_else(corrupt_storage)
}
pub(in crate::hub) fn whole_packs(value: i64) -> Result<WholePacks, SafeError> {
    WholePacks::from_value(u32::try_from(value).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())
}
pub(in crate::hub) fn record_id(row: &SqliteRow, field: &str) -> Result<HubRecordId, SafeError> {
    optional_id(row, field)?.ok_or_else(corrupt_storage)
}
pub(in crate::hub) fn optional_id(
    row: &SqliteRow,
    field: &str,
) -> Result<Option<HubRecordId>, SafeError> {
    let bytes: Option<Vec<u8>> = row.try_get(field).map_err(|_| corrupt_storage())?;
    bytes
        .map(|bytes| {
            HubRecordId::from_bytes(bytes.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())
        })
        .transpose()
}
pub(in crate::hub) fn text(row: &SqliteRow, field: &str) -> Result<String, SafeError> {
    String::from_utf8(row.try_get(field).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())
}

macro_rules! stock_rows {
    ($prefix:literal, $suffix:literal) => { concat!($prefix,
        " FROM hub_stock_positions p JOIN hub_stock_groups g ON g.group_id=p.group_id ",
        "JOIN hub_receipts r ON r.trace_id=g.trace_id JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "JOIN hub_reference_versions product ON product.record_id=r.product_id AND product.record_revision=r.product_revision ",
        "JOIN hub_reference_versions pack ON pack.record_id=r.variant_id AND pack.record_revision=r.variant_revision ",
        "JOIN hub_reference_versions current_product ON current_product.record_id=r.product_id AND NOT EXISTS (SELECT 1 FROM hub_reference_versions newer WHERE newer.record_id=current_product.record_id AND newer.record_revision>current_product.record_revision) ",
        "LEFT JOIN hub_source_references source ON source.source_id=r.source_id ",
        "WHERE o.hub_id=? AND p.packs>0 AND (? IS NULL OR p.location_id=?) AND (? IS NULL OR r.trace_id=?) ",
        "AND (?='' OR product.label LIKE ? ESCAPE '\\' OR pack.label LIKE ? ESCAPE '\\' OR current_product.label LIKE ? ESCAPE '\\' OR source.source_code LIKE ? ESCAPE '\\')",
        $suffix) };
}
macro_rules! filtered_query {
    ($sql:expr, $hub:expr, $query:expr, $pattern:expr) => {
        sqlx::query($sql)
            .bind($hub.as_bytes().as_slice())
            .bind($query.location.as_ref().map(|id| id.as_bytes().as_slice()))
            .bind($query.location.as_ref().map(|id| id.as_bytes().as_slice()))
            .bind($query.trace.as_ref().map(|id| id.as_bytes().as_slice()))
            .bind($query.trace.as_ref().map(|id| id.as_bytes().as_slice()))
            .bind(&$query.text)
            .bind(&$pattern)
            .bind(&$pattern)
            .bind(&$pattern)
            .bind(&$pattern)
    };
}
async fn page_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    catalog: ReferenceCatalog,
    state: StockState,
    query: HubStockQuery,
) -> Result<HubStockPage, SafeError> {
    let pattern = format!(
        "%{}%",
        query
            .text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let count = filtered_query!(stock_rows!("SELECT count(*) AS n", ""), hub, query, pattern)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    let total = u32::try_from(
        count
            .try_get::<i64, _>("n")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if total as usize > harvestcircle_domain::hub::movement::HUB_GROUP_CAPACITY {
        return Err(corrupt_storage());
    }
    if query.offset > total {
        return Err(invalid_input());
    }
    let selected = filtered_query!(stock_rows!(
        "SELECT substr(p.group_id,1,17) AS group_id, substr(g.trace_id,1,17) AS trace_id, substr(p.location_id,1,17) AS location_id, p.location_revision, p.packs, p.revision, substr(r.operation_id,1,37) AS operation_id",
        " ORDER BY p.group_id,p.location_id LIMIT ? OFFSET ?"),hub,query,pattern)
        .bind(i64::from(query.limit)).bind(i64::from(query.offset))
        .fetch_all(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let mut rows = Vec::with_capacity(selected.len());
    for row in selected {
        let operation = DurableRequestId::parse(
            row.try_get::<String, _>("operation_id")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let operation = operation_on(transaction, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = receipt_on(transaction, hub, operation).await?.original;
        let current_group = crate::hub::handling::read::group_on(
            transaction,
            hub,
            record_id(&row, "group_id")?,
            &catalog,
        )
        .await?;
        if current_group.location != record_id(&row, "location_id")?
            || current_group.packs
                != whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?
        {
            return Err(corrupt_storage());
        }
        let trace = record_id(&row, "trace_id")?;
        if trace != original.trace.id {
            return Err(corrupt_storage());
        }
        let location = reference::record_at_on(
            transaction,
            hub,
            record_id(&row, "location_id")?,
            revision_field(&row, "location_revision")?,
        )
        .await?;
        if location.value.kind() != harvestcircle_domain::hub::reference::ReferenceKind::Location {
            return Err(corrupt_storage());
        }
        let work = crate::hub::handling::read::work_on(transaction, hub, trace).await?;
        let put_away = work.quantity;
        let historical_target = work.target;
        let work_target = historical_target
            .map(|target| {
                catalog
                    .require_kind(
                        target.id,
                        harvestcircle_domain::hub::reference::ReferenceKind::Location,
                    )
                    .cloned()
                    .map_err(|_| corrupt_storage())
            })
            .transpose()?;
        let location = catalog
            .require_kind(
                location.id,
                harvestcircle_domain::hub::reference::ReferenceKind::Location,
            )
            .cloned()
            .map_err(|_| corrupt_storage())?;
        let product = catalog
            .require_kind(
                original.trace.product.id,
                harvestcircle_domain::hub::reference::ReferenceKind::Product,
            )
            .cloned()
            .map_err(|_| corrupt_storage())?;
        rows.push(HubStockRow {
            product,
            position: StockPosition {
                group: record_id(&row, "group_id")?,
                trace,
                variant: original.trace.variant.id,
                location: location.id,
                packs: whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?,
                revision: revision_field(&row, "revision")?,
            },
            location,
            trace: original.trace,
            put_away,
            work_revision: work.revision,
            work_target,
        });
    }
    let more = query.offset + u32::try_from(rows.len()).map_err(|_| corrupt_storage())? < total;
    Ok(HubStockPage {
        handling_basis: catalog.handling_basis(),
        revision: state.revision,
        catalog_revision: catalog.revision(),
        offset: query.offset,
        total,
        more,
        rows,
    })
}

pub(in crate::hub) fn known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "receipt"
            | "move"
            | "split"
            | "acceptance"
            | "hold_place"
            | "hold_release"
            | "work_replan"
            | "work_new"
            | "discrepancy_open"
            | "discrepancy_resolve"
    )
}
