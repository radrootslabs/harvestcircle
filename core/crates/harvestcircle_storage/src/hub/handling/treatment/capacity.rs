//! One journal slot remains reserved for each outstanding hold release.
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

fn check_budget(counters: &Counters, reservation: Reservation) -> Result<(), SafeError> {
    let (events, new_reservations, safety_headroom) = match reservation {
        Reservation::Ordinary(events) => {
            if events == 0 || events as usize > HUB_STOCK_EVENT_CAPACITY {
                return Err(invalid_input());
            }
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            // A precautionary hold and its release must still fit after normal work.
            (events, 0, 2)
        }
        Reservation::PlaceHold => {
            if counters.primary >= HUB_TREATMENT_CAPACITY as u32 {
                return Err(capacity());
            }
            (1, 1, 0)
        }
        Reservation::ReleaseHold => {
            if counters.open == 0 {
                return Err(corrupt_storage());
            }
            (1, -1, 0)
        }
    };
    let reserved = i64::from(counters.events)
        + i64::from(counters.open)
        + i64::from(events)
        + i64::from(new_reservations)
        + i64::from(safety_headroom);
    if reserved > HUB_STOCK_EVENT_CAPACITY as i64 {
        return Err(capacity());
    }
    Ok(())
}

pub(super) async fn available_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<bool, SafeError> {
    let counters = counters_on(tx, hub).await?;
    Ok(check_budget(&counters, Reservation::Ordinary(1)).is_ok())
}

struct Counters {
    primary: u32,
    open: u32,
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
    if u64::from(stock.event_count) + u64::from(open) > HUB_STOCK_EVENT_CAPACITY as u64 {
        return Err(corrupt_storage());
    }
    Ok(Counters {
        primary,
        open,
        events: stock.event_count,
    })
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
            primary: 0,
        };
        check_budget(&before, Reservation::Ordinary(1)).unwrap();
        let last_precaution = Counters {
            events: maximum - 2,
            open: 0,
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
            primary: HUB_TREATMENT_CAPACITY as u32,
        };
        check_budget(&held, Reservation::ReleaseHold).unwrap();
        assert!(check_budget(&held, Reservation::Ordinary(1)).is_err());
        assert!(check_budget(&held, Reservation::PlaceHold).is_err());
        let two = Counters {
            events: maximum - 2,
            open: 2,
            primary: 2,
        };
        check_budget(&two, Reservation::ReleaseHold).unwrap();
        check_budget(
            &Counters {
                events: maximum - 1,
                open: 1,
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
                    primary: 2
                },
                Reservation::Ordinary(1)
            )
            .is_err()
        );
        assert!(check_budget(&before, Reservation::Ordinary(0)).is_err());
        assert!(check_budget(&before, Reservation::Ordinary(maximum + 1)).is_err());
    }
}
