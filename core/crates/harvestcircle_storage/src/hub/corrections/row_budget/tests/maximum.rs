//! Lower native inventory boundary, deliberately NOT 4096 fully authenticated
//! source originals or a maximum application-writer qualification. Seed only
//! closed commands, native headers and serializer-built admission identities;
//! full source captures/results remain the original reader's separate gate.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmissionContext, HubSubmittedRequest};

async fn append_inventory_roots(db: &Database, trace: HubRecordId, amount: u16) -> HubRevision {
    db.host().transaction(|tx| Box::pin(async move {
        let hub = reference::authorize_on(tx, owner()).await?.id();
        let first = stock::read::operation_on(tx, hub, &operation(1000)).await?.unwrap();
        let row: Vec<u8> = sqlx::query_scalar("SELECT payload FROM hub_corrections WHERE operation_id=?")
            .bind(first.id.as_str()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        let template = super::super::super::source_payload::decode(&row)?;
        let captured: Vec<u8> = sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
            .bind(first.id.as_str()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        let metadata = harvestcircle_application::hub_submission::inspect_hub_submission(&captured).unwrap();
        let mut state = stock::read::journal_state_on(tx, hub).await?;
        let mut previous = crate::hub::actuals::common::operation_id(&first.id)?;
        // The existing ordinary root plus these records reaches the exact cut.
        let target = operation(1);
        for n in 0..amount {
            if n == 4094 {
                assert_eq!(through_on(tx, hub, trace, state.revision).await?, 4095);
                ensure_on(tx, hub, trace).await?;
            }
            if n == 4095 {
                assert_eq!(ensure_on(tx, hub, trace).await.unwrap_err().code(), SafeErrorCode::HubCapacity);
            }
            let operation = operation(2000 + u32::from(n));
            let own = crate::hub::actuals::common::operation_id(&operation)?;
            let mut command = template.clone();
            command.trace = trace;
            command.expected_stock_revision = state.revision;
            command.expected_prior = Some(previous);
            let request = HubSubmittedRequest::SourceAttribution(&command);
            let input = request.archive().unwrap();
            let capture = request.capture(&operation, metadata.scope, &HubSubmissionContext {
                hub: metadata.hub,
                catalog_revision: Some(command.expected_catalog_revision),
                stock_revision: Some(state.revision),
                material: &[],
            }).unwrap();
            let hash = command.fingerprint(hub, owner(), own);
            sqlx::query(concat!(
                "INSERT INTO hub_submissions(operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) ",
                "SELECT ?1,hub_id,actor_public_key,kind,runtime_id,generation,?2,?3,1,?4,?5,submitted_at_unix_s FROM hub_submissions WHERE operation_id=?6"
            )).bind(operation.as_str()).bind(input.digest().as_slice()).bind(hash.as_slice())
                .bind(capture.as_bytes()).bind(capture.digest().as_slice()).bind(first.id.as_str())
                .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
            let revision = HubRevision::from_value(state.revision.value()+1).unwrap();
            let op = OperationRecord { id: operation.clone(), kind: first.kind.clone(), actor: owner(), hash,
                catalog_revision: command.expected_catalog_revision, revision, result: own, recorded_at: at(300+i64::from(n)) };
            handling::write::insert_operation_on(tx, hub, &op).await?;
            sqlx::query(concat!(
                "INSERT INTO hub_corrections(correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,payload_schema,payload,result_hash) ",
                "VALUES (?,?,'source',?,'population',?,?,1,?,?)"
            )).bind(own.as_bytes().as_slice()).bind(operation.as_str()).bind(target.as_str())
                .bind(trace.as_bytes().as_slice()).bind(crate::hub::actuals::common::operation(previous)?.as_str())
                .bind(super::super::super::source_payload::encode(&command)?).bind([1_u8;32].as_slice())
                .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
            sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1")
                .bind(i64::try_from(revision.value()).unwrap()).execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
            state.revision = revision;
            previous = own;
        }
        Ok(state.revision)
    })).await.map_err(map_transaction_error).unwrap()
}

async fn room(db: &Database, trace: HubRecordId) -> Result<(), SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                ensure_on(tx, hub, trace).await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn lower_inventory_4095_4096_and_overfull_are_per_trace_and_preserve_original_cuts() {
    let (_fixture, db, a, b, _before, first_cut) = source_world().await;
    let almost = append_inventory_roots(&db, a, 4094).await;
    assert_eq!(count(&db, a, almost).await.unwrap(), 4095);
    room(&db, a).await.unwrap();
    room(&db, b).await.unwrap();
    // Continue the same disposable inventory with the remaining closed records.
    // A second seeding call would reuse IDs; append both final rows in one seed
    // world below, preserving the exact earlier-cut observations here.
    db.close().await.unwrap();

    let (_fixture, db, a, b, _before, _) = source_world().await;
    let overfull = append_inventory_roots(&db, a, 4096).await;
    let full = HubRevision::from_value(overfull.value() - 1).unwrap();
    assert_eq!(count(&db, a, first_cut).await.unwrap(), 1);
    assert_eq!(count(&db, a, full).await.unwrap(), 4096);
    assert_eq!(count(&db, b, full).await.unwrap(), 0);
    assert_eq!(
        count(&db, a, overfull).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    // The other population still has no corrections; the global enumeration
    // must not impose one population's per-trace total on it.
    room(&db, b).await.unwrap();
    assert_eq!(
        room(&db, a).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    // The reader memo is only a lower ownership inventory proof, just like
    // this deliberately seeded fixture. It is not 4096 authenticated originals.
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                let almost = HubRevision::from_value(full.value() - 1).unwrap();
                for limit in [0, 1, 4096] {
                    let mut reader = OriginalReader::new(tx);
                    reader.test_count_row_limit(limit);
                    assert_eq!(reader.correction_rows_on(hub, a, almost).await?, 4095);
                    assert_eq!(reader.correction_rows_on(hub, a, full).await?, 4096);
                    let checkpoint = reader.test_count_row_points();
                    for _ in 0..2 {
                        assert_eq!(
                            reader
                                .correction_rows_on(hub, a, overfull)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        assert_eq!(reader.test_count_row_points(), checkpoint);
                    }
                    assert_eq!(reader.correction_rows_on(hub, a, almost).await?, 4095);
                    assert_eq!(reader.correction_rows_on(hub, a, full).await?, 4096);
                    assert_eq!(reader.correction_rows_on(hub, b, overfull).await?, 0);
                    assert_eq!(reader.correction_rows_on(hub, a, first_cut).await?, 1);
                    assert!(reader.test_count_row_points().len() <= limit);
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    db.close().await.unwrap();
}
