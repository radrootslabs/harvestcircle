//! Immutable committed allowance data, never an archived request or a grant.
use super::*;
use harvestcircle_domain::hub::{
    reference::WholePacks,
    returns::{ReturnAllowanceCapture, ReturnLineAllowance, ReturnQuantityHead},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 64 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    departure: String,
    operation: String,
    stock_revision: u64,
    lines: Vec<Line>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    group: String,
    effective: u32,
    physical: u32,
    head: Option<Head>,
    funding_hash: [u8; 32],
    unresolved: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    operation: String,
    stock_revision: u64,
    projection_hash: [u8; 32],
}

pub(super) fn encode(value: &ReturnAllowanceCapture) -> Result<Vec<u8>, SafeError> {
    if value.lines.is_empty() || value.lines.len() > 64 {
        return Err(corrupt_storage());
    }
    let mut lines = value
        .lines
        .iter()
        .map(|line| Line {
            group: line.original_group.to_hex(),
            effective: line.effective_departure.value(),
            physical: line.original_debit_return_bound.value(),
            head: line.quantity_head.map(|head| Head {
                operation: head.operation.to_hex(),
                stock_revision: head.stock_revision.value(),
                projection_hash: head.projection_hash,
            }),
            funding_hash: line.funding_hash,
            unresolved: line.unresolved_returns,
        })
        .collect::<Vec<_>>();
    lines.sort_by(|a, b| a.group.cmp(&b.group));
    if lines.windows(2).any(|pair| pair[0].group == pair[1].group)
        || lines.iter().any(|line| {
            line.funding_hash == [0; 32]
                || line.head.as_ref().is_some_and(|head| {
                    head.stock_revision > value.stock_revision.value()
                        || head.projection_hash == [0; 32]
                })
        })
    {
        return Err(corrupt_storage());
    }
    let bytes = serde_json::to_vec(&Data {
        schema: 1,
        departure: value.departure.to_hex(),
        operation: value.operation.to_hex(),
        stock_revision: value.stock_revision.value(),
        lines,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    Ok(bytes)
}

pub(super) fn decode(hub: HubId, bytes: &[u8]) -> Result<ReturnAllowanceCapture, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let data: Data = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if data.schema != 1 || data.lines.is_empty() || data.lines.len() > 64 {
        return Err(corrupt_storage());
    }
    let id = |text: &str| HubRecordId::from_hex(text).map_err(|_| corrupt_storage());
    let revision = |value| HubRevision::from_value(value).map_err(|_| corrupt_storage());
    let packs = |value| WholePacks::from_value(value).map_err(|_| corrupt_storage());
    let mut lines = Vec::with_capacity(data.lines.len());
    for line in data.lines {
        lines.push(ReturnLineAllowance {
            original_group: id(&line.group)?,
            effective_departure: packs(line.effective)?,
            original_debit_return_bound: packs(line.physical)?,
            quantity_head: line
                .head
                .map(|head| {
                    Ok::<_, SafeError>(ReturnQuantityHead {
                        operation: id(&head.operation)?,
                        stock_revision: revision(head.stock_revision)?,
                        projection_hash: head.projection_hash,
                    })
                })
                .transpose()?,
            funding_hash: line.funding_hash,
            unresolved_returns: line.unresolved,
        });
    }
    let value = ReturnAllowanceCapture {
        hub,
        departure: id(&data.departure)?,
        operation: id(&data.operation)?,
        stock_revision: revision(data.stock_revision)?,
        lines,
    };
    if encode(&value)?.as_slice() != bytes {
        return Err(corrupt_storage());
    }
    Ok(value)
}

pub(super) fn scope_hash(
    op: &OperationRecord,
    hub: HubId,
    result_hash: [u8; 32],
    payload: &[u8],
) -> Result<[u8; 32], SafeError> {
    if op.kind != "return" || payload.is_empty() || payload.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    #[derive(Serialize)]
    struct Header<'a> {
        operation: &'a str,
        kind: &'a str,
        hub: HubId,
        actor: PublicKey,
        input: [u8; 32],
        result: HubRecordId,
        stock_revision: HubRevision,
        catalog_revision: HubRevision,
        recorded_at: UnixTimestamp,
        result_hash: [u8; 32],
    }
    let header = serde_json::to_vec(&Header {
        operation: op.id.as_str(),
        kind: &op.kind,
        hub,
        actor: op.actor,
        input: op.hash,
        result: op.result,
        stock_revision: op.revision,
        catalog_revision: op.catalog_revision,
        recorded_at: op.recorded_at,
        result_hash,
    })
    .map_err(|_| corrupt_storage())?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.allowance.capture.v1\0");
    for bytes in [header.as_slice(), payload] {
        hash.update(
            u64::try_from(bytes.len())
                .map_err(|_| corrupt_storage())?
                .to_be_bytes(),
        );
        hash.update(bytes);
    }
    Ok(hash.finalize().into())
}

