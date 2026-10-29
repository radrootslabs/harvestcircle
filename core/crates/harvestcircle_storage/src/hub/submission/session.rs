//! One durable current session fence; archived correlation never becomes authority.
use super::*;
use harvestcircle_application::{SessionGeneration, hub_submission::HubRuntimeIncarnation};
use harvestcircle_domain::UnixTimestamp;
use sqlx::Row;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubRuntimeSession {
    pub runtime: HubRuntimeIncarnation,
    pub generation: SessionGeneration,
    pub actor: Option<PublicKey>,
}

impl Database {
    /// Generates a fresh native incarnation before SQL and starts signed out.
    pub async fn begin_hub_runtime(
        &self,
        now: UnixTimestamp,
    ) -> Result<HubRuntimeIncarnation, SafeError> {
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes).map_err(|_| storage_unavailable())?;
        let runtime =
            HubRuntimeIncarnation::from_bytes(bytes).map_err(|_| storage_unavailable())?;
        self.host().transaction(|tx| Box::pin(async move {
            sqlx::query(concat!(
                "INSERT INTO hub_runtime_session (singleton,runtime_id,generation,actor_public_key,updated_at_unix_s) VALUES (1,?,?,NULL,?) ",
                "ON CONFLICT(singleton) DO UPDATE SET runtime_id=excluded.runtime_id,generation=excluded.generation,actor_public_key=NULL,",
                "updated_at_unix_s=MAX(hub_runtime_session.updated_at_unix_s,excluded.updated_at_unix_s)"
            )).bind(runtime.as_bytes().as_slice()).bind(0_u64.to_be_bytes().as_slice()).bind(now.as_seconds())
                .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
            Ok(runtime)
        })).await.map_err(map_transaction_error)
    }

    /// Advances exactly once before the runtime publishes a successful session change.
    pub async fn advance_hub_session(
        &self,
        runtime: HubRuntimeIncarnation,
        prior: SessionGeneration,
        actor: Option<PublicKey>,
        now: UnixTimestamp,
    ) -> Result<SessionGeneration, SafeError> {
        let next = prior.next().ok_or_else(stock::capacity)?;
        self.host().transaction(|tx|Box::pin(async move {
            let changed=sqlx::query(concat!(
                "UPDATE hub_runtime_session SET generation=?,actor_public_key=?,updated_at_unix_s=MAX(updated_at_unix_s,?) ",
                "WHERE singleton=1 AND runtime_id=? AND generation=?"
            )).bind(next.value().to_be_bytes().as_slice()).bind(actor.as_ref().map(|key|key.as_bytes().as_slice()))
                .bind(now.as_seconds()).bind(runtime.as_bytes().as_slice()).bind(prior.value().to_be_bytes().as_slice())
                .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if changed.rows_affected()!=1 {return Err(stock::conflict());}
            Ok(next)
        })).await.map_err(map_transaction_error)
    }

    pub async fn hub_runtime_session(&self) -> Result<Option<HubRuntimeSession>, SafeError> {
        self.host()
            .transaction(|tx| Box::pin(async move { current_on(tx).await }))
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn current_on(
    tx: &mut ServiceSqliteTransaction<'_>,
) -> Result<Option<HubRuntimeSession>, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(runtime_id,1,17) AS runtime_id,length(runtime_id) AS runtime_bytes,",
        "substr(generation,1,9) AS generation,length(generation) AS generation_bytes,",
        "substr(actor_public_key,1,33) AS actor,length(actor_public_key) AS actor_bytes FROM hub_runtime_session WHERE singleton=1 LIMIT 1"
    )).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let Some(row) = row else { return Ok(None) };
    if row
        .try_get::<i64, _>("runtime_bytes")
        .map_err(|_| corrupt_storage())?
        != 16
        || row
            .try_get::<i64, _>("generation_bytes")
            .map_err(|_| corrupt_storage())?
            != 8
    {
        return Err(corrupt_storage());
    }
    let runtime: Vec<u8> = row.try_get("runtime_id").map_err(|_| corrupt_storage())?;
    let generation: Vec<u8> = row.try_get("generation").map_err(|_| corrupt_storage())?;
    let actor: Option<Vec<u8>> = row.try_get("actor").map_err(|_| corrupt_storage())?;
    let actor = match actor {
        None => None,
        Some(bytes)
            if row
                .try_get::<Option<i64>, _>("actor_bytes")
                .map_err(|_| corrupt_storage())?
                == Some(32) =>
        {
            Some(
                PublicKey::from_bytes(bytes.try_into().map_err(|_| corrupt_storage())?)
                    .map_err(|_| corrupt_storage())?,
            )
        }
        Some(_) => return Err(corrupt_storage()),
    };
    Ok(Some(HubRuntimeSession {
        runtime: HubRuntimeIncarnation::from_bytes(
            runtime.try_into().map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?,
        generation: SessionGeneration::from_value(u64::from_be_bytes(
            generation.try_into().map_err(|_| corrupt_storage())?,
        )),
        actor,
    }))
}

pub(super) async fn require_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    scope: HubSubmissionScope,
) -> Result<(), SafeError> {
    let current = current_on(tx)
        .await?
        .ok_or_else(reference::hub_permission_denied)?;
    if current.runtime != scope.runtime
        || current.generation != scope.generation
        || current.actor != Some(scope.actor)
    {
        return Err(reference::hub_permission_denied());
    }
    Ok(())
}
