//! Shallow effective-quantity history. Projections are descriptive evidence;
//! neither stored observations nor arithmetic establish a fresh effect grant.
use super::*;
use harvestcircle_domain::hub::corrections::{
    DepartureQuantityAccount, DepartureQuantityAmendment, DepartureQuantityDelta,
    HUB_CORRECTION_ROWS, OriginalDepartureQuantity,
};
use harvestcircle_domain::hub::reference::WholePacks;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct CanonicalLine {
    pub original: OriginalDepartureQuantity,
    pub evidence_digest: [u8; 32],
    pub kind: String,
    pub result: HubRecordId,
    pub actor: PublicKey,
    pub intent_hash: [u8; 32],
    pub catalog_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct QuantityProjection {
    pub original: CanonicalLine,
    pub amendment: DepartureQuantityAmendment,
    pub previous: Option<HubRecordId>,
    pub report: Option<HubRecordId>,
    pub observation: Option<HubRecordId>,
}

pub(super) fn seal(
    op: &OperationRecord,
    hub: HubId,
    value: &QuantityProjection,
) -> Result<[u8; 32], SafeError> {
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let delta = value.amendment.delta;
    let (kind, after) = match delta {
        DepartureQuantityDelta::AdditionalDeparture(packs)
            if packs.value() > 0 && value.report.is_some() && value.observation.is_none() =>
        {
            (
                "correction_additional",
                value.amendment.before.checked_add(packs),
            )
        }
        DepartureQuantityDelta::IdentifiedFoundRemainder(packs)
            if packs.value() > 0 && value.report.is_none() && value.observation.is_some() =>
        {
            (
                "correction_found",
                value.amendment.before.checked_sub(packs),
            )
        }
        _ => return Err(corrupt_storage()),
    };
    if op.kind != kind
        || value.amendment.operation != own
        || value.amendment.stock_revision != op.revision
        || value.original.original.operation == own
        || value.original.original.packs.value() == 0
        || value.original.original.stock_revision == HubRevision::INITIAL
        || value.original.original.stock_revision.value() >= op.revision.value()
        || !matches!(value.original.kind.as_str(), "departure" | "actual_review")
        || value.original.catalog_revision.value() > op.catalog_revision.value()
        || value.original.recorded_at > op.recorded_at
        || value.previous == Some(own)
        || after.map_err(|_| corrupt_storage())? != value.amendment.after
    {
        return Err(corrupt_storage());
    }
    #[derive(Serialize)]
    struct Bound<'a> {
        hub: HubId,
        operation: &'a str,
        kind: &'a str,
        result: HubRecordId,
        actor: PublicKey,
        input_hash: [u8; 32],
        catalog_revision: HubRevision,
        recorded_at: UnixTimestamp,
        value: &'a QuantityProjection,
    }
    let bytes = serde_json::to_vec(&Bound {
        hub,
        operation: op.id.as_str(),
        kind: &op.kind,
        result: op.result,
        actor: op.actor,
        input_hash: op.hash,
        catalog_revision: op.catalog_revision,
        recorded_at: op.recorded_at,
        value,
    })
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.quantity.projection.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

/// Resolve the immutable native line independently of any correction head.
pub(super) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    target: &DurableRequestId,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<CanonicalLine, SafeError> {
    let op = stock::read::operation_on(tx, hub, target)
        .await?
        .ok_or_else(stock::invalid_input)?;
    if op.revision.value() > cut.value() {
        return Err(stock::invalid_input());
    }
    let revision = op.revision;
    let kind = op.kind.clone();
    let result = op.result;
    let actor = op.actor;
    let intent_hash = op.hash;
    let catalog_revision = op.catalog_revision;
    let recorded_at = op.recorded_at;
    let operation =
        crate::hub::actuals::common::operation_id(target).map_err(|_| corrupt_storage())?;
    let (trace, variant, packs, digest_query) = match op.kind.as_str() {
        "departure" => {
            let original = handling::departure::read::original_on(tx, hub, op).await?;
            let line = original
                .record
                .consumed
                .iter()
                .find(|line| line.before.id == group)
                .ok_or_else(stock::invalid_input)?;
            (
                line.before.trace,
                line.before.variant,
                line.before.packs,
                "SELECT substr(result_hash,1,33) FROM hub_departures WHERE operation_id=?",
            )
        }
        "actual_review" => {
            let original = crate::hub::actuals::projection::known_on(tx, hub, &op).await?;
            let line = original
                .lines
                .iter()
                .find(|line| line.group == group)
                .ok_or_else(stock::invalid_input)?;
            (
                original.trace,
                original.variant.id,
                line.packs,
                "SELECT substr(projection_hash,1,33) FROM hub_actual_reviews WHERE operation_id=? AND review_kind='departure'",
            )
        }
        _ => return Err(stock::invalid_input()),
    };
    let digest: Vec<u8> = sqlx::query_scalar(digest_query)
        .bind(target.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    Ok(CanonicalLine {
        original: OriginalDepartureQuantity {
            operation,
            group,
            trace,
            variant,
            stock_revision: revision,
            packs,
        },
        evidence_digest: digest.try_into().map_err(|_| corrupt_storage())?,
        kind,
        result,
        actor,
        intent_hash,
        catalog_revision,
        recorded_at,
    })
}

/// Complete cut-specific scalar history. Callers must separately verify current
/// return reservations and physical funding before constructing an effect context.
pub(in crate::hub) struct QuantityHistory {
    pub original: OriginalDepartureQuantity,
    pub amendments: Vec<DepartureQuantityAmendment>,
}
impl QuantityHistory {
    pub fn with_verified_returns(
        self,
        returns: WholePacks,
    ) -> Result<DepartureQuantityAccount, SafeError> {
        DepartureQuantityAccount::restore(self.original, self.amendments, returns)
            .map_err(|_| corrupt_storage())
    }
}

pub(in crate::hub) async fn history_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    target: &DurableRequestId,
    group: HubRecordId,
    cut: HubRevision,
) -> Result<QuantityHistory, SafeError> {
    let original = original_on(tx, hub, target, group, cut).await?;
    let count:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_corrections correction JOIN hub_stock_operations current ON current.operation_id=correction.operation_id ",
        "WHERE correction.target_operation_id=? AND correction.subject_kind='line' AND correction.subject_id=? ",
        "AND correction.family IN ('additional','found') AND current.stock_revision<=?"
    )).bind(target.as_str()).bind(group.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if !(0..=HUB_CORRECTION_ROWS as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query(concat!(
        "SELECT substr(c.operation_id,1,37) AS operation,substr(c.family,1,33) AS family,substr(c.correction_id,1,17) AS correction_id,",
        "substr(c.previous_operation_id,1,37) AS previous,substr(c.report_id,1,17) AS report,substr(c.observation_operation_id,1,37) AS observation,",
        "substr(q.target_operation_id,1,37) AS quantity_target,substr(q.group_id,1,17) AS quantity_group,q.before_packs,q.after_packs,",
        "substr(q.delta_kind,1,33) AS delta_kind,q.delta_packs,substr(q.projection_hash,1,33) AS projection_hash ",
        "FROM hub_corrections c JOIN hub_stock_operations current ON current.operation_id=c.operation_id ",
        "LEFT JOIN hub_correction_quantity_changes q ON q.operation_id=c.operation_id ",
        "WHERE c.target_operation_id=? AND c.subject_kind='line' AND c.subject_id=? AND c.family IN ('additional','found') ",
        "AND current.stock_revision<=? ORDER BY current.stock_revision LIMIT 4097"
    )).bind(target.as_str()).bind(group.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != usize::try_from(count).map_err(|_| corrupt_storage())? {
        return Err(corrupt_storage());
    }
    let mut amendments = Vec::with_capacity(rows.len());
    let mut previous = None;
    for row in rows {
        let id = DurableRequestId::parse(
            row.try_get::<String, _>("operation")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let parse_optional = |name| -> Result<Option<HubRecordId>, SafeError> {
            row.try_get::<Option<String>, _>(name)
                .map_err(|_| corrupt_storage())?
                .map(|s| {
                    DurableRequestId::parse(s)
                        .map_err(|_| corrupt_storage())
                        .and_then(|id| {
                            crate::hub::actuals::common::operation_id(&id)
                                .map_err(|_| corrupt_storage())
                        })
                })
                .transpose()
        };
        let prior = parse_optional("previous")?;
        let family: String = row.try_get("family").map_err(|_| corrupt_storage())?;
        let kind: String = row.try_get("delta_kind").map_err(|_| corrupt_storage())?;
        let packs =
            stock::read::whole_packs(row.try_get("delta_packs").map_err(|_| corrupt_storage())?)?;
        let delta = match (family.as_str(), kind.as_str()) {
            ("additional", "additional") => DepartureQuantityDelta::AdditionalDeparture(packs),
            ("found", "found") => DepartureQuantityDelta::IdentifiedFoundRemainder(packs),
            _ => return Err(corrupt_storage()),
        };
        if prior != previous
            || stock::read::record_id(&row, "correction_id")? != op.result
            || row
                .try_get::<String, _>("quantity_target")
                .map_err(|_| corrupt_storage())?
                != target.as_str()
            || stock::read::record_id(&row, "quantity_group")? != group
        {
            return Err(corrupt_storage());
        }
        let amendment = DepartureQuantityAmendment {
            operation: crate::hub::actuals::common::operation_id(&id)
                .map_err(|_| corrupt_storage())?,
            stock_revision: op.revision,
            before: stock::read::whole_packs(
                row.try_get("before_packs").map_err(|_| corrupt_storage())?,
            )?,
            after: stock::read::whole_packs(
                row.try_get("after_packs").map_err(|_| corrupt_storage())?,
            )?,
            delta,
        };
        let projection = QuantityProjection {
            original: original.clone(),
            amendment,
            previous: prior,
            report: stock::read::optional_id(&row, "report")?,
            observation: parse_optional("observation")?,
        };
        let hash: Vec<u8> = row
            .try_get("projection_hash")
            .map_err(|_| corrupt_storage())?;
        if seal(&op, hub, &projection)?.as_slice() != hash.as_slice() {
            return Err(corrupt_storage());
        }
        verify_fact_on(tx, hub, &op, &projection).await?;
        previous = Some(amendment.operation);
        amendments.push(amendment);
    }
    // Validate complete arithmetic independently of the future current-return
    // check; zero is a retained interpretation, never omitted from this history.
    DepartureQuantityAccount::restore(
        original.original,
        amendments.clone(),
        WholePacks::from_value(0).expect("zero"),
    )
    .map_err(|_| corrupt_storage())?;
    Ok(QuantityHistory {
        original: original.original,
        amendments,
    })
}

async fn verify_fact_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    value: &QuantityProjection,
) -> Result<(), SafeError> {
    match value.amendment.delta {
        DepartureQuantityDelta::AdditionalDeparture(_) => {
            let report = crate::hub::actuals::reports::by_id_on(
                tx,
                hub,
                value.report.ok_or_else(corrupt_storage)?,
            )
            .await?
            .original;
            if report.stock_revision.value() >= op.revision.value()
                || report.recorded_at > op.recorded_at
            {
                return Err(corrupt_storage());
            }
        }
        DepartureQuantityDelta::IdentifiedFoundRemainder(packs) => {
            let id = crate::hub::actuals::common::operation(
                value.observation.ok_or_else(corrupt_storage)?,
            )
            .map_err(|_| corrupt_storage())?;
            let observed = stock::read::operation_on(tx, hub, &id)
                .await?
                .ok_or_else(corrupt_storage)?;
            if observed.revision.value() >= op.revision.value()
                || observed.recorded_at > op.recorded_at
            {
                return Err(corrupt_storage());
            }
            let original = super::original_on(tx, hub, observed).await?.original;
            let harvestcircle_domain::hub::corrections::CorrectionObservedFacts::Found {
                target_operation,
                target_group,
                allocations,
                ..
            } = original.observation.facts
            else {
                return Err(corrupt_storage());
            };
            let total = allocations
                .iter()
                .try_fold(
                    WholePacks::from_value(0).expect("zero"),
                    |sum, allocation| sum.checked_add(allocation.packs),
                )
                .map_err(|_| corrupt_storage())?;
            if target_operation != value.original.original.operation
                || target_group != value.original.original.group
                || total != packs
            {
                return Err(corrupt_storage());
            }
        }
    }
    Ok(())
}