/// Both records are required for new captures. If both are absent, the owning
/// original reader must still verify its exact old None result seal.
pub(in crate::hub) async fn read_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    result_hash: [u8; 32],
) -> Result<Option<ReturnAllowanceCapture>, SafeError> {
    if op.kind != "return" {
        return Err(corrupt_storage());
    }
    let row=sqlx::query("SELECT substr(kind,1,33) AS kind,payload_schema,substr(payload,1,65537) AS payload,substr(scope_hash,1,33) AS scope_hash FROM hub_correction_captures WHERE operation_id=? LIMIT 1")
        .bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let projections: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_correction_return_projections WHERE operation_id=?",
    )
    .bind(op.id.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let Some(row) = row else {
        return if projections == 0 {
            Ok(None)
        } else {
            Err(corrupt_storage())
        };
    };
    if projections != 1
        || row
            .try_get::<String, _>("kind")
            .map_err(|_| corrupt_storage())?
            != "return"
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let hash: Vec<u8> = row.try_get("scope_hash").map_err(|_| corrupt_storage())?;
    if scope_hash(op, hub, result_hash, &bytes)?.as_slice() != hash.as_slice() {
        return Err(corrupt_storage());
    }
    let capture = decode(hub, &bytes)?;
    if capture.stock_revision.value().checked_add(1) != Some(op.revision.value()) {
        return Err(corrupt_storage());
    }
    Ok(Some(capture))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> ReturnAllowanceCapture {
        let id = |n| HubRecordId::from_bytes([n; 16]).unwrap();
        ReturnAllowanceCapture {
            hub: HubId::from_bytes([1; 16]).unwrap(),
            departure: id(2),
            operation: id(3),
            stock_revision: HubRevision::from_value(8).unwrap(),
            lines: vec![ReturnLineAllowance {
                original_group: id(4),
                effective_departure: WholePacks::from_value(0).unwrap(),
                original_debit_return_bound: WholePacks::from_value(0).unwrap(),
                quantity_head: Some(ReturnQuantityHead {
                    operation: id(5),
                    stock_revision: HubRevision::from_value(8).unwrap(),
                    projection_hash: [6; 32],
                }),
                funding_hash: [7; 32],
                unresolved_returns: true,
            }],
        }
    }
    #[test]
    fn committed_allowance_codec_keeps_zero_and_uncertainty_and_rejects_noncanonical_data() {
        let value = fixture();
        let bytes = encode(&value).unwrap();
        assert_eq!(decode(value.hub, &bytes).unwrap(), value);
        let mut extra = bytes.clone();
        extra.push(b' ');
        assert!(decode(value.hub, &extra).is_err());
        let text = String::from_utf8(bytes).unwrap();
        for (from, to) in [
            ("\"schema\":1", "\"schema\":2"),
            ("\"schema\":1", "\"schema\":1,\"unknown\":0"),
            ("\"schema\":1", "\"schema\":1,\"schema\":1"),
            ("\"effective\":0", "\"effective\":1000001"),
        ] {
            let changed = text.replace(from, to);
            assert_ne!(changed, text);
            assert!(decode(value.hub, changed.as_bytes()).is_err());
        }
        assert!(decode(value.hub, &[]).is_err());
        assert!(decode(value.hub, &vec![b' '; MAX_BYTES + 1]).is_err());
    }
    #[test]
    fn allowance_codec_rejects_duplicate_oversize_or_unproved_heads_before_serialization() {
        let value = fixture();
        for fault in 0..5 {
            let mut changed = value.clone();
            match fault {
                0 => changed.lines.clear(),
                1 => changed.lines.push(changed.lines[0].clone()),
                2 => changed.lines = vec![changed.lines[0].clone(); 65],
                3 => {
                    changed.lines[0]
                        .quantity_head
                        .as_mut()
                        .unwrap()
                        .stock_revision = HubRevision::from_value(9).unwrap()
                }
                4 => changed.lines[0].funding_hash = [0; 32],
                _ => unreachable!(),
            }
            assert!(encode(&changed).is_err());
        }
    }
    #[test]
    fn allowance_scope_hash_binds_native_header_full_result_and_canonical_payload() {
        let value = fixture();
        let bytes = encode(&value).unwrap();
        let operation = || OperationRecord {
            id: crate::hub::stock::tests::operation(100),
            kind: "return".into(),
            actor: crate::hub::stock::tests::owner(),
            hash: [10; 32],
            catalog_revision: HubRevision::INITIAL,
            revision: HubRevision::from_value(9).unwrap(),
            result: HubRecordId::from_bytes([11; 16]).unwrap(),
            recorded_at: UnixTimestamp::from_seconds(200).unwrap(),
        };
        let original = scope_hash(&operation(), value.hub, [12; 32], &bytes).unwrap();
        for fault in 0..6 {
            let mut op = operation();
            match fault {
                0 => op.hash[0] ^= 1,
                1 => op.actor = crate::hub::stock::tests::other(),
                2 => op.catalog_revision = HubRevision::from_value(2).unwrap(),
                3 => op.revision = HubRevision::from_value(10).unwrap(),
                4 => op.result = HubRecordId::from_bytes([13; 16]).unwrap(),
                5 => op.recorded_at = UnixTimestamp::from_seconds(201).unwrap(),
                _ => unreachable!(),
            }
            assert_ne!(
                scope_hash(&op, value.hub, [12; 32], &bytes).unwrap(),
                original
            );
        }
        assert_ne!(
            scope_hash(&operation(), value.hub, [13; 32], &bytes).unwrap(),
            original
        );
        let mut changed = value.clone();
        changed.lines[0].funding_hash = [14; 32];
        assert_ne!(
            scope_hash(
                &operation(),
                value.hub,
                [12; 32],
                &encode(&changed).unwrap()
            )
            .unwrap(),
            original
        );
        let mut wrong = operation();
        wrong.kind = "correction_found".into();
        assert!(scope_hash(&wrong, value.hub, [12; 32], &bytes).is_err());
        assert!(scope_hash(&operation(), value.hub, [12; 32], &[]).is_err());
    }
}
