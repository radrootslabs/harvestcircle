//! Fresh SQL guard fixtures only. Header-only post-epoch Return rows here are
//! NOT authenticated native Return originals or scope/result-seal qualification.
use super::*;
use crate::hub::stock::tests::{id, now, operation, owner, query};
use harvestcircle_domain::hub::{
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceAttribution, SourceProvenance, WholePacks},
    returns::ReturnCommand,
    stock::Occurrence,
};

#[tokio::test]
async fn explicit_intake_mode_guards_epoch_admission_compatibility_and_body_result() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    db.apply_hub_return(
        owner(),
        operation(100),
        ReturnCommand {
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            variant: id(2),
            location: id(6),
            returned_by: None,
            source: SourceAttribution::Unknown,
            packs: WholePacks::from_value(1).unwrap(),
            identification: PhysicalIdentification {
                label: HubLabel::new("Fictional mode guard intake").unwrap(),
                observation: SourceProvenance::new("Fictional independent physical identification")
                    .unwrap(),
            },
            planned_location: None,
            departure: None,
            occurred: Occurrence::Unknown,
            observation: SourceProvenance::new("Fictional separate returned custody").unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let mut tx = raw.begin().await.unwrap();
    // Current fixtures already contain22 and operation100's authentic mode.
    sqlx::query(concat!(
        "INSERT INTO hub_stock_operations(operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) ",
        "SELECT ?1,hub_id,actor_public_key,'return',intent_hash,catalog_revision,stock_revision+1,?2,recorded_at_unix_s FROM hub_stock_operations WHERE operation_id=?3"
    )).bind(operation(500).as_str()).bind(id(77).as_bytes().as_slice()).bind(operation(100).as_str())
        .execute(&mut *tx).await.unwrap();
    for fault in 0..5 {
        let (mode, count, legacy_delta, capture) = match fault {
            0 => ("owned", 0_i64, 0_i64, Some(vec![4_u8; 32])), // No current runtime/admission.
            1 => ("legacy_compat", 1, 0, None),
            2 => ("legacy_compat", 0, 1, None),
            3 => ("legacy_compat", 0, 0, Some(vec![4_u8; 32])),
            4 => ("owned", 0, 0, None),
            _ => unreachable!(),
        };
        assert!(sqlx::query(concat!(
            "INSERT INTO hub_count_return_intake_modes(operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,mode,association_count,scope_hash,inner_result_hash,result_hash,capture_hash) ",
            "SELECT ?1,hub_id,epoch,legacy_stock_revision+?2,legacy_catalog_revision,?3,?4,?5,?6,?7,?8 FROM hub_count_return_epochs"
        )).bind(operation(500).as_str()).bind(legacy_delta).bind(mode).bind(count)
            .bind([1_u8;32].as_slice()).bind([2_u8;32].as_slice()).bind([3_u8;32].as_slice()).bind(capture)
            .execute(&mut *tx).await.is_err(), "mode fault {fault}");
    }
    let copy_body = concat!(
        "INSERT INTO hub_returns(return_id,operation_id,population_id,policy_version,returned_by_id,returned_by_revision,claimed_departure_id,claimed_group_id,known_departure_id,established_group_id,resolution,unresolved_reason,timing,source_count,intake_work_id,population_hash,result_hash) ",
        "SELECT return_id,?1,population_id,policy_version,returned_by_id,returned_by_revision,claimed_departure_id,claimed_group_id,",
        "known_departure_id,established_group_id,resolution,unresolved_reason,timing,source_count,intake_work_id,population_hash,result_hash ",
        "FROM hub_returns WHERE operation_id=?2"
    );
    // BEFORE INSERT guard, rather than an unrelated duplicate/FK constraint,
    // must reject the missing native mode before any Return body is admitted.
    let missing = sqlx::query(copy_body)
        .bind(operation(500).as_str())
        .bind(operation(100).as_str())
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("new return body requires its immutable native mode"),
        "unexpected missing-mode rejection: {missing}"
    );
    sqlx::query(concat!(
        "INSERT INTO hub_count_return_intake_modes(operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,mode,association_count,scope_hash,inner_result_hash,result_hash,capture_hash) ",
        "SELECT ?1,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,'legacy_compat',0,?2,?3,?4,NULL FROM hub_count_return_epochs"
    )).bind(operation(500).as_str()).bind([1_u8;32].as_slice()).bind([2_u8;32].as_slice()).bind([3_u8;32].as_slice())
        .execute(&mut *tx).await.unwrap();
    let wrong_result = sqlx::query(copy_body)
        .bind(operation(500).as_str())
        .bind(operation(100).as_str())
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(
        wrong_result
            .to_string()
            .contains("new return body requires its immutable native mode"),
        "unexpected wrapped-result rejection: {wrong_result}"
    );
    assert!(
        sqlx::query("UPDATE hub_count_return_intake_modes SET mode='owned',capture_hash=?")
            .bind([4_u8; 32].as_slice())
            .execute(&mut *tx)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM hub_count_return_intake_modes")
            .execute(&mut *tx)
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO hub_runtime_session(singleton,runtime_id,generation,actor_public_key,updated_at_unix_s) VALUES(1,?,zeroblob(8),NULL,100)")
        .bind([88_u8;16].as_slice()).execute(&mut *tx).await.unwrap();
    let mode: (String, Option<Vec<u8>>, i64) = sqlx::query_as(
        "SELECT mode,capture_hash,association_count FROM hub_count_return_intake_modes WHERE operation_id=?",
    )
    .bind(operation(500).as_str())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(mode, ("legacy_compat".into(), None, 0)); // Historical SQL metadata stays immutable.
    tx.rollback().await.unwrap();
    raw.close().await.unwrap();
}
