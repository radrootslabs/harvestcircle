use super::super::{treatment::capacity as budget, write as journal};
use super::*;
use crate::hub::{reference, stock};
use crate::{Database, db::map_transaction_error};
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchContext, HUB_DISPATCH_CAPACITY, dispatch_result_digest,
};

impl Database {
    pub async fn apply_hub_dispatch(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: DispatchCommand,
        now: UnixTimestamp,
    ) -> Result<HubDispatchReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move { apply_on(tx, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: DispatchCommand,
    now: UnixTimestamp,
) -> Result<HubDispatchReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let identities = identities(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, identities);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.actor != actor || original.kind != command.kind() || original.hash != hash {
            return Err(conflict());
        }
        return read::original_on(tx, hub.id(), original).await;
    }
    let state = stock::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let current = match command.action.target() {
        Some((id, _)) => Some(read::record_on(tx, hub.id(), id).await?),
        None => None,
    };
    if matches!(command.action, DispatchAction::Prepare { .. }) {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM hub_dispatch_changes WHERE kind='dispatch_prepare'",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
        if !(0..HUB_DISPATCH_CAPACITY as i64).contains(&count) {
            return Err(capacity());
        }
        if read::head_on(tx, hub.id(), identities.preparation)
            .await?
            .is_some()
        {
            return Err(conflict());
        }
    }
    let selection = match &command.action {
        DispatchAction::Issue { .. } => Some(
            current
                .as_ref()
                .and_then(|record| record.draft.as_ref())
                .ok_or_else(conflict)?
                .selection()
                .map_err(|_| corrupt())?,
        ),
        _ => command.action.selection().cloned(),
    };
    let sources = match selection.as_ref() {
        Some(selection) => {
            material::sources_on(tx, hub.id(), &catalog, state.revision, selection).await?
        }
        None => vec![],
    };
    let context = DispatchContext {
        hub: hub.id(),
        stock_revision: state.revision,
        catalog: &catalog,
        sources: &sources,
        current: current.as_ref(),
    };
    let plan = command
        .plan(&context, identities, actor, now)
        .map_err(dispatch_error)?;
    let kind = HubDispatchKind::parse(command.kind())?;
    let reservation = match kind {
        HubDispatchKind::Issue => budget::Reservation::OpenIssued,
        HubDispatchKind::MarkUnused => budget::Reservation::CloseIssued,
        _ => budget::Reservation::Ordinary(1),
    };
    budget::ensure_on(tx, actor, &operation, reservation).await?;
    let snapshot = plan
        .record
        .draft
        .as_ref()
        .or_else(|| plan.record.versions.last().map(|version| &version.material))
        .ok_or_else(corrupt)?;
    let material_operation = if matches!(
        kind,
        HubDispatchKind::Prepare | HubDispatchKind::Revise | HubDispatchKind::Replace
    ) {
        operation.clone()
    } else {
        read::head_on(tx, hub.id(), plan.record.id)
            .await?
            .ok_or_else(corrupt)?
            .material
    };
    let original = stock::read::OperationRecord {
        id: operation.clone(),
        kind: command.kind().to_owned(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision: plan.stock_revision,
        result: plan.record.id,
        recorded_at: now,
    };
    journal::insert_operation_on(tx, hub.id(), &original).await?;
    if material_operation == operation {
        material::insert_on(tx, &operation, plan.record.id, snapshot).await?;
    }
    let (identification, previous) = match kind {
        HubDispatchKind::Issue | HubDispatchKind::Replace => (
            Some(plan.record.versions.last().ok_or_else(corrupt)?.id),
            current
                .as_ref()
                .and_then(|record| record.versions.last())
                .map(|version| version.id),
        ),
        HubDispatchKind::MarkUnused => {
            (None, plan.record.versions.last().map(|version| version.id))
        }
        _ => (None, None),
    };
    let digest = dispatch_result_digest(&hash, &plan.record).map_err(|_| corrupt())?;
    sqlx::query(concat!(
        "INSERT INTO hub_dispatch_changes (operation_id,preparation_id,record_revision,expected_record_revision,kind,material_operation_id,identification_id,previous_identification_id,observation,result_hash) ",
        "VALUES (?,?,?,?,?,?,?,?,?,?)"
    )).bind(operation.as_str()).bind(plan.record.id.as_bytes().as_slice()).bind(integer(plan.record.revision)?)
        .bind(command.action.target().map(|(_,revision)|integer(revision)).transpose()?).bind(command.kind()).bind(material_operation.as_str())
        .bind(identification.as_ref().map(|id|id.as_bytes().as_slice())).bind(previous.as_ref().map(|id|id.as_bytes().as_slice()))
        .bind(command.observation.as_str()).bind(digest.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if matches!(kind, HubDispatchKind::Issue | HubDispatchKind::Replace) {
        let version = plan.record.versions.last().ok_or_else(corrupt)?;
        sqlx::query("INSERT INTO hub_dispatch_identifications (identification_id,preparation_id,version_number,previous_identification_id,issued_operation_id,material_operation_id) VALUES (?,?,?,?,?,?)")
            .bind(version.id.as_bytes().as_slice()).bind(plan.record.id.as_bytes().as_slice()).bind(i64::from(version.number)).bind(version.previous.as_ref().map(|id|id.as_bytes().as_slice()))
            .bind(operation.as_str()).bind(material_operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    let first = snapshot.lines.first().ok_or_else(corrupt)?;
    journal::event_on(
        tx,
        &operation,
        journal::EventEffect {
            kind: command.kind(),
            prior_events: state.event_count,
            trace: first.trace.id,
            group: first.group,
            packs: WholePacks::from_value(0).map_err(|_| corrupt())?,
            from: None,
            to: None,
            occurred: Occurrence::Unknown,
        },
    )
    .await?;
    journal::advance_on(tx, hub.id(), &state, plan.stock_revision).await?;
    let restored = read::original_on(tx, hub.id(), original).await?;
    if restored.record != plan.record {
        return Err(corrupt());
    }
    Ok(restored)
}
