//! Interpretation mode comes from native epoch and operation records, never JSON.
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::stock,
};
use harvestcircle_domain::{SafeError, hub::HubId};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Epoch {
    pub legacy_stock: u64,
    pub legacy_catalog: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Mode {
    Legacy(Epoch),
    Qualified {
        epoch: Epoch,
        capture: [u8; 32],
    },
    CountQualified {
        epoch: Epoch,
        count: crate::hub::corrections::count_mode::Mode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Capture {
    Legacy,
    Physical20,
    Count21,
}
impl Capture {
    pub fn departure_result(self, base: [u8; 32]) -> [u8; 32] {
        self.count_result(
            b"harvestcircle.hub.actual.count_aware.departure.result.v1\0",
            base,
        )
    }
    pub fn link_result(self, base: [u8; 32]) -> [u8; 32] {
        self.count_result(
            b"harvestcircle.hub.actual.count_aware.link.result.v1\0",
            base,
        )
    }
    fn count_result(self, purpose: &[u8], base: [u8; 32]) -> [u8; 32] {
        if self != Self::Count21 {
            return base;
        }
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(purpose);
        hash.update(base);
        hash.finalize().into()
    }
}

impl Mode {
    pub const fn qualified(self) -> bool {
        !matches!(self, Self::Legacy(_))
    }
    pub const fn capture_kind(self) -> Capture {
        match self {
            Self::Legacy(_) => Capture::Legacy,
            Self::Qualified { .. } => Capture::Physical20,
            Self::CountQualified { .. } => Capture::Count21,
        }
    }
    pub const fn capture_hash(self) -> Option<[u8; 32]> {
        match self {
            Self::Legacy(_) => None,
            Self::Qualified { capture, .. } => Some(capture),
            Self::CountQualified { count, .. } => Some(count.capture),
        }
    }
}

pub(super) async fn current_capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Capture, SafeError> {
    epoch_on(tx, hub).await?;
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 21 {
        crate::hub::corrections::count_mode::epoch_on(tx, hub).await?;
        Ok(Capture::Count21)
    } else {
        Ok(Capture::Physical20)
    }
}

pub(super) async fn epoch_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Epoch, SafeError> {
    let row = sqlx::query(concat!(
        "SELECT e.epoch,e.legacy_stock_revision,e.legacy_catalog_revision,s.revision,",
        "(SELECT count(*) FROM hub_actual_interpretation_epochs) AS total,",
        "COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=e.hub_id),1) AS catalog,",
        "COALESCE((SELECT max(o.catalog_revision) FROM hub_stock_operations o WHERE o.hub_id=e.hub_id AND o.stock_revision<=e.legacy_stock_revision),1) AS dependency ",
        "FROM hub_actual_interpretation_epochs e JOIN hub_stock_state s ON s.hub_id=e.hub_id ",
        "JOIN hub_context h ON h.hub_id=e.hub_id WHERE e.hub_id=? LIMIT 1"
    ))
    .bind(hub.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let epoch = Epoch {
        legacy_stock: number("legacy_stock_revision")?,
        legacy_catalog: number("legacy_catalog_revision")?,
    };
    if number("epoch")? != 1
        || number("total")? != 1
        || epoch.legacy_stock == 0
        || epoch.legacy_stock > number("revision")?
        || epoch.legacy_catalog == 0
        || epoch.legacy_catalog > number("catalog")?
        || number("dependency")? > epoch.legacy_catalog
    {
        return Err(corrupt_storage());
    }
    Ok(epoch)
}

pub(super) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
) -> Result<Mode, SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    let count = if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 21 {
        crate::hub::corrections::count_mode::for_actual_on(tx, hub, op).await?
    } else {
        None
    };
    let row = sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,",
        "stock_revision,catalog_revision,substr(capture_hash,1,33) AS capture_hash ",
        "FROM hub_actual_interpretation_markers WHERE operation_id=? LIMIT 1"
    ))
    .bind(op.id.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if op.kind != "actual_review" {
        return Err(corrupt_storage());
    }
    if op.revision.value() <= epoch.legacy_stock {
        return if row.is_none() && count.is_none() {
            Ok(Mode::Legacy(epoch))
        } else {
            Err(corrupt_storage())
        };
    }
    let row = row.ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let owner: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
    let capture: Vec<u8> = row.try_get("capture_hash").map_err(|_| corrupt_storage())?;
    if owner.as_slice() != hub.as_bytes()
        || number("epoch")? != 1
        || number("legacy_stock_revision")? != epoch.legacy_stock
        || number("legacy_catalog_revision")? != epoch.legacy_catalog
        || number("stock_revision")? != op.revision.value()
        || number("catalog_revision")? != op.catalog_revision.value()
        || op.catalog_revision.value() < epoch.legacy_catalog
    {
        return Err(corrupt_storage());
    }
    let capture: [u8; 32] = capture.try_into().map_err(|_| corrupt_storage())?;
    match count {
        Some(count) if count.capture == capture => Ok(Mode::CountQualified { epoch, count }),
        Some(_) => Err(corrupt_storage()),
        None => Ok(Mode::Qualified { epoch, capture }),
    }
}

pub(super) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &stock::read::OperationRecord,
) -> Result<(), SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    if op.kind != "actual_review" || op.revision.value() <= epoch.legacy_stock {
        return Err(corrupt_storage());
    }
    let changed = sqlx::query(concat!(
        "INSERT INTO hub_actual_interpretation_markers ",
        "(operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,capture_hash) ",
        "SELECT ?,?,1,?,?,?,?,s.capture_hash FROM hub_submissions s WHERE s.operation_id=?"
    ))
    .bind(op.id.as_str())
    .bind(hub.as_bytes().as_slice())
    .bind(i64::try_from(epoch.legacy_stock).map_err(|_| corrupt_storage())?)
    .bind(i64::try_from(epoch.legacy_catalog).map_err(|_| corrupt_storage())?)
    .bind(super::common::integer(op.revision)?)
    .bind(super::common::integer(op.catalog_revision)?)
    .bind(op.id.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(corrupt_storage());
    }
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 21 {
        crate::hub::corrections::count_mode::persist_on(tx, hub, op).await?;
    }
    Ok(())
}

