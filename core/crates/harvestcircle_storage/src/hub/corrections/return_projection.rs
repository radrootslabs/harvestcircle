//! Separately sealed return linkage avoids recursively replanning prior Some
//! originals. These projections describe committed outcomes, never fresh grants.
use super::*;
use crate::hub::custody::returns;
use harvestcircle_domain::hub::{
    departure::{DepartureRecord, departure_result_digest},
    reference::WholePacks,
    returns::{EstablishedReturnTotal, ReturnPlan},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 16 * 1024;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Known {
    departure: String,
    operation: String,
    evidence_hash: [u8; 32],
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Total {
    group: String,
    packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    population: String,
    variant: String,
    packs: u32,
    claim_departure: String,
    claim_group: Option<String>,
    known: Known,
    plausible_groups: Vec<String>,
    totals: Vec<Total>,
    selected: Option<String>,
    resolution: String,
    reason: Option<String>,
    timing: String,
    population_hash: [u8; 32],
    allowance_scope_hash: [u8; 32],
}

fn id(text: &str) -> Result<HubRecordId, SafeError> {
    HubRecordId::from_hex(text).map_err(|_| corrupt_storage())
}
fn known(record: &DepartureRecord) -> Result<Known, SafeError> {
    Ok(Known {
        departure: record.id.to_hex(),
        operation: record.operation.to_hex(),
        evidence_hash: departure_result_digest(&[0; 32], record).map_err(|_| corrupt_storage())?,
    })
}
fn encode(data: &Data) -> Result<Vec<u8>, SafeError> {
    if data.schema != 1
        || data.totals.is_empty()
        || data.totals.len() > 64
        || data.plausible_groups.len() > 64
        || data.packs == 0
        || data.packs > 1_000_000
        || !matches!(
            data.timing.as_str(),
            "after_or_same" | "uncertain" | "contradictory"
        )
    {
        return Err(corrupt_storage());
    }
    for text in [
        &data.population,
        &data.variant,
        &data.claim_departure,
        &data.known.departure,
        &data.known.operation,
    ] {
        id(text)?;
    }
    if data.claim_departure != data.known.departure {
        return Err(corrupt_storage());
    }
    if let Some(group) = &data.claim_group {
        id(group)?;
    }
    let mut groups = std::collections::BTreeSet::new();
    for total in &data.totals {
        if !groups.insert(id(&total.group)?) || total.packs > 1_000_000 {
            return Err(corrupt_storage());
        }
    }
    let mut plausible = std::collections::BTreeSet::new();
    for group in &data.plausible_groups {
        let group = id(group)?;
        if !groups.contains(&group) || !plausible.insert(group) {
            return Err(corrupt_storage());
        }
    }
    match data.resolution.as_str() {
        "established"
            if data.reason.is_none()
                && data.selected.is_some()
                && data.selected == data.claim_group
                && data.timing != "contradictory" =>
        {
            if !plausible.contains(&id(data.selected.as_ref().ok_or_else(corrupt_storage)?)?) {
                return Err(corrupt_storage());
            }
        }
        "unresolved"
            if data.selected.is_none()
                && data.reason.as_deref().is_some_and(|why| {
                    matches!(
                        why,
                        "original_group_unknown"
                            | "no_matching_variant"
                            | "claimed_group_missing"
                            | "variant_conflict"
                            | "quantity_conflict"
                            | "time_conflict"
                    )
                }) => {}
        _ => return Err(corrupt_storage()),
    }
    let bytes = serde_json::to_vec(data).map_err(|_| corrupt_storage())?;
    if bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Data, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if encode(&data)?.as_slice() != bytes {
        return Err(corrupt_storage());
    }
    Ok(data)
}
fn seal(scope: [u8; 32], result: [u8; 32], bytes: &[u8]) -> Result<[u8; 32], SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    // The verified allowance scope already binds the exact own header/intent,
    // clocks, full result and input context. This is a distinct output purpose.
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.link.projection.v1\0");
    hash.update(scope);
    hash.update(result);
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(in crate::hub) struct WriteProjection<'a> {
    pub op: &'a OperationRecord,
    pub record: &'a ReturnPlan,
    pub departure: &'a DepartureRecord,
    pub totals: &'a [EstablishedReturnTotal],
    pub population_hash: [u8; 32],
    pub result_hash: [u8; 32],
}
pub(in crate::hub) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    value: WriteProjection<'_>,
) -> Result<(), SafeError> {
    let WriteProjection {
        op,
        record,
        departure,
        totals,
        population_hash,
        result_hash,
    } = value;
    let capture = record
        .current_allowances
        .as_ref()
        .ok_or_else(corrupt_storage)?;
    if op.kind != "return"
        || record.identities.operation
            != crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?
        || record.identities.returned != op.result
        || record.stock_revision != op.revision
        || record.catalog_revision != op.catalog_revision
        || record.intake.actor != op.actor
        || record.intake.recorded_at != op.recorded_at
        || capture.hub != hub
        || capture.departure != departure.id
        || capture.operation != departure.operation
        || capture.stock_revision.value().checked_add(1) != Some(op.revision.value())
    {
        return Err(corrupt_storage());
    }
    let input = super::return_capture::encode(capture)?;
    let scope = super::return_capture::scope_hash(op, hub, result_hash, &input)?;
    let claim = record.linkage.claim.ok_or_else(corrupt_storage)?;
    let (resolution, reason) = returns::resolution(&record.linkage.resolution);
    let data = Data {
        schema: 1,
        population: record.identities.population.to_hex(),
        variant: record.intake.variant.id.to_hex(),
        packs: record.intake.packs.value(),
        claim_departure: claim.departure.to_hex(),
        claim_group: claim.group.map(|id| id.to_hex()),
        known: known(departure)?,
        plausible_groups: record
            .linkage
            .plausible_sources
            .iter()
            .map(|source| source.original.group.to_hex())
            .collect(),
        totals: totals
            .iter()
            .map(|total| Total {
                group: total.original_group.to_hex(),
                packs: total.packs.value(),
            })
            .collect(),
        selected: record.linkage.established_group().map(|id| id.to_hex()),
        resolution: resolution.into(),
        reason: reason.map(str::to_owned),
        timing: returns::timing(record.linkage.timing).into(),
        population_hash,
        allowance_scope_hash: scope,
    };
    let bytes = encode(&data)?;
    let hash = seal(scope, result_hash, &bytes)?;
    sqlx::query("INSERT INTO hub_correction_captures (operation_id,kind,payload_schema,payload,scope_hash) VALUES (?,'return',1,?,?)")
        .bind(op.id.as_str()).bind(input).bind(scope.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query("INSERT INTO hub_correction_return_projections (operation_id,payload_schema,payload,projection_hash) VALUES (?,1,?,?)")
        .bind(op.id.as_str()).bind(bytes).bind(hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

pub(in crate::hub) struct ReservationProjection {
    pub operation: HubRecordId,
    pub variant: HubRecordId,
    pub packs: WholePacks,
    pub known_operation: HubRecordId,
    pub known_departure: HubRecordId,
    pub claim_departure: HubRecordId,
    pub claim_group: Option<HubRecordId>,
    pub established_group: Option<HubRecordId>,
    pub stock_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
}

/// None permits only the caller's fully verified legacy None original path.
pub(in crate::hub) async fn read_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Option<ReservationProjection>, SafeError> {
    if op.kind != "return" {
        return Err(corrupt_storage());
    }
    let own = returns::read::own_on(tx, hub, op).await?;
    let capture = super::return_capture::read_on(tx, hub, op, own.result_hash).await?;
    let Some(capture) = capture else {
        return Ok(None);
    };
    crate::hub::submission::verify_recorded_input_on(
        tx,
        hub,
        op,
        harvestcircle_application::hub_submission::HubSubmittedRequest::Return(&own.command),
    )
    .await?;
    let row=sqlx::query("SELECT payload_schema,substr(payload,1,16385) AS payload,substr(projection_hash,1,33) AS projection_hash FROM hub_correction_return_projections WHERE operation_id=? LIMIT 1")
        .bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<i64, _>("payload_schema")
        .map_err(|_| corrupt_storage())?
        != 1
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let data = decode(&bytes)?;
    let scope = super::return_capture::scope_hash(
        op,
        hub,
        own.result_hash,
        &super::return_capture::encode(&capture)?,
    )?;
    let hash: Vec<u8> = row
        .try_get("projection_hash")
        .map_err(|_| corrupt_storage())?;
    if data.allowance_scope_hash != scope
        || seal(scope, own.result_hash, &bytes)?.as_slice() != hash.as_slice()
    {
        return Err(corrupt_storage());
    }
    let departure = returns::context::departure_on(
        tx,
        hub,
        own.known.ok_or_else(corrupt_storage)?,
        own.command.expected_stock_revision,
    )
    .await?
    .ok_or_else(corrupt_storage)?;
    if data.known != known(&departure)?
        || capture.departure != departure.id
        || capture.operation != departure.operation
        || data.totals.len() != own.source_count
        || data.population != own.population.id.to_hex()
        || data.variant != own.population.variant.id.to_hex()
        || data.packs != own.population.original_packs.value()
        || data.resolution != own.resolution
        || data.reason != own.reason
        || data.timing != own.timing
        || data.selected.as_ref().map(|s| id(s)).transpose()? != own.established
    {
        return Err(corrupt_storage());
    }
    let claim = own.command.departure.ok_or_else(corrupt_storage)?;
    if data.claim_departure != claim.departure.to_hex()
        || data.claim_group != claim.group.map(|id| id.to_hex())
    {
        return Err(corrupt_storage());
    }
    let population_hash = harvestcircle_domain::hub::returns::return_population_digest(
        &op.hash,
        hub,
        op.catalog_revision,
        own.ids,
        &own.population,
    )
    .map_err(|_| corrupt_storage())?;
    if data.population_hash != population_hash {
        return Err(corrupt_storage());
    }
    let lines = &departure
        .paper
        .versions
        .last()
        .ok_or_else(corrupt_storage)?
        .material
        .lines;
    let rows=sqlx::query("SELECT ordinal,substr(original_group_id,1,17) AS group_id,substr(departure_operation_id,1,37) AS departure_operation,previously_returned_packs FROM hub_return_sources WHERE return_id=? ORDER BY ordinal LIMIT 65")
        .bind(own.ids.returned.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != lines.len() || rows.len() != data.totals.len() {
        return Err(corrupt_storage());
    }
    for (n, ((row, line), total)) in rows.into_iter().zip(lines).zip(&data.totals).enumerate() {
        let source = DurableRequestId::parse(
            row.try_get::<String, _>("departure_operation")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        if row
            .try_get::<i64, _>("ordinal")
            .map_err(|_| corrupt_storage())?
            != i64::try_from(n).map_err(|_| corrupt_storage())?
            || stock::read::record_id(&row, "group_id")? != line.group
            || total.group != line.group.to_hex()
            || crate::hub::actuals::common::operation_id(&source).map_err(|_| corrupt_storage())?
                != departure.operation
            || stock::read::whole_packs(
                row.try_get("previously_returned_packs")
                    .map_err(|_| corrupt_storage())?,
            )?
            .value()
                != total.packs
        {
            return Err(corrupt_storage());
        }
    }
    let plausible = lines
        .iter()
        .filter(|line| line.variant.id == own.population.variant.id)
        .map(|line| line.group.to_hex())
        .collect::<Vec<_>>();
    if plausible != data.plausible_groups {
        return Err(corrupt_storage());
    }
    let totals = data
        .totals
        .iter()
        .map(|total| {
            Ok::<_, SafeError>(EstablishedReturnTotal {
                original_group: id(&total.group)?,
                packs: WholePacks::from_value(total.packs).map_err(|_| corrupt_storage())?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    capture
        .validate(
            &harvestcircle_domain::hub::returns::ReturnContext {
                hub,
                stock_revision: own.command.expected_stock_revision,
                catalog: &own.catalog,
                departure: Some(&departure),
                established_return_totals: &totals,
                current_allowances: Some(&capture),
            },
            &departure,
        )
        .map_err(|_| corrupt_storage())?;
    Ok(Some(ReservationProjection {
        operation: own.ids.operation,
        variant: own.population.variant.id,
        packs: own.population.original_packs,
        known_operation: departure.operation,
        known_departure: departure.id,
        claim_departure: claim.departure,
        claim_group: claim.group,
        established_group: own.established,
        stock_revision: op.revision,
        recorded_at: op.recorded_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Data {
        let text = |n| HubRecordId::from_bytes([n; 16]).unwrap().to_hex();
        Data {
            schema: 1,
            population: text(1),
            variant: text(2),
            packs: 2,
            claim_departure: text(3),
            claim_group: Some(text(4)),
            known: Known {
                departure: text(3),
                operation: text(5),
                evidence_hash: [6; 32],
            },
            plausible_groups: vec![text(4)],
            totals: vec![Total {
                group: text(4),
                packs: 0,
            }],
            selected: Some(text(4)),
            resolution: "established".into(),
            reason: None,
            timing: "uncertain".into(),
            population_hash: [7; 32],
            allowance_scope_hash: [8; 32],
        }
    }
    #[test]
    fn bounded_output_codec_retains_established_and_unresolved_linkage_without_authority() {
        let mut data = fixture();
        let bytes = encode(&data).unwrap();
        assert_eq!(decode(&bytes).unwrap(), data);
        data.selected = None;
        data.resolution = "unresolved".into();
        data.reason = Some("quantity_conflict".into());
        assert_eq!(decode(&encode(&data).unwrap()).unwrap(), data);
        let text = String::from_utf8(bytes).unwrap();
        for (from, to) in [
            ("\"schema\":1", "\"schema\":2"),
            ("\"schema\":1", "\"schema\":1,\"unknown\":0"),
            ("\"schema\":1", "\"schema\":1,\"schema\":1"),
        ] {
            let changed = text.replace(from, to);
            assert_ne!(text, changed);
            assert!(decode(changed.as_bytes()).is_err());
        }
        assert!(decode(&[]).is_err());
        assert!(decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
    #[test]
    fn inconsistent_or_unbounded_output_links_fail_before_serializing() {
        let original = fixture();
        for fault in 0..9 {
            let mut value = original.clone();
            match fault {
                0 => value.totals.clear(),
                1 => value.totals.push(value.totals[0].clone()),
                2 => value.totals = vec![value.totals[0].clone(); 65],
                3 => value
                    .plausible_groups
                    .push(value.plausible_groups[0].clone()),
                4 => value.selected = None,
                5 => value.claim_group = None,
                6 => value.timing = "contradictory".into(),
                7 => value.reason = Some("invented_clearance".into()),
                8 => value.claim_departure = HubRecordId::from_bytes([9; 16]).unwrap().to_hex(),
                _ => unreachable!(),
            }
            assert!(encode(&value).is_err(), "fault {fault}");
        }
    }
    #[test]
    fn output_purpose_binds_allowance_scope_full_result_and_every_linkage_field() {
        let original = fixture();
        let bytes = encode(&original).unwrap();
        let scope = original.allowance_scope_hash;
        let first = seal(scope, [9; 32], &bytes).unwrap();
        assert_ne!(first, seal([10; 32], [9; 32], &bytes).unwrap());
        assert_ne!(first, seal(scope, [10; 32], &bytes).unwrap());
        for fault in 0..6 {
            let mut value = original.clone();
            match fault {
                0 => value.population_hash = [11; 32],
                1 => value.known.evidence_hash = [12; 32],
                2 => value.totals[0].packs = 1,
                3 => value.packs = 3,
                4 => value.timing = "after_or_same".into(),
                5 => {
                    value.resolution = "unresolved".into();
                    value.reason = Some("quantity_conflict".into());
                    value.selected = None;
                }
                _ => unreachable!(),
            }
            assert_ne!(
                first,
                seal(scope, [9; 32], &encode(&value).unwrap()).unwrap()
            );
        }
    }
}
