//! Journal slots remain reserved for outstanding hold and issued-document closure.
use super::super::super::{reference, stock};
use super::*;
use crate::db::corrupt_storage;
use harvestcircle_domain::hub::{
    stock::HUB_STOCK_EVENT_CAPACITY, treatment::HUB_TREATMENT_CAPACITY,
};
use sqlx::Row;

pub(in crate::hub) enum Reservation {
    Ordinary(u32),
    PlaceHold,
    ReleaseHold,
    OpenIssued,
    CloseIssued,
    Depart(u32),
}

pub(in crate::hub) async fn ensure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: &DurableRequestId,
    reservation: Reservation,
) -> Result<(), SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    // Known original replay remains available at capacity; the handler verifies intent.
    if stock::read::operation_on(tx, hub.id(), operation)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let counters = counters_on(tx, hub.id()).await?;
    check_budget(&counters, reservation)
}

/// Checks a new document-specific departure after original replay is resolved.
/// Read-only reviews reuse this exact reservation arithmetic without admitting intent.
pub(in crate::hub) async fn ensure_departure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    events: u32,
) -> Result<(), SafeError> {
    check_budget(&counters_on(tx, hub).await?, Reservation::Depart(events))
}

fn check_budget(counters: &Counters, reservation: Reservation) -> Result<(), SafeError> {
    let (events, new_reservations, issued_reservations, safety_headroom) = match reservation {
        Reservation::Ordinary(events) => {
            if events == 0 || events as usize > HUB_STOCK_EVENT_CAPACITY {
                return Err(invalid_input());
            }
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            // A precautionary hold and its release must still fit after normal work.
            (events, 0, 0, 2)
        }
        Reservation::PlaceHold => {
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            (1, 1, 0, 0)
        }
        Reservation::ReleaseHold => {
            if counters.open == 0 {
                return Err(corrupt_storage());
            }
            (1, -1, 0, 0)
        }
        Reservation::OpenIssued => {
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            (1, 0, 1, 2)
        }
        Reservation::Depart(events) => {
            if events == 0
                || events as usize > harvestcircle_domain::hub::dispatch::HUB_DISPATCH_LINES
            {
                return Err(invalid_input());
            }
            if counters.issued == 0 {
                return Err(corrupt_storage());
            }
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            (events, 0, -1, 2)
        }
        Reservation::CloseIssued => {
            if counters.issued == 0 {
                return Err(corrupt_storage());
            }
            (1, 0, -1, 0)
        }
    };
    let reserved = i64::from(counters.events)
        + i64::from(counters.open)
        + i64::from(counters.issued)
        + i64::from(events)
        + i64::from(new_reservations)
        + i64::from(issued_reservations)
        + i64::from(safety_headroom);
    if reserved > HUB_STOCK_EVENT_CAPACITY as i64 {
        return Err(capacity());
    }
    Ok(())
}

pub(in crate::hub) async fn available_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<bool, SafeError> {
    let counters = counters_on(tx, hub).await?;
    Ok(check_budget(&counters, Reservation::Ordinary(1)).is_ok())
}

struct Counters {
    primary: u32,
    open: u32,
    issued: u32,
    events: u32,
}

