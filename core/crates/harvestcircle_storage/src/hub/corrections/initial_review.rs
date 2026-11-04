//! Fixed negative constraints from independently sealed found births. Later
//! acceptance is interpreted by the ordinary treatment projection, never here.
use super::*;
use harvestcircle_domain::hub::{
    portions::{PhysicalLineage, PhysicalScopeAnchor},
    reference::SampleHandlingBasis,
    stock::Occurrence,
    treatment::{
        HUB_TREATMENT_RECORDS_PER_TRACE, InitialGroupReview, TreatmentStamp, treatment_record_id,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const REVIEW_BYTES: usize = 64 * 1024;

pub(super) fn seal(
    op: &OperationRecord,
    hub: HubId,
    review: &InitialGroupReview,
    birth_seals: &[(HubRecordId, [u8; 32])],
) -> Result<[u8; 32], SafeError> {
    if op.kind != "correction_found"
        || review.scope.hub() != hub
        || review.scope.captured_revision() != op.revision
        || review.stamp.stock_revision != op.revision
        || review.stamp.actor != op.actor
        || review.stamp.recorded_at != op.recorded_at
        || review.stamp.occurred != Occurrence::Known(op.recorded_at)
        || review.stamp.basis != SampleHandlingBasis::WHOLE_PACK_V1
        || review.stamp.id
            != treatment_record_id(
                crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?
        || birth_seals.is_empty()
        || birth_seals.len() > 64
        || birth_seals.len() != review.scope.anchors().len()
        || birth_seals
            .iter()
            .zip(review.scope.anchors())
            .any(|((group, _), anchor)| *group != anchor.group)
        || birth_seals.windows(2).any(|pair| pair[0].0 >= pair[1].0)
    {
        return Err(corrupt_storage());
    }
    #[derive(Serialize)]
    struct Projection<'a> {
        operation: &'a str,
        kind: &'a str,
        result: HubRecordId,
        hub: HubId,
        catalog_revision: HubRevision,
        review: &'a InitialGroupReview,
        birth_seals: &'a [(HubRecordId, [u8; 32])],
    }
    let bytes = serde_json::to_vec(&Projection {
        operation: op.id.as_str(),
        kind: &op.kind,
        result: op.result,
        hub,
        catalog_revision: op.catalog_revision,
        review,
        birth_seals,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > REVIEW_BYTES {
        return Err(corrupt_storage());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.found.initial_review.projection.v1\0");
    hash.update(op.hash);
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(in crate::hub) async fn through_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &harvestcircle_domain::hub::portions::PhysicalLineage,
) -> Result<Vec<InitialGroupReview>, SafeError> {
    reference::validate_basis_on(tx, hub).await?;
    let (count,bytes):(i64,i64)=sqlx::query_as(concat!(
        "SELECT count(*),COALESCE(sum(length(review.payload)),0) FROM hub_correction_initial_reviews review ",
        "JOIN hub_stock_operations original ON original.operation_id=review.operation_id ",
        "WHERE review.trace_id=? AND original.hub_id=? AND original.stock_revision<=?"
    )).bind(graph.trace().as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(graph.revision().value()).map_err(|_|corrupt_storage())?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if !(0..=HUB_TREATMENT_RECORDS_PER_TRACE as i64).contains(&count)
        || !(0..=(HUB_TREATMENT_RECORDS_PER_TRACE * REVIEW_BYTES) as i64).contains(&bytes)
    {
        return Err(corrupt_storage());
    }
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(review.operation_id,1,37) FROM hub_correction_initial_reviews review ",
        "JOIN hub_stock_operations original ON original.operation_id=review.operation_id ",
        "WHERE review.trace_id=? AND original.hub_id=? AND original.stock_revision<=? ORDER BY original.stock_revision LIMIT 257"
    )).bind(graph.trace().as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(graph.revision().value()).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() != usize::try_from(count).map_err(|_| corrupt_storage())? {
        return Err(corrupt_storage());
    }
    let mut expected = std::collections::BTreeSet::new();
    for node in graph.nodes().iter().filter(|node| node.recovery.is_some()) {
        let operation:String=sqlx::query_scalar("SELECT substr(created_operation_id,1,37) FROM hub_group_identification WHERE group_id=? LIMIT 1")
            .bind(node.id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        expected.insert(operation);
    }
    if ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        != expected
    {
        return Err(corrupt_storage());
    }
    let mut reviews = Vec::with_capacity(ids.len());
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        reviews.push(original_on(tx, hub, graph, &op).await?);
    }
    Ok(reviews)
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &PhysicalLineage,
    op: &OperationRecord,
) -> Result<InitialGroupReview, SafeError> {
    if op.kind != "correction_found"
        || graph.hub() != hub
        || op.revision.value() > graph.revision().value()
    {
        return Err(corrupt_storage());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(record_id,1,17) AS record_id,substr(trace_id,1,17) AS trace_id,capture_stock_revision,anchor_count,payload_schema,",
        "substr(payload,1,65537) AS payload,substr(result_hash,1,33) AS result_hash,",
        "(SELECT count(*) FROM hub_correction_found_allocations allocation WHERE allocation.operation_id=review.operation_id) AS allocations,",
        "(SELECT count(*) FROM hub_group_identification birth WHERE birth.created_operation_id=review.operation_id) AS births ",
        "FROM hub_correction_initial_reviews review WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let operation =
        crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let record = treatment_record_id(operation).map_err(|_| corrupt_storage())?;
    let count: i64 = row.try_get("anchor_count").map_err(|_| corrupt_storage())?;
    if stock::read::record_id(&row, "record_id")? != record
        || stock::read::record_id(&row, "trace_id")? != graph.trace()
        || stock::read::revision_field(&row, "capture_stock_revision")? != op.revision
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
        || !(1..=64).contains(&count)
        || count
            != row
                .try_get::<i64, _>("allocations")
                .map_err(|_| corrupt_storage())?
        || count
            != row
                .try_get::<i64, _>("births")
                .map_err(|_| corrupt_storage())?
    {
        return Err(corrupt_storage());
    }
    let anchors=sqlx::query(concat!(
        "SELECT substr(group_id,1,17) AS group_id,created_revision,packs FROM hub_correction_initial_review_anchors ",
        "WHERE operation_id=? ORDER BY group_id LIMIT 65"
    )).bind(op.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if anchors.len() != usize::try_from(count).map_err(|_| corrupt_storage())? {
        return Err(corrupt_storage());
    }
    let mut scope = Vec::with_capacity(anchors.len());
    let mut seals = Vec::with_capacity(anchors.len());
    let mut basis = None;
    let mut observed = None;
    for anchor in anchors {
        let group = stock::read::record_id(&anchor, "group_id")?;
        let created = stock::read::revision_field(&anchor, "created_revision")?;
        let packs =
            stock::read::whole_packs(anchor.try_get("packs").map_err(|_| corrupt_storage())?)?;
        let projection = super::birth::birth_on(tx, hub, op, group).await?;
        projection
            .birth
            .validate(graph, op.recorded_at)
            .map_err(|_| corrupt_storage())?;
        if created != op.revision
            || packs != projection.group.packs
            || packs.value() == 0
            || basis.as_ref().is_some_and(|b| b != &projection.basis)
            || observed.is_some_and(|id| id != projection.observation_operation)
        {
            return Err(corrupt_storage());
        }
        basis = Some(projection.basis);
        observed = Some(projection.observation_operation);
        let stored:Vec<u8>=sqlx::query_scalar("SELECT substr(projection_hash,1,33) FROM hub_correction_found_allocations WHERE operation_id=? AND child_group_id=? LIMIT 1")
            .bind(op.id.as_str()).bind(group.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        let stored: [u8; 32] = stored.try_into().map_err(|_| corrupt_storage())?;
        seals.push((group, stored));
        scope.push(PhysicalScopeAnchor {
            group,
            created_revision: created,
            captured_packs: packs,
        });
    }
    let review = InitialGroupReview {
        stamp: TreatmentStamp {
            id: record,
            stock_revision: op.revision,
            actor: op.actor,
            basis: SampleHandlingBasis::WHOLE_PACK_V1,
            recorded_at: op.recorded_at,
            occurred: Occurrence::Known(op.recorded_at),
            observation: basis.ok_or_else(corrupt_storage)?,
        },
        scope: graph
            .restore_scope(op.revision, scope)
            .map_err(|_| corrupt_storage())?,
    };
    let bytes: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let saved: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if bytes.is_empty()
        || bytes.len() > REVIEW_BYTES
        || serde_json::to_vec(&review).map_err(|_| corrupt_storage())? != bytes
        || seal(op, hub, &review, &seals)?.as_slice() != saved.as_slice()
    {
        return Err(corrupt_storage());
    }
    Ok(review)
}