/// Shallow completeness, with no recursive replanning of a current operation.
pub(super) async fn audit_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    let invalid: i64 = sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_actual_interpretation_markers m ON m.operation_id=o.operation_id ",
        "WHERE o.kind='actual_review' AND ((o.stock_revision>? AND m.operation_id IS NULL) OR (o.stock_revision<=? AND m.operation_id IS NOT NULL))) + ",
        "(SELECT count(*) FROM hub_actual_interpretation_markers m LEFT JOIN hub_stock_operations o ON o.operation_id=m.operation_id ",
        "LEFT JOIN hub_actual_reviews r ON r.operation_id=m.operation_id LEFT JOIN hub_submissions s ON s.operation_id=m.operation_id ",
        "WHERE o.operation_id IS NULL OR r.operation_id IS NULL OR s.operation_id IS NULL OR o.kind<>'actual_review' OR ",
        "m.hub_id<>? OR o.hub_id<>m.hub_id OR s.hub_id<>m.hub_id OR s.kind<>o.kind OR s.actor_public_key<>o.actor_public_key OR s.domain_intent_hash<>o.intent_hash OR ",
        "m.epoch<>1 OR m.legacy_stock_revision<>? OR m.legacy_catalog_revision<>? OR o.stock_revision<=m.legacy_stock_revision OR ",
        "m.stock_revision<>o.stock_revision OR m.catalog_revision<>o.catalog_revision OR m.catalog_revision<m.legacy_catalog_revision OR ",
        "m.capture_hash<>s.capture_hash OR length(m.capture_hash)<>32)"
    ))
    .bind(i64::try_from(epoch.legacy_stock).map_err(|_| corrupt_storage())?)
    .bind(i64::try_from(epoch.legacy_stock).map_err(|_| corrupt_storage())?)
    .bind(hub.as_bytes().as_slice())
    .bind(i64::try_from(epoch.legacy_stock).map_err(|_| corrupt_storage())?)
    .bind(i64::try_from(epoch.legacy_catalog).map_err(|_| corrupt_storage())?)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 21 {
        crate::hub::corrections::count_mode::audit_on(tx, hub).await?;
    }
    Ok(())
}
