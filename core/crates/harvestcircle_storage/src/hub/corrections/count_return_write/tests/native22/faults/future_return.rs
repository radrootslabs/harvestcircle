use super::*;

#[tokio::test]
async fn later_return_body_and_mode_faults_are_excluded_before_their_dense_cut() {
    for fault in 0..2 {
        let world = association_world().await;
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Returned(returned) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(1000),
                HubMutation::Return(Box::new(ReturnCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: page.catalog_revision,
                    variant: id(2),
                    location: id(6),
                    returned_by: None,
                    source: SourceAttribution::Unknown,
                    packs: WholePacks::from_value(1).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Later independently identified restricted return")
                            .unwrap(),
                        observation: evidence(),
                    },
                    planned_location: None,
                    departure: None,
                    occurred: Occurrence::Unknown,
                    observation: evidence(),
                })),
                at(270),
            )
            .await
            .unwrap()
        else {
            panic!("ordinary later Return")
        };
        let hub = world.db.load_hub().await.unwrap().unwrap().id();
        world
            .db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let op = crate::hub::stock::read::operation_on(tx, hub, &operation(1000))
                        .await?
                        .unwrap();
                    let own = crate::hub::custody::returns::read::own_on(tx, hub, &op).await?;
                    let mode = own.mode.unwrap();
                    assert_eq!(mode.kind, return_mode::Kind::Owned);
                    assert_eq!(mode.associations, 1);
                    Ok::<_, SafeError>(())
                })
            })
            .await
            .unwrap();
        let mut raw = world.fixture.raw().await;
        let (guard, change) = if fault == 0 {
            (
                "hub_returns_no_update",
                "UPDATE hub_returns SET result_hash=zeroblob(32) WHERE operation_id=?",
            )
        } else {
            (
                "hub_count_return_intake_modes_no_delete",
                "DELETE FROM hub_count_return_intake_modes WHERE operation_id=?",
            )
        };
        assert!(
            sqlx::query(change)
                .bind(operation(1000).as_str())
                .execute(&mut raw)
                .await
                .is_err()
        );
        let mut tx = raw.begin().await.unwrap();
        let ddl = remove_guard(&mut tx, guard).await;
        assert_eq!(
            sqlx::query(change)
                .bind(operation(1000).as_str())
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        restore_guard(&mut tx, ddl).await;
        tx.commit().await.unwrap();
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut raw)
                .await
                .unwrap()
                .is_empty()
        );
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        let cut = returned.original.stock_revision;
        assert_return_cuts(&world.db, &world, cut).await;
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_return_cuts(&db, &world, cut).await;
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        db.close().await.unwrap();
    }
}

async fn assert_return_cuts(db: &Database, world: &AssociationWorld, included: HubRevision) {
    assert_eq!(
        db.hub_return_operation(owner(), operation(900))
            .await
            .unwrap()
            .as_ref(),
        Some(world.returned.as_ref())
    );
    assert_eq!(
        db.hub_count_explanation_operation(owner(), operation(905))
            .await
            .unwrap()
            .as_ref(),
        Some(world.explanation.as_ref())
    );
    assert_eq!(
        db.hub_count_return_association_operation(owner(), operation(906))
            .await
            .unwrap()
            .as_ref(),
        Some(world.associated.as_ref())
    );
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let older = world.associated.original.association.stock_revision;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = crate::hub::actuals::proofs::OriginalReader::new(tx);
                let scope = crate::hub::corrections::count_return_scope::VerifiedScope::load_on(
                    &mut reader,
                    hub,
                    older,
                )
                .await?;
                assert_eq!(scope.material(own(1100))?.associations, 1);
                assert_eq!(
                    crate::hub::corrections::count_return_scope::VerifiedScope::load_on(
                        &mut reader,
                        hub,
                        included
                    )
                    .await
                    .err()
                    .unwrap()
                    .code(),
                    SafeErrorCode::StorageCorrupt
                );
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        db.hub_return_operation(owner(), operation(1000))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
}
