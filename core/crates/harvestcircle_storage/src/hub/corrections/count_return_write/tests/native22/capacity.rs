//! Explicit lower inventory for the fresh-admission global capacity regression.
//! Serializer-built identities retain all SQL guards; no complete source originals.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmissionContext, HubSubmittedRequest};
use harvestcircle_domain::hub::corrections::SourceAttributionCommand;
use sqlx::Connection;

pub(in crate::hub) async fn fill_hub_with_lower_source_inventory(world: &AssociationWorld) {
    let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
    let hub = world.db.load_hub().await.unwrap().unwrap();
    let mut raw = world.fixture.raw().await;
    let mut tx = raw.begin().await.unwrap();
    let corrections: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_corrections")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let associations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_count_return_associations")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!((corrections, associations), (3, 1));
    // These distinct lower rows preserve SQL/FK shape only. They have no complete
    // source captures/results and must never qualify successful native maximums.
    // A different physical population prevents target-trace capacity masking.
    for n in 0..4092_u32 {
        let id = operation(50000 + n);
        let identity = own(50000 + n);
        let cut = HubRevision::from_value(page.revision.value() + u64::from(n)).unwrap();
        let input = SourceAttributionCommand {
            trace: world.returned.original.identities.population,
            expected_stock_revision: cut,
            expected_catalog_revision: page.catalog_revision,
            expected_prior: (n > 0).then(|| own(50000 + n - 1)),
            source: SourceAttribution::Unknown,
            basis: evidence(),
        };
        let request = HubSubmittedRequest::SourceAttribution(&input);
        let archive = request.archive().unwrap();
        let capture = request
            .capture(
                &id,
                world.scope,
                &HubSubmissionContext {
                    hub,
                    catalog_revision: Some(page.catalog_revision),
                    stock_revision: Some(cut),
                    material: &[],
                },
            )
            .unwrap();
        let hash = input.fingerprint(hub.id(), owner(), identity);
        sqlx::query(concat!(
            "INSERT INTO hub_submissions(operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) ",
            "SELECT ?1,hub_id,actor_public_key,'correction_source',runtime_id,generation,?2,?3,1,?4,?5,submitted_at_unix_s FROM hub_submissions WHERE operation_id=?6"
        )).bind(id.as_str()).bind(archive.digest().as_slice()).bind(hash.as_slice())
            .bind(capture.as_bytes()).bind(capture.digest().as_slice()).bind(operation(900).as_str())
            .execute(&mut *tx).await.unwrap();
        sqlx::query(concat!(
            "INSERT INTO hub_stock_operations(operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) ",
            "SELECT ?,hub_id,actor_public_key,'correction_source',?,catalog_revision,?,?,recorded_at_unix_s FROM hub_stock_operations WHERE operation_id=?"
        )).bind(id.as_str()).bind(hash.as_slice()).bind(i64::try_from(page.revision.value()).unwrap()+1+i64::from(n))
            .bind(identity.as_bytes().as_slice()).bind(operation(1000).as_str())
            .execute(&mut *tx).await.unwrap();
        sqlx::query(concat!(
            "INSERT INTO hub_corrections(correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,payload_schema,payload,result_hash) ",
            "VALUES (?,?,'source',?,'population',?,?,1,?,?)"
        )).bind(identity.as_bytes().as_slice()).bind(id.as_str()).bind(operation(900).as_str())
            .bind(world.returned.original.identities.population.as_bytes().as_slice())
            .bind((n>0).then(|| operation(50000+n-1).as_str().to_owned()))
            .bind(crate::hub::corrections::source_payload::encode(&input).unwrap())
            .bind([1_u8;32].as_slice()).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE hub_stock_state SET revision=revision+4092 WHERE singleton=1")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut raw)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT (SELECT count(*) FROM hub_corrections)+(SELECT count(*) FROM hub_count_return_associations)")
        .fetch_one(&mut raw).await.unwrap(), 4096);
    raw.close().await.unwrap();
}
