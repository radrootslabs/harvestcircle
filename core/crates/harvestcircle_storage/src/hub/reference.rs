//! Append-only reference versions and committed original-operation receipts.

use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::reference::{
    HUB_REFERENCE_CAPACITY, HUB_REFERENCE_HISTORY_CAPACITY, HUB_REFERENCE_PAGE_ROWS, HubLabel,
    HubRecordId, HubReferenceError, LocationPurpose, NominalGrams, ReferenceCatalog,
    ReferenceChange, ReferenceCommand, ReferenceRecord, ReferenceValue, SampleHandlingBasis,
};
use harvestcircle_domain::hub::{HubId, HubOwnership, HubRevision};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};
use radroots_service_sqlite::{
    MigrationChecksum, SchemaObject, SchemaObjectKind, ServiceSqliteTransaction,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};

pub(crate) const MIGRATION_SQL: &str = include_str!("reference/schema-v5.sql");
// Pinned before publication using the provider's actual frozen checksum/catalog APIs.
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    130, 15, 114, 61, 144, 69, 137, 77, 249, 74, 62, 157, 23, 90, 254, 135, 97, 72, 219, 95, 209,
    99, 131, 86, 134, 175, 153, 45, 114, 25, 36, 53,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    49, 8, 67, 209, 75, 74, 3, 9, 79, 78, 217, 168, 242, 220, 205, 148, 3, 208, 173, 238, 16, 97,
    201, 247, 221, 247, 155, 198, 232, 57, 2, 38,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    [
        (
            SchemaObjectKind::Table,
            "hub_reference_versions",
            "hub_reference_versions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_reference_no_update",
            "hub_reference_versions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_reference_no_delete",
            "hub_reference_versions",
        ),
        (
            SchemaObjectKind::Table,
            "hub_handling_context",
            "hub_handling_context",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_handling_no_update",
            "hub_handling_context",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_handling_no_delete",
            "hub_handling_context",
        ),
    ]
    .into_iter()
    .map(|(kind, name, table)| {
        let sql = statements
            .next()
            .expect("frozen reference migration inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        SchemaObject::new(kind, name, table, sql, digest)
    })
    .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubReferenceReceipt {
    pub operation: DurableRequestId,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub change: ReferenceChange,
}

// Bound every variable-width field before SQLx hydration, including a damaged
// database created with checks disabled. Domain decoders revalidate the bytes.
macro_rules! reference_select {
    ($suffix:literal) => {
        concat!(
            "SELECT substr(record_id,1,17) AS record_id, record_revision, catalog_revision, ",
            "substr(kind,1,9) AS kind, substr(CAST(label AS BLOB),1,121) AS label, ",
            "substr(product_id,1,17) AS product_id, nominal_grams, ",
            "substr(location_purpose,1,10) AS location_purpose, ",
            "substr(actor_public_key,1,33) AS actor_public_key, ",
            "substr(operation_id,1,37) AS operation_id, substr(intent_hash,1,33) AS intent_hash, recorded_at_unix_s",
            $suffix,
        )
    };
}

impl Database {
    /// Trusted adapter boundary; the actor supplies a verified current subject.
    pub async fn hub_references(&self, actor: PublicKey) -> Result<ReferenceCatalog, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = authorize_on(transaction, actor).await?;
                    load_catalog_on(transaction, hub.id()).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn apply_hub_reference(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ReferenceCommand,
        now: UnixTimestamp,
    ) -> Result<HubReferenceReceipt, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move { apply_on(transaction, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }

    /// An absent receipt is unresolved after restore; this read never replays.
    pub async fn hub_reference_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubReferenceReceipt>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = authorize_on(transaction, actor).await?;
                    Ok(operation_on(transaction, hub.id(), &operation)
                        .await?
                        .map(|(receipt, _)| receipt))
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_reference_history(
        &self,
        actor: PublicKey,
        id: HubRecordId,
        after: u64,
        limit: u32,
    ) -> Result<Vec<HubReferenceReceipt>, SafeError> {
        if !(1..=u32::try_from(HUB_REFERENCE_PAGE_ROWS).expect("finite page")).contains(&limit)
            || after > HubRevision::MAXIMUM
        {
            return Err(reference_error(HubReferenceError::InvalidQuantity));
        }
        self.host().transaction(|transaction| Box::pin(async move {
            let hub = authorize_on(transaction, actor).await?;
            let query = reference_select!(" FROM hub_reference_versions WHERE hub_id=? AND record_id=? AND record_revision>? ORDER BY record_revision LIMIT ?");
            let rows = sqlx::query(query).bind(hub.id().as_bytes().as_slice()).bind(id.as_bytes().as_slice())
                .bind(i64::try_from(after).map_err(|_| corrupt_storage())?).bind(i64::from(limit))
                .fetch_all(&mut *transaction).await.map_err(|_| storage_unavailable())?;
            let mut receipts = Vec::with_capacity(rows.len());
            for row in rows { receipts.push(receipt_on(transaction, hub.id(), &row).await?.0); }
            Ok(receipts)
        })).await.map_err(map_transaction_error)
    }
}

async fn apply_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ReferenceCommand,
    now: UnixTimestamp,
) -> Result<HubReferenceReceipt, SafeError> {
    let hub = authorize_on(transaction, actor).await?;
    let fingerprint = command.fingerprint(hub.id(), actor);
    if let Some((receipt, original_hash)) = operation_on(transaction, hub.id(), &operation).await? {
        if original_hash != fingerprint || receipt.actor != actor {
            return Err(reference_error(HubReferenceError::StaleRevision));
        }
        return Ok(receipt);
    }
    let mut catalog = load_catalog_on(transaction, hub.id()).await?;
    if catalog.revision().value()
        > u64::try_from(HUB_REFERENCE_HISTORY_CAPACITY).expect("finite history")
    {
        return Err(reference_error(HubReferenceError::Capacity));
    }
    let change = command.apply(&mut catalog).map_err(reference_error)?;
    let (kind, product, nominal, purpose) = match &change.current.value {
        ReferenceValue::Product { .. } => ("product", None, None, None),
        ReferenceValue::Party { .. } => ("party", None, None, None),
        ReferenceValue::Pack {
            product,
            nominal_grams,
            ..
        } => (
            "pack",
            Some(*product),
            nominal_grams.map(NominalGrams::value),
            None,
        ),
        ReferenceValue::Location { purpose, .. } => (
            "location",
            None,
            None,
            Some(match purpose {
                LocationPurpose::Receiving => "receiving",
                LocationPurpose::Storage => "storage",
                LocationPurpose::Other => "other",
            }),
        ),
    };
    sqlx::query("INSERT INTO hub_reference_versions (record_id, record_revision, catalog_revision, hub_id, kind, label, product_id, nominal_grams, location_purpose, actor_public_key, operation_id, intent_hash, recorded_at_unix_s) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(change.current.id.as_bytes().as_slice())
        .bind(i64::try_from(change.current.revision.value()).map_err(|_| corrupt_storage())?)
        .bind(i64::try_from(change.catalog_revision.value()).map_err(|_| corrupt_storage())?)
        .bind(hub.id().as_bytes().as_slice()).bind(kind).bind(change.current.value.label().as_str())
        .bind(product.map(|id| id.as_bytes().to_vec())).bind(nominal.map(i64::from)).bind(purpose)
        .bind(actor.as_bytes().as_slice()).bind(operation.as_str()).bind(fingerprint.as_slice()).bind(now.as_seconds())
        .execute(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    Ok(HubReferenceReceipt {
        operation,
        actor,
        recorded_at: now,
        change,
    })
}

pub(super) async fn record_at_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
    revision: HubRevision,
) -> Result<ReferenceRecord, SafeError> {
    let query = reference_select!(
        " FROM hub_reference_versions WHERE hub_id=? AND record_id=? AND record_revision=? LIMIT 1"
    );
    let row = sqlx::query(query)
        .bind(hub.as_bytes().as_slice())
        .bind(id.as_bytes().as_slice())
        .bind(i64::try_from(revision.value()).map_err(|_| corrupt_storage())?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?
        .ok_or_else(corrupt_storage)?;
    decode_record(&row)
}

pub(super) async fn authorize_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
) -> Result<HubOwnership, SafeError> {
    let (hub, _) = super::load_on(transaction)
        .await?
        .ok_or_else(hub_permission_denied)?;
    hub.authorize(Some(actor))
        .map_err(|_| hub_permission_denied())?;
    Ok(hub)
}

pub(super) async fn validate_basis_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<SampleHandlingBasis, SafeError> {
    let row = sqlx::query("SELECT substr(basis_id,1,65) AS basis_id, basis_version, substr(site_timezone,1,65) AS site_timezone FROM hub_handling_context WHERE hub_id=? LIMIT 1")
        .bind(hub.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let id: String = row.try_get("basis_id").map_err(|_| corrupt_storage())?;
    let version: i64 = row
        .try_get("basis_version")
        .map_err(|_| corrupt_storage())?;
    let timezone: String = row
        .try_get("site_timezone")
        .map_err(|_| corrupt_storage())?;
    SampleHandlingBasis::from_stored(
        &id,
        u32::try_from(version).map_err(|_| corrupt_storage())?,
        &timezone,
    )
    .map_err(|_| corrupt_storage())
}

pub(super) async fn load_catalog_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<ReferenceCatalog, SafeError> {
    validate_basis_on(transaction, hub).await?;
    let clock = sqlx::query("SELECT count(*) AS n, COALESCE(max(catalog_revision),1) AS revision FROM hub_reference_versions WHERE hub_id=?")
        .bind(hub.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    let count: i64 = clock.try_get("n").map_err(|_| corrupt_storage())?;
    let revision: i64 = clock.try_get("revision").map_err(|_| corrupt_storage())?;
    if !(0..=i64::try_from(HUB_REFERENCE_HISTORY_CAPACITY).expect("finite capacity"))
        .contains(&count)
        || revision != count + 1
    {
        return Err(corrupt_storage());
    }
    let gaps: i64 = sqlx::query_scalar("SELECT count(*) FROM (SELECT record_id FROM hub_reference_versions WHERE hub_id=? GROUP BY record_id HAVING count(*)<>max(record_revision))")
        .bind(hub.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_| storage_unavailable())?;
    if gaps != 0 {
        return Err(corrupt_storage());
    }
    let query = reference_select!(
        " FROM hub_reference_versions AS v WHERE hub_id=? AND NOT EXISTS (SELECT 1 FROM hub_reference_versions AS newer WHERE newer.record_id=v.record_id AND newer.record_revision>v.record_revision) ORDER BY record_id LIMIT ?"
    );
    let rows = sqlx::query(query)
        .bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(HUB_REFERENCE_CAPACITY + 1).expect("finite capacity"))
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    let records = rows
        .iter()
        .map(decode_record)
        .collect::<Result<Vec<_>, _>>()?;
    ReferenceCatalog::restore(revision_from_sql(revision)?, records).map_err(|_| corrupt_storage())
}

async fn operation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
) -> Result<Option<(HubReferenceReceipt, [u8; 32])>, SafeError> {
    let query =
        reference_select!(" FROM hub_reference_versions WHERE hub_id=? AND operation_id=? LIMIT 1");
    let row = sqlx::query(query)
        .bind(hub.as_bytes().as_slice())
        .bind(operation.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    match row {
        Some(row) => Ok(Some(receipt_on(transaction, hub, &row).await?)),
        None => Ok(None),
    }
}

async fn receipt_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    row: &SqliteRow,
) -> Result<(HubReferenceReceipt, [u8; 32]), SafeError> {
    let current = decode_record(row)?;
    let catalog_revision = revision_from_sql(
        row.try_get("catalog_revision")
            .map_err(|_| corrupt_storage())?,
    )?;
    if current.revision.value() >= catalog_revision.value() {
        return Err(corrupt_storage());
    }
    let previous = if current.revision == HubRevision::INITIAL {
        None
    } else {
        let query = reference_select!(
            " FROM hub_reference_versions WHERE hub_id=? AND record_id=? AND record_revision=? LIMIT 1"
        );
        let previous = sqlx::query(query)
            .bind(hub.as_bytes().as_slice())
            .bind(current.id.as_bytes().as_slice())
            .bind(i64::try_from(current.revision.value() - 1).map_err(|_| corrupt_storage())?)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?
            .ok_or_else(corrupt_storage)?;
        Some(decode_record(&previous)?)
    };
    let operation: String = row.try_get("operation_id").map_err(|_| corrupt_storage())?;
    let actor: Vec<u8> = row
        .try_get("actor_public_key")
        .map_err(|_| corrupt_storage())?;
    let hash: Vec<u8> = row.try_get("intent_hash").map_err(|_| corrupt_storage())?;
    let time: i64 = row
        .try_get("recorded_at_unix_s")
        .map_err(|_| corrupt_storage())?;
    Ok((
        HubReferenceReceipt {
            operation: DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?,
            actor: PublicKey::from_bytes(actor.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            recorded_at: UnixTimestamp::from_seconds(time).ok_or_else(corrupt_storage)?,
            change: ReferenceChange {
                catalog_revision,
                previous,
                current,
            },
        },
        hash.try_into().map_err(|_| corrupt_storage())?,
    ))
}

fn decode_record(row: &SqliteRow) -> Result<ReferenceRecord, SafeError> {
    let bytes: Vec<u8> = row.try_get("record_id").map_err(|_| corrupt_storage())?;
    let id = HubRecordId::from_bytes(bytes.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let revision = revision_from_sql(
        row.try_get("record_revision")
            .map_err(|_| corrupt_storage())?,
    )?;
    let kind: String = row.try_get("kind").map_err(|_| corrupt_storage())?;
    let bytes: Vec<u8> = row.try_get("label").map_err(|_| corrupt_storage())?;
    let label = HubLabel::new(String::from_utf8(bytes).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let product: Option<Vec<u8>> = row.try_get("product_id").map_err(|_| corrupt_storage())?;
    let nominal: Option<i64> = row
        .try_get("nominal_grams")
        .map_err(|_| corrupt_storage())?;
    let purpose: Option<String> = row
        .try_get("location_purpose")
        .map_err(|_| corrupt_storage())?;
    let value = match (kind.as_str(), product, nominal, purpose.as_deref()) {
        ("product", None, None, None) => ReferenceValue::Product { label },
        ("party", None, None, None) => ReferenceValue::Party { label },
        ("pack", Some(product), nominal, None) => ReferenceValue::Pack {
            product: HubRecordId::from_bytes(product.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            label,
            nominal_grams: nominal
                .map(|n| {
                    NominalGrams::from_value(u32::try_from(n).map_err(|_| corrupt_storage())?)
                        .map_err(|_| corrupt_storage())
                })
                .transpose()?,
        },
        ("location", None, None, Some(purpose)) => ReferenceValue::Location {
            label,
            purpose: match purpose {
                "receiving" => LocationPurpose::Receiving,
                "storage" => LocationPurpose::Storage,
                "other" => LocationPurpose::Other,
                _ => return Err(corrupt_storage()),
            },
        },
        _ => return Err(corrupt_storage()),
    };
    Ok(ReferenceRecord {
        id,
        revision,
        value,
    })
}

fn revision_from_sql(value: i64) -> Result<HubRevision, SafeError> {
    HubRevision::from_value(u64::try_from(value).map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())
}

const fn reference_error(error: HubReferenceError) -> SafeError {
    let message = match error {
        HubReferenceError::Capacity => {
            "The demo reference capacity is full. Preserve this run before starting another."
        }
        HubReferenceError::StaleRevision
        | HubReferenceError::DuplicateId
        | HubReferenceError::NoChange => {
            "The reference changed. Review current records before submitting another operation."
        }
        HubReferenceError::ImmutablePack | HubReferenceError::ImmutableLocationPurpose => {
            "Create another reference identity for a changed pack definition or location purpose."
        }
        _ => "The reference input is invalid or refers to an unavailable record.",
    };
    let code = match error {
        HubReferenceError::Capacity => SafeErrorCode::HubCapacity,
        HubReferenceError::StaleRevision
        | HubReferenceError::DuplicateId
        | HubReferenceError::NoChange => SafeErrorCode::HubConflict,
        _ => SafeErrorCode::HubInvalidInput,
    };
    SafeError::new(code, SafeMessage::new(message))
}

const fn hub_permission_denied() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubPermissionDenied,
        SafeMessage::new("Activate the current hub owner's local identity to access hub records."),
    )
}

#[cfg(test)]
mod tests;