async fn counters_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Counters, SafeError> {
    let stock = stock::read::state_on(tx, hub).await?;
    let row=sqlx::query(concat!(
        "SELECT count(CASE WHEN t.kind<>'hold_release' THEN 1 END) AS primary_count,",
        "count(CASE WHEN t.kind='hold_place' THEN 1 END) AS placed,count(CASE WHEN t.kind='hold_release' THEN 1 END) AS released,",
        "count(CASE WHEN o.hub_id<>? OR t.kind<>o.kind OR t.record_id<>o.result_id OR (t.kind='hold_release' AND (held.kind IS NULL OR held.kind<>'hold_place' OR held.trace_id<>t.trace_id OR placed.stock_revision>=o.stock_revision)) THEN 1 END) AS invalid ",
        "FROM hub_treatment_records t JOIN hub_stock_operations o ON o.operation_id=t.operation_id ",
        "LEFT JOIN hub_treatment_records held ON held.record_id=t.hold_id LEFT JOIN hub_stock_operations placed ON placed.operation_id=held.operation_id"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let read = |name: &str| -> Result<u32, SafeError> {
        u32::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let primary = read("primary_count")?;
    let placed = read("placed")?;
    let released = read("released")?;
    if primary > HUB_TREATMENT_CAPACITY as u32
        || placed > primary
        || released > placed
        || read("invalid")? != 0
    {
        return Err(corrupt_storage());
    }
    let open = placed - released;
    let issued = issued_on(tx, hub).await?;
    if u64::from(stock.event_count) + u64::from(open) + u64::from(issued)
        > HUB_STOCK_EVENT_CAPACITY as u64
    {
        return Err(corrupt_storage());
    }
    Ok(Counters {
        primary,
        open,
        issued,
        events: stock.event_count,
    })
}

async fn issued_on(tx: &mut ServiceSqliteTransaction<'_>, hub: HubId) -> Result<u32, SafeError> {
    let operations:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_operations WHERE kind IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused')")
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let bodies: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_dispatch_changes")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if operations != bodies {
        return Err(corrupt_storage());
    }
    let roots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_dispatch_changes WHERE kind='dispatch_prepare'",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let row=sqlx::query(concat!(
        "SELECT count(*) AS heads,count(CASE WHEN c.kind IN ('dispatch_issue','dispatch_replace') AND used.operation_id IS NULL THEN 1 END) AS issued,",
        "count(CASE WHEN o.hub_id<>? OR o.result_id<>c.preparation_id OR o.kind<>c.kind OR m.operation_id IS NULL OR m.preparation_id<>c.preparation_id OR recipient.record_id IS NULL OR ",
        "(used.operation_id IS NOT NULL AND (c.kind NOT IN ('dispatch_issue','dispatch_replace') OR c.identification_id IS NULL OR used.identification_id<>c.identification_id OR used.preparation_revision<>c.record_revision OR actual.operation_id IS NULL OR actual.hub_id<>o.hub_id OR actual.kind<>'departure' OR actual.stock_revision<=o.stock_revision)) OR ",
        "(c.kind IN ('dispatch_issue','dispatch_replace') AND (i.identification_id IS NULL OR i.preparation_id<>c.preparation_id OR i.issued_operation_id<>c.operation_id OR i.material_operation_id<>c.material_operation_id)) THEN 1 END) AS invalid ",
        "FROM hub_dispatch_changes c JOIN hub_stock_operations o ON o.operation_id=c.operation_id LEFT JOIN hub_dispatch_identifications i ON i.identification_id=c.identification_id ",
        "LEFT JOIN hub_dispatch_materials m ON m.operation_id=c.material_operation_id LEFT JOIN hub_reference_versions recipient ON recipient.record_id=m.recipient_id AND recipient.record_revision=m.recipient_revision ",
        "LEFT JOIN hub_departures used ON used.preparation_id=c.preparation_id LEFT JOIN hub_stock_operations actual ON actual.operation_id=used.operation_id ",
        "WHERE NOT EXISTS (SELECT 1 FROM hub_dispatch_changes newer WHERE newer.preparation_id=c.preparation_id AND newer.record_revision>c.record_revision)"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let heads: i64 = row.try_get("heads").map_err(|_| corrupt_storage())?;
    let issued: i64 = row.try_get("issued").map_err(|_| corrupt_storage())?;
    if !(0..=harvestcircle_domain::hub::dispatch::HUB_DISPATCH_CAPACITY as i64).contains(&roots)
        || heads != roots
        || !(0..=roots).contains(&issued)
        || row
            .try_get::<i64, _>("invalid")
            .map_err(|_| corrupt_storage())?
            != 0
    {
        return Err(corrupt_storage());
    }
    u32::try_from(issued).map_err(|_| corrupt_storage())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_budget_keeps_precautionary_hold_and_all_existing_releases_possible() {
        let maximum = HUB_STOCK_EVENT_CAPACITY as u32;
        let before = Counters {
            events: maximum - 3,
            open: 0,
            issued: 0,
            primary: 0,
        };
        check_budget(&before, Reservation::Ordinary(1)).unwrap();
        let last_precaution = Counters {
            events: maximum - 2,
            open: 0,
            issued: 0,
            primary: 0,
        };
        assert_eq!(
            check_budget(&last_precaution, Reservation::Ordinary(1))
                .unwrap_err()
                .code(),
            SafeErrorCode::HubCapacity
        );
        check_budget(&last_precaution, Reservation::PlaceHold).unwrap();
        let held = Counters {
            events: maximum - 1,
            open: 1,
            issued: 0,
            primary: HUB_TREATMENT_CAPACITY as u32,
        };
        check_budget(&held, Reservation::ReleaseHold).unwrap();
        assert!(check_budget(&held, Reservation::Ordinary(1)).is_err());
        assert!(check_budget(&held, Reservation::PlaceHold).is_err());
        let two = Counters {
            events: maximum - 2,
            open: 2,
            issued: 0,
            primary: 2,
        };
        check_budget(&two, Reservation::ReleaseHold).unwrap();
        check_budget(
            &Counters {
                events: maximum - 1,
                open: 1,
                issued: 0,
                primary: 2,
            },
            Reservation::ReleaseHold,
        )
        .unwrap();
        assert!(
            check_budget(
                &Counters {
                    events: maximum,
                    open: 0,
                    issued: 0,
                    primary: 2
                },
                Reservation::Ordinary(1)
            )
            .is_err()
        );
        assert!(check_budget(&before, Reservation::Ordinary(0)).is_err());
        assert!(check_budget(&before, Reservation::Ordinary(maximum + 1)).is_err());
    }
    #[test]
    fn issued_documents_reserve_unused_closure_without_spending_hold_release_slots() {
        let maximum = HUB_STOCK_EVENT_CAPACITY as u32;
        let before = Counters {
            events: maximum - 4,
            open: 0,
            issued: 0,
            primary: 0,
        };
        check_budget(&before, Reservation::OpenIssued).unwrap();
        let full = Counters {
            events: maximum - 1,
            open: 0,
            issued: 1,
            primary: HUB_TREATMENT_CAPACITY as u32,
        };
        check_budget(&full, Reservation::CloseIssued).unwrap();
        assert!(check_budget(&full, Reservation::Ordinary(1)).is_err());
        assert!(check_budget(&full, Reservation::OpenIssued).is_err());
        let both = Counters {
            events: maximum - 2,
            open: 1,
            issued: 1,
            primary: 1,
        };
        check_budget(&both, Reservation::ReleaseHold).unwrap();
        check_budget(&both, Reservation::CloseIssued).unwrap();
        assert!(check_budget(&both, Reservation::Ordinary(1)).is_err());
        assert!(check_budget(&both, Reservation::PlaceHold).is_err());
        assert!(check_budget(&before, Reservation::CloseIssued).is_err());
    }
}

#[cfg(test)]
mod departure_review_tests {
    use super::*;
    #[test]
    fn an_issued_departure_uses_its_reserved_closure_while_generic_work_stays_blocked() {
        let counters = Counters {
            primary: 0,
            open: 0,
            issued: 1,
            events: HUB_STOCK_EVENT_CAPACITY as u32 - 3,
        };
        assert_eq!(
            check_budget(&counters, Reservation::Ordinary(1))
                .unwrap_err()
                .code(),
            SafeErrorCode::HubCapacity
        );
        check_budget(&counters, Reservation::Depart(1)).unwrap();
        assert_eq!(
            check_budget(&counters, Reservation::Depart(2))
                .unwrap_err()
                .code(),
            SafeErrorCode::HubCapacity
        );
    }
}
