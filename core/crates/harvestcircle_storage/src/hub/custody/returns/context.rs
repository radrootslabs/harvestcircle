//! Complete native originals and current validated association totals, not UI clear flags.
use super::*;
use std::collections::BTreeMap;

pub(in crate::hub) struct Context {
    pub state: stock::read::StockState,
    pub catalog: ReferenceCatalog,
    pub departure: Option<DepartureRecord>,
    pub totals: Vec<EstablishedReturnTotal>,
    pub current_allowances: Option<harvestcircle_domain::hub::returns::ReturnAllowanceCapture>,
}
impl Context {
    pub(in crate::hub) fn domain(&self, hub: HubId) -> ReturnContext<'_> {
        ReturnContext {
            hub,
            stock_revision: self.state.revision,
            catalog: &self.catalog,
            departure: self.departure.as_ref(),
            established_return_totals: &self.totals,
            current_allowances: self.current_allowances.as_ref(),
        }
    }
}
pub(in crate::hub) async fn departure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    departure: HubRecordId,
    before: HubRevision,
) -> Result<Option<DepartureRecord>, SafeError> {
    let operation: Option<String> = sqlx::query_scalar("SELECT substr(d.operation_id,1,37) FROM hub_departures d JOIN hub_stock_operations o ON o.operation_id=d.operation_id WHERE d.departure_id=? AND o.hub_id=? LIMIT 1")
        .bind(departure.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
    let original = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    if original.kind != "departure"
        || original.result != departure
        || original.revision.value() > before.value()
    {
        return Err(corrupt_storage());
    }
    Ok(Some(
        handling::departure::read::original_on(tx, hub, original)
            .await?
            .record,
    ))
}
pub(in crate::hub) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    command: &ReturnCommand,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
    {
        return Err(stock::conflict());
    }
    let departure = match command.departure {
        None => None,
        Some(claim) => departure_on(tx, hub, claim.departure, state.revision).await?,
    };
    // This is the existing schema16 pre-runtime compatibility mode, never a
    // fallback for missing admission once a runtime row exists.
    let runtime: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hub_runtime_session)")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if runtime == 0 {
        let corrections:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_corrections)+(SELECT count(*) FROM hub_correction_observations)+(SELECT count(*) FROM hub_correction_captures)+(SELECT count(*) FROM hub_correction_return_projections)").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if corrections != 0 {
            return Err(corrupt_storage());
        }
    }
    let (totals, current_allowances) = match &departure {
        None => (Vec::new(), None),
        Some(departure) if runtime == 0 => (
            validated_totals_on(tx, hub, departure, state.revision).await?,
            None,
        ),
        Some(departure) => {
            let captured = crate::hub::corrections::return_allowances::load_on(
                tx,
                hub,
                departure,
                state.revision,
            )
            .await?;
            (captured.totals, captured.allowances)
        }
    };
    Ok(Context {
        state,
        catalog,
        departure,
        totals,
        current_allowances,
    })
}

async fn validated_totals_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    departure: &DepartureRecord,
    cut: HubRevision,
) -> Result<Vec<EstablishedReturnTotal>, SafeError> {
    let lines = &departure
        .paper
        .versions
        .last()
        .ok_or_else(corrupt_storage)?
        .material
        .lines;
    let mut totals: BTreeMap<_, _> = lines
        .iter()
        .map(|line| {
            (
                line.group,
                harvestcircle_domain::hub::reference::WholePacks::from_value(0)
                    .expect("zero packs"),
            )
        })
        .collect();
    // Do not trust a resolution scalar or omit a damaged known/unrecorded marker.
    // Every original which claims or captures this departure is verified first.
    let rows=sqlx::query(concat!(
        "SELECT substr(o.operation_id,1,37) AS operation_id FROM hub_returns returned JOIN hub_stock_operations o ON o.operation_id=returned.operation_id ",
        "WHERE o.hub_id=? AND o.stock_revision<=? AND (returned.claimed_departure_id=? OR returned.known_departure_id=? OR EXISTS(SELECT 1 FROM hub_return_sources source WHERE source.return_id=returned.return_id AND source.departure_operation_id=(SELECT operation_id FROM hub_departures WHERE departure_id=?))) ",
        "ORDER BY o.stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(cut.value()).map_err(|_|corrupt_storage())?)
        .bind(departure.id.as_bytes().as_slice()).bind(departure.id.as_bytes().as_slice())
        .bind(departure.id.as_bytes().as_slice())
        .fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY {
        return Err(stock::capacity());
    }
    for row in rows {
        let id = DurableRequestId::parse(
            row.try_get::<String, _>("operation_id")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let prior = read::original_on(tx, hub, op).await?.original;
        if prior.linkage.resolution == ReturnLinkResolution::EstablishedScope {
            let known = prior
                .linkage
                .departure
                .as_ref()
                .ok_or_else(corrupt_storage)?;
            if known.departure != departure.id {
                return Err(corrupt_storage());
            }
            let selected = prior
                .linkage
                .established_group()
                .ok_or_else(corrupt_storage)?;
            let total = totals.get_mut(&selected).ok_or_else(corrupt_storage)?;
            *total = total
                .checked_add(prior.intake.packs)
                .map_err(|_| corrupt_storage())?;
        }
    }
    for line in lines {
        if totals.get(&line.group).ok_or_else(corrupt_storage)?.value() > line.packs.value() {
            return Err(corrupt_storage());
        }
    }
    Ok(totals
        .into_iter()
        .map(|(original_group, packs)| EstablishedReturnTotal {
            original_group,
            packs,
        })
        .collect())
}
