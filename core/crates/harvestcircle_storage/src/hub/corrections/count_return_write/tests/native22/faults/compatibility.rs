use super::*;

#[tokio::test]
async fn genuine_pre_runtime_return_survives_attachment_but_resealed_scope_or_added_admission_rejects()
 {
    for fault in 0..3 {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let catalog = crate::hub::stock::tests::setup(&db).await;
        assert!(db.hub_runtime_session().await.unwrap().is_none());
        let input = ReturnCommand {
            expected_stock_revision: HubRevision::INITIAL,
            expected_catalog_revision: catalog.revision(),
            variant: id(2),
            location: id(6),
            returned_by: None,
            source: SourceAttribution::Unknown,
            packs: WholePacks::from_value(1).unwrap(),
            identification: PhysicalIdentification {
                label: HubLabel::new("Pre-runtime identified return").unwrap(),
                observation: evidence(),
            },
            planned_location: None,
            departure: None,
            occurred: Occurrence::Unknown,
            observation: evidence(),
        };
        let now = crate::hub::stock::tests::now();
        let returned = db
            .apply_hub_return(owner(), operation(900), input.clone(), now)
            .await
            .unwrap();
        let (hub, op, mut mode) = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = crate::hub::reference::authorize_on(tx, owner()).await?.id();
                    let op = crate::hub::stock::read::operation_on(tx, hub, &operation(900))
                        .await?
                        .unwrap();
                    let own = crate::hub::custody::returns::read::own_on(tx, hub, &op).await?;
                    Ok::<_, SafeError>((hub, op, own.mode.unwrap()))
                })
            })
            .await
            .unwrap();
        assert_eq!(mode.kind, return_mode::Kind::Compatibility);
        assert_eq!(mode.associations, 0);
        assert_eq!(mode.capture, None);
        let runtime = db.begin_hub_runtime(now).await.unwrap();
        let generation = db
            .advance_hub_session(
                runtime,
                harvestcircle_application::SessionGeneration::initial(),
                Some(owner()),
                now,
            )
            .await
            .unwrap();
        assert_eq!(
            db.hub_return_operation(owner(), operation(900))
                .await
                .unwrap(),
            Some(returned.clone())
        );
        let mut next = input.clone();
        next.expected_stock_revision = returned.original.stock_revision;
        next.identification.label = HubLabel::new("Later owned return").unwrap();
        db.submit_hub_mutation(
            HubSubmissionScope {
                runtime,
                generation,
                actor: owner(),
            },
            operation(901),
            HubMutation::Return(Box::new(next)),
            now,
        )
        .await
        .unwrap();
        assert_eq!(
            db.hub_return_operation(owner(), operation(900))
                .await
                .unwrap(),
            Some(returned.clone())
        );
        let mut raw = fixture.raw().await;
        if fault == 1 {
            mode.scope[0] ^= 1;
            mode.result = return_mode::wrapped_for_test(hub, &op, &mode).unwrap();
            let mut tx = raw.begin().await.unwrap();
            let mode_ddl = remove_guard(&mut tx, "hub_count_return_intake_modes_no_update").await;
            let body_ddl = remove_guard(&mut tx, "hub_returns_no_update").await;
            assert_eq!(sqlx::query("UPDATE hub_count_return_intake_modes SET scope_hash=?,result_hash=? WHERE operation_id=?")
                .bind(mode.scope.as_slice()).bind(mode.result.as_slice()).bind(operation(900).as_str()).execute(&mut *tx).await.unwrap().rows_affected(), 1);
            assert_eq!(
                sqlx::query("UPDATE hub_returns SET result_hash=? WHERE operation_id=?")
                    .bind(mode.result.as_slice())
                    .bind(operation(900).as_str())
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
            restore_guard(&mut tx, mode_ddl).await;
            restore_guard(&mut tx, body_ddl).await;
            tx.commit().await.unwrap();
        } else if fault == 2 {
            // The real old operation never had admission. Introduce exactly one
            // forged admission using a later real row, retaining both originals.
            let insert = "INSERT INTO hub_submissions SELECT ?1,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s FROM hub_submissions WHERE operation_id=?2";
            assert!(
                sqlx::query(insert)
                    .bind(operation(900).as_str())
                    .bind(operation(901).as_str())
                    .execute(&mut raw)
                    .await
                    .is_err()
            );
            let mut tx = raw.begin().await.unwrap();
            let ddl = remove_guard(&mut tx, "hub_submission_admission_guard").await;
            assert_eq!(
                sqlx::query(insert)
                    .bind(operation(900).as_str())
                    .bind(operation(901).as_str())
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
            restore_guard(&mut tx, ddl).await;
            tx.commit().await.unwrap();
            let command = input.clone();
            db.host()
                .transaction(|tx| {
                    Box::pin(async move {
                        assert_eq!(
                            return_mode::original_on(tx, hub, &op, &command, mode.result)
                                .await
                                .unwrap_err()
                                .code(),
                            SafeErrorCode::StorageCorrupt
                        );
                        Ok::<_, SafeError>(())
                    })
                })
                .await
                .unwrap();
        }
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut raw)
                .await
                .unwrap()
                .is_empty()
        );
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        let original = db.hub_return_operation(owner(), operation(900)).await;
        if fault == 0 {
            assert_eq!(original.unwrap(), Some(returned.clone()));
        } else {
            assert_eq!(original.unwrap_err().code(), SafeErrorCode::StorageCorrupt);
        }
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        let original = db.hub_return_operation(owner(), operation(900)).await;
        if fault == 0 {
            assert_eq!(original.unwrap(), Some(returned));
        } else {
            assert_eq!(original.unwrap_err().code(), SafeErrorCode::StorageCorrupt);
        }
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
