//! Canonical return reservations remain tied to exact original physical debits.
//! Unlinked same-variant custody is uncertainty, never an allocated deduction.
use super::quantity::CanonicalLine;
use super::*;
use harvestcircle_domain::hub::{
    corrections::{EstablishedReturnReservation, PhysicalDebitKey},
    reference::WholePacks,
    returns::ReturnLinkResolution,
    stock::HUB_TRACE_CAPACITY,
};
use sha2::{Digest, Sha256};

pub(super) struct ReturnReservations {
    pub established: Vec<EstablishedReturnReservation>,
    pub unresolved: bool,
    pub total: WholePacks,
    pub evidence_digest: [u8; 32],
}

/// Verify every return through the cut before selecting its canonical variant,
/// including those recorded
/// before a historical departure was reviewed. Current attribution/party labels
/// do not prove that an unresolved return belongs to a disjoint physical debit.
pub(super) async fn returns_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    line: &CanonicalLine,
    cut: HubRevision,
) -> Result<ReturnReservations, SafeError> {
    if line.original.stock_revision.value() > cut.value() {
        return Err(corrupt_storage());
    }
    let ids: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT substr(original.operation_id,1,37) FROM hub_returns returned ",
        "JOIN hub_stock_operations original ON original.operation_id=returned.operation_id ",
        "WHERE original.hub_id=? AND original.stock_revision<=? ",
        "ORDER BY original.stock_revision LIMIT 4097"
    ))
    .bind(hub.as_bytes().as_slice())
    .bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if ids.len() > HUB_TRACE_CAPACITY {
        return Err(stock::capacity());
    }
    let mut result = ReturnReservations {
        established: Vec::new(),
        unresolved: false,
        total: WholePacks::from_value(0).expect("zero"),
        evidence_digest: [0; 32],
    };
    let mut evidence = Sha256::new();
    evidence.update(b"harvestcircle.hub.return.funding.evidence.v1\0");
    evidence.update(hub.as_bytes());
    evidence.update(line.original.operation.as_bytes());
    evidence.update(line.original.group.as_bytes());
    evidence.update(line.original.variant.as_bytes());
    evidence.update(cut.value().to_be_bytes());
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "return" || op.revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        let revision = op.revision;
        let recorded_at = op.recorded_at;
        let input_hash = op.hash;
        let actor = op.actor;
        let catalog_revision = op.catalog_revision;
        let result_id = op.result;
        let (operation, variant, packs, established, related) =
            if let Some(projected) = super::return_projection::read_on(tx, hub, &op).await? {
                if projected.stock_revision != revision || projected.recorded_at != recorded_at {
                    return Err(corrupt_storage());
                }
                if projected.established_group.is_some()
                    && projected.established_group != projected.claim_group
                {
                    return Err(corrupt_storage());
                }
                (
                    projected.operation,
                    projected.variant,
                    projected.packs,
                    projected
                        .established_group
                        .map(|group| (projected.known_operation, projected.known_departure, group)),
                    projected.claim_departure == line.result
                        || projected.known_operation == line.original.operation,
                )
            } else {
                // Only an absent capture AND output projection may take this path;
                // the exact None full result seal must still verify. Old originals
                // never recurse through current allowance or funding reconstruction.
                let original = Box::pin(crate::hub::custody::returns::read::legacy_original_on(
                    tx, hub, op,
                ))
                .await?
                .original;
                if original.stock_revision != revision {
                    return Err(corrupt_storage());
                }
                let established =
                    if original.linkage.resolution == ReturnLinkResolution::EstablishedScope {
                        let known = original
                            .linkage
                            .departure
                            .as_ref()
                            .ok_or_else(corrupt_storage)?;
                        Some((
                            known.operation,
                            known.departure,
                            original
                                .linkage
                                .established_group()
                                .ok_or_else(corrupt_storage)?,
                        ))
                    } else {
                        None
                    };
                (
                    original.identities.operation,
                    original.intake.variant.id,
                    original.intake.packs,
                    established,
                    original
                        .linkage
                        .claim
                        .is_some_and(|claim| claim.departure == line.result)
                        || original
                            .linkage
                            .departure
                            .as_ref()
                            .is_some_and(|known| known.operation == line.original.operation),
                )
            };
        if variant != line.original.variant && !related {
            continue;
        }
        // Commit every relevant immutable original, including uncertain and
        // disjoint established scopes; scalar totals alone are not provenance.
        let full: Vec<u8> = sqlx::query_scalar(
            "SELECT substr(result_hash,1,33) FROM hub_returns WHERE operation_id=?",
        )
        .bind(id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
        let full: [u8; 32] = full.try_into().map_err(|_| corrupt_storage())?;
        let projection:Option<Vec<u8>>=sqlx::query_scalar("SELECT substr(projection_hash,1,33) FROM hub_correction_return_projections WHERE operation_id=?").bind(id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
        evidence.update(id.as_str().as_bytes());
        evidence.update(input_hash);
        evidence.update(actor.as_bytes());
        evidence.update(revision.value().to_be_bytes());
        evidence.update(catalog_revision.value().to_be_bytes());
        evidence.update(recorded_at.as_seconds().to_be_bytes());
        evidence.update(result_id.as_bytes());
        evidence.update(full);
        evidence.update([u8::from(projection.is_some())]);
        if let Some(projection) = projection {
            let projection: [u8; 32] = projection.try_into().map_err(|_| corrupt_storage())?;
            evidence.update(projection);
        }
        let Some((known_operation, known_departure, selected)) = established else {
            result.unresolved = true;
            continue;
        };
        if known_operation != line.original.operation || selected != line.original.group {
            // Canonical established linkage proves a disjoint original scope.
            continue;
        }
        if line.kind != "departure"
            || known_departure != line.result
            || variant != line.original.variant
        {
            return Err(corrupt_storage());
        }
        if revision.value() <= line.original.stock_revision.value()
            || recorded_at < line.recorded_at
        {
            result.unresolved = true;
            continue;
        }
        let reservation = EstablishedReturnReservation {
            operation,
            debit: PhysicalDebitKey {
                operation: line.original.operation,
                group: line.original.group,
            },
            packs,
            stock_revision: revision,
            recorded_at,
        };
        result.total = result
            .total
            .checked_add(reservation.packs)
            .map_err(|_| corrupt_storage())?;
        result.established.push(reservation);
    }
    result.evidence_digest = evidence.finalize().into();
    Ok(result)
}
