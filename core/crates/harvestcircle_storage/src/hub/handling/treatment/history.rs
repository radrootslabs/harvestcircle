use super::super::super::stock;
use super::*;
use crate::db::corrupt_storage;
use harvestcircle_domain::hub::portions::{PhysicalLineage, PhysicalScopeAnchor};
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::treatment::{
    AcceptanceDecision, AcceptanceStatus, Hold, TreatmentAction, TreatmentCommand,
    TreatmentSelection, TreatmentStamp, treatment_record_id, treatment_result_digest,
};
use sqlx::Row;
use stock::read::{OperationRecord, optional_id, record_id, revision_field, text, whole_packs};

pub(in crate::hub::handling) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    let trace_id: Vec<u8> = sqlx::query_scalar(
        "SELECT substr(trace_id,1,17) FROM hub_treatment_records WHERE operation_id=? LIMIT 1",
    )
    .bind(operation.id.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(corrupt_storage)?;
    let trace = HubRecordId::from_bytes(trace_id.try_into().map_err(|_| corrupt_storage())?)
        .map_err(|_| corrupt_storage())?;
    let cut = HubRevision::from_value(
        operation
            .revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let graph = super::super::lineage::historical_on(tx, hub, trace, cut).await?;
    decode_on(tx, &graph, operation).await
}

pub(super) async fn decode_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    graph: &PhysicalLineage,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    super::super::history::single_event_on(tx, &operation.id).await?;
    let row=sqlx::query(concat!(
        "SELECT substr(t.record_id,1,17) AS record_id,substr(t.trace_id,1,17) AS trace_id,substr(t.kind,1,33) AS kind,substr(t.acceptance,1,9) AS acceptance,substr(t.hold_id,1,17) AS hold_id,t.expected_hold_revision,",
        "substr(t.basis_id,1,65) AS basis_id,t.basis_version,substr(t.site_timezone,1,65) AS site_timezone,substr(CAST(t.observation AS BLOB),1,513) AS observation,t.scope_count,t.affected_packs,t.on_hand,t.accepted,t.review_required,t.rejected,t.restricted,t.departure_eligible,t.history_capacity_reached,substr(t.result_hash,1,33) AS result_hash,",
        "e.sequence,substr(e.kind,1,33) AS event_kind,substr(e.trace_id,1,17) AS event_trace,substr(e.group_id,1,17) AS event_group,e.packs,substr(e.from_location_id,1,17) AS from_location_id,e.from_location_revision,substr(e.to_location_id,1,17) AS to_location_id,e.to_location_revision,",
        "substr(e.occurrence_kind,1,9) AS occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s ",
        "FROM hub_treatment_records t JOIN hub_stock_events e ON e.operation_id=t.operation_id WHERE t.operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let operation_bytes = HubRecordId::from_hex(&operation.id.as_str().replace('-', ""))
        .map_err(|_| corrupt_storage())?;
    let id = treatment_record_id(operation_bytes).map_err(|_| corrupt_storage())?;
    let kind: String = row.try_get("kind").map_err(|_| corrupt_storage())?;
    let affected = whole_packs(
        row.try_get("affected_packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let sequence: i64 = row.try_get("sequence").map_err(|_| corrupt_storage())?;
    if id != operation.result
        || id != record_id(&row, "record_id")?
        || graph.trace() != record_id(&row, "trace_id")?
        || graph.trace() != record_id(&row, "event_trace")?
        || kind != operation.kind
        || kind
            != row
                .try_get::<String, _>("event_kind")
                .map_err(|_| corrupt_storage())?
        || !(1..=harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY as i64)
            .contains(&sequence)
        || affected != whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?
        || optional_id(&row, "from_location_id")?.is_some()
        || optional_id(&row, "to_location_id")?.is_some()
        || row
            .try_get::<Option<i64>, _>("from_location_revision")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<Option<i64>, _>("to_location_revision")
            .map_err(|_| corrupt_storage())?
            .is_some()
    {
        return Err(corrupt_storage());
    }
    let basis = SampleHandlingBasis::from_stored(
        &row.try_get::<String, _>("basis_id")
            .map_err(|_| corrupt_storage())?,
        u32::try_from(
            row.try_get::<i64, _>("basis_version")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?,
        &row.try_get::<String, _>("site_timezone")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let stamp = TreatmentStamp {
        id,
        stock_revision: operation.revision,
        actor: operation.actor,
        basis,
        recorded_at: operation.recorded_at,
        occurred: stock::read::occurrence(&row)?,
        observation: SourceProvenance::new(text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    stamp
        .occurred
        .validate(stamp.recorded_at)
        .map_err(|_| corrupt_storage())?;
    let expected = HubRevision::from_value(
        operation
            .revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let anchors=sqlx::query("SELECT substr(group_id,1,17) AS group_id,created_stock_revision,captured_packs FROM hub_treatment_scope_anchors WHERE record_id=? ORDER BY group_id LIMIT 65")
        .bind(id.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let declared: i64 = row.try_get("scope_count").map_err(|_| corrupt_storage())?;
    if !(0..=64).contains(&declared) || anchors.len() != declared as usize {
        return Err(corrupt_storage());
    }
    let acceptance: Option<String> = row.try_get("acceptance").map_err(|_| corrupt_storage())?;
    let hold = optional_id(&row, "hold_id")?;
    let hold_revision: Option<i64> = row
        .try_get("expected_hold_revision")
        .map_err(|_| corrupt_storage())?;
    let (action, change, event_group) = match kind.as_str() {
        "acceptance" | "hold_place" if hold.is_none() && hold_revision.is_none() => {
            if anchors.is_empty() {
                return Err(corrupt_storage());
            }
            let anchors = anchors
                .into_iter()
                .map(|anchor| {
                    Ok(PhysicalScopeAnchor {
                        group: record_id(&anchor, "group_id")?,
                        created_revision: revision_field(&anchor, "created_stock_revision")?,
                        captured_packs: whole_packs(
                            anchor
                                .try_get("captured_packs")
                                .map_err(|_| corrupt_storage())?,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, SafeError>>()?;
            let scope = graph
                .restore_scope(expected, anchors)
                .map_err(|_| corrupt_storage())?;
            let captured = scope.anchors().iter().try_fold(
                WholePacks::from_value(0).expect("zero packs"),
                |total, anchor| {
                    total
                        .checked_add(anchor.captured_packs)
                        .map_err(|_| corrupt_storage())
                },
            )?;
            if captured != affected {
                return Err(corrupt_storage());
            }
            let group = scope.anchors()[0].group;
            let selection = TreatmentSelection::new(
                scope.anchors().iter().map(|anchor| anchor.group).collect(),
            )
            .map_err(|_| corrupt_storage())?;
            if kind == "acceptance" {
                let status = match acceptance.as_deref() {
                    Some("review") => AcceptanceStatus::ReviewRequired,
                    Some("accepted") => AcceptanceStatus::Accepted,
                    Some("rejected") => AcceptanceStatus::Rejected,
                    _ => return Err(corrupt_storage()),
                };
                (
                    TreatmentAction::SetAcceptance { selection, status },
                    TreatmentChange::Acceptance(Box::new(AcceptanceDecision {
                        stamp: stamp.clone(),
                        scope,
                        status,
                    })),
                    group,
                )
            } else {
                if acceptance.is_some() {
                    return Err(corrupt_storage());
                }
                (
                    TreatmentAction::PlaceHold { selection },
                    TreatmentChange::HoldPlaced(Box::new(Hold {
                        stamp: stamp.clone(),
                        scope,
                        release: None,
                    })),
                    group,
                )
            }
        }
        "hold_release"
            if acceptance.is_none()
                && hold.is_some()
                && hold_revision == Some(1)
                && anchors.is_empty() =>
        {
            let hold = hold.ok_or_else(corrupt_storage)?;
            let group:Vec<u8>=sqlx::query_scalar(concat!(
                "SELECT substr(a.group_id,1,17) FROM hub_treatment_records h JOIN hub_stock_operations placed ON placed.operation_id=h.operation_id JOIN hub_treatment_scope_anchors a ON a.record_id=h.record_id ",
                "WHERE h.record_id=? AND h.kind='hold_place' AND h.trace_id=? AND placed.hub_id=? AND placed.stock_revision<? ORDER BY a.group_id LIMIT 1"
            )).bind(hold.as_bytes().as_slice()).bind(graph.trace().as_bytes().as_slice()).bind(graph.hub().as_bytes().as_slice()).bind(integer(operation.revision)?).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
            let group = HubRecordId::from_bytes(group.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?;
            (
                TreatmentAction::ReleaseHold {
                    hold,
                    expected_revision: HubRevision::INITIAL,
                },
                TreatmentChange::HoldReleased {
                    hold,
                    release: Box::new(stamp.clone()),
                },
                group,
            )
        }
        _ => return Err(corrupt_storage()),
    };
    if record_id(&row, "event_group")? != event_group {
        return Err(corrupt_storage());
    }
    let summary = TreatmentSummary {
        on_hand: whole_packs(row.try_get("on_hand").map_err(|_| corrupt_storage())?)?,
        accepted: whole_packs(row.try_get("accepted").map_err(|_| corrupt_storage())?)?,
        review_required: whole_packs(
            row.try_get("review_required")
                .map_err(|_| corrupt_storage())?,
        )?,
        rejected: whole_packs(row.try_get("rejected").map_err(|_| corrupt_storage())?)?,
        restricted: whole_packs(row.try_get("restricted").map_err(|_| corrupt_storage())?)?,
        departure_eligible: whole_packs(
            row.try_get("departure_eligible")
                .map_err(|_| corrupt_storage())?,
        )?,
    };
    summary.validate().map_err(|_| corrupt_storage())?;
    if affected.value() > summary.on_hand.value() {
        return Err(corrupt_storage());
    }
    let capacity = match row
        .try_get::<i64, _>("history_capacity_reached")
        .map_err(|_| corrupt_storage())?
    {
        0 => false,
        1 => true,
        _ => return Err(corrupt_storage()),
    };
    let primary:i64=sqlx::query_scalar("SELECT count(*) FROM hub_treatment_records t JOIN hub_stock_operations o ON o.operation_id=t.operation_id WHERE t.trace_id=? AND t.kind<>'hold_release' AND o.stock_revision<=?")
        .bind(graph.trace().as_bytes().as_slice()).bind(integer(operation.revision)?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if !(0..=256).contains(&primary) || capacity != (primary == 256) {
        return Err(corrupt_storage());
    }
    let command = TreatmentCommand {
        expected_stock_revision: expected,
        expected_catalog_revision: operation.catalog_revision,
        action,
        occurred: stamp.occurred,
        observation: stamp.observation,
    };
    let digest = treatment_result_digest(&operation.hash, &change, summary, affected, capacity);
    let stored: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if command.fingerprint(graph.hub(), operation.actor, id) != operation.hash
        || stored.as_slice() != digest
    {
        return Err(corrupt_storage());
    }
    Ok(HubHandlingReceipt {
        operation: operation.id,
        kind: HubHandlingKind::parse(&kind)?,
        revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        trace: graph.trace(),
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        result: HubHandlingResult::Treatment(Box::new(HubTreatmentResult {
            change,
            summary,
            affected_packs: affected,
            history_capacity_reached: capacity,
        })),
    })
}
