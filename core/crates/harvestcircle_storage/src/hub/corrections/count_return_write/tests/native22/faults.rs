use super::*;
use crate::hub::corrections::{count_return_ledger, return_mode};
use sqlx::{AssertSqlSafe, Connection};
mod compatibility;
mod future_return;

async fn remove_guard(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, name: &str) -> String {
    let ddl: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=? AND type='trigger'")
            .bind(name)
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {name}")))
        .execute(&mut **tx)
        .await
        .unwrap();
    ddl
}
async fn restore_guard(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, ddl: String) {
    sqlx::query(AssertSqlSafe(ddl))
        .execute(&mut **tx)
        .await
        .unwrap();
}

// Preserve the authentic command, admission and result while making the saved
// description internally consistent. Only full original reconstruction can
// reject this mismatch with the genuine one-pack plan.
pub(in crate::hub) async fn reseal_two_pack_projection(world: &AssociationWorld) {
    reseal_association_fault(world, 0).await;
}

async fn reseal_association_fault(world: &AssociationWorld, fault: u8) {
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let mut root = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = crate::hub::stock::read::operation_on(tx, hub, &operation(906))
                    .await?
                    .unwrap();
                count_return_ledger::read_on(tx, hub, op).await
            })
        })
        .await
        .unwrap();
    match fault {
        0 => {
            let mut projection = world.associated.original.association.clone();
            assert_eq!(projection.packs.value(), 1);
            projection.packs = WholePacks::from_value(2).unwrap();
            root.packs = 2;
            root.projection = count_return_ledger::projection_bytes(&projection).unwrap();
        }
        1 => root.result[0] ^= 1,
        2 => root.capture[0] ^= 1,
        _ => unreachable!(),
    }
    root.projection_hash = count_return_ledger::projection_seal(hub, &root).unwrap();
    let mut raw = world.fixture.raw().await;
    let mut tx = raw.begin().await.unwrap();
    let ddl = remove_guard(&mut tx, "hub_count_return_associations_no_update").await;
    assert_eq!(sqlx::query("UPDATE hub_count_return_associations SET packs=?,projection=?,projection_hash=?,result_hash=?,capture_hash=? WHERE operation_id=?")
        .bind(i64::from(root.packs)).bind(root.projection).bind(root.projection_hash.as_slice())
        .bind(root.result.as_slice()).bind(root.capture.as_slice())
        .bind(operation(906).as_str()).execute(&mut *tx).await.unwrap().rows_affected(), 1);
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
}

#[tokio::test]
async fn resealed_future_association_faults_leave_earlier_originals_and_cut_exact() {
    for fault in 0..3 {
        let world = association_world().await;
        reseal_association_fault(&world, fault).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_future_association_cut(&world.db, &world).await;
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_future_association_cut(&db, &world).await;
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        db.close().await.unwrap();
    }
}

async fn assert_future_association_cut(db: &Database, world: &AssociationWorld) {
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
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let older = world.command.expected_stock_revision;
    let included = world.associated.original.association.stock_revision;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                // These are real dense journal cuts: no malformed/future header is used
                // to make the damaged body disappear from the earlier proof.
                let op = crate::hub::stock::read::operation_on(tx, hub, &operation(906))
                    .await?
                    .unwrap();
                count_return_ledger::read_on(tx, hub, op).await?;
                let mut reader = crate::hub::actuals::proofs::OriginalReader::new(tx);
                let scope = super::super::super::super::count_return_scope::VerifiedScope::load_on(
                    &mut reader,
                    hub,
                    older,
                )
                .await?;
                assert_eq!(scope.material(own(1000))?.associations, 0);
                for cut in [
                    included,
                    HubRevision::from_value(included.value() + 1).unwrap(),
                ] {
                    assert_eq!(
                        super::super::super::super::count_return_scope::VerifiedScope::load_on(
                            &mut reader,
                            hub,
                            cut
                        )
                        .await
                        .err()
                        .unwrap()
                        .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        db.hub_count_return_association_operation(owner(), operation(906))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
}

// Delete one exact encoded material entry without re-encoding any other byte.
fn omit_return_scope(original: &[u8]) -> Vec<u8> {
    let tag = b"{\"return_scope\":";
    let positions = original
        .windows(tag.len())
        .enumerate()
        .filter_map(|(i, v)| (v == tag).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 1);
    let mut start = positions[0];
    let mut end = start;
    let (mut depth, mut quoted, mut escaped) = (0_i32, false, false);
    for (offset, &byte) in original[start..].iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                end = start + offset + 1;
                break;
            }
        }
    }
    assert!(end > start);
    if original.get(end) == Some(&b',') {
        end += 1;
    } else if start > 0 && original[start - 1] == b',' {
        start -= 1;
    }
    let mut changed = original.to_vec();
    changed.drain(start..end);
    let mut restored = changed.clone();
    restored.splice(start..start, original[start..end].iter().copied());
    assert_eq!(restored, original);
    changed
}

#[tokio::test]
async fn owned_return_mode_omission_resealed_scope_and_admission_present_downgrade_reject_without_writes()
 {
    for fault in 0..6 {
        let world = association_world().await;
        let (hub, op, mut mode, command) = world
            .db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = crate::hub::reference::authorize_on(tx, owner()).await?.id();
                    let op = crate::hub::stock::read::operation_on(tx, hub, &operation(900))
                        .await?
                        .unwrap();
                    let own = crate::hub::custody::returns::read::own_on(tx, hub, &op).await?;
                    Ok::<_, SafeError>((hub, op, own.mode.unwrap(), own.command))
                })
            })
            .await
            .unwrap();
        let mut raw = world.fixture.raw().await;
        if fault == 0 {
            assert!(
                sqlx::query("DELETE FROM hub_count_return_intake_modes WHERE operation_id=?")
                    .bind(operation(900).as_str())
                    .execute(&mut raw)
                    .await
                    .is_err()
            );
            let mut tx = raw.begin().await.unwrap();
            let ddl = remove_guard(&mut tx, "hub_count_return_intake_modes_no_delete").await;
            assert_eq!(
                sqlx::query("DELETE FROM hub_count_return_intake_modes WHERE operation_id=?")
                    .bind(operation(900).as_str())
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
            restore_guard(&mut tx, ddl).await;
            tx.commit().await.unwrap();
        } else {
            let mut changed_capture = None;
            match fault {
                1 => mode.associations = 1,
                2 => {
                    assert_ne!(mode.scope, [0x55; 32]);
                    mode.scope = [0x55; 32];
                }
                3 => {
                    let original: Vec<u8> = sqlx::query_scalar(
                        "SELECT capture_body FROM hub_submissions WHERE operation_id=?",
                    )
                    .bind(operation(900).as_str())
                    .fetch_one(&mut raw)
                    .await
                    .unwrap();
                    let changed = omit_return_scope(&original);
                    let before = harvestcircle_application::hub_submission::inspect_hub_submission(
                        &original,
                    )
                    .unwrap();
                    let after =
                        harvestcircle_application::hub_submission::inspect_hub_submission(&changed)
                            .unwrap();
                    assert_eq!(before.scope, after.scope);
                    assert_eq!(before.input_digest, after.input_digest);
                    assert_ne!(before.capture_digest, after.capture_digest);
                    mode.capture = Some(after.capture_digest);
                    changed_capture = Some(changed);
                }
                4 => {
                    mode.kind = return_mode::Kind::Compatibility;
                    mode.capture = None;
                }
                5 => mode.inner[0] ^= 1,
                _ => unreachable!(),
            }
            mode.result = return_mode::wrapped_for_test(hub, &op, &mode).unwrap();
            let update = "UPDATE hub_count_return_intake_modes SET mode=?,association_count=?,scope_hash=?,inner_result_hash=?,result_hash=?,capture_hash=? WHERE operation_id=?";
            let kind = if mode.kind == return_mode::Kind::Owned {
                "owned"
            } else {
                "legacy_compat"
            };
            assert!(
                sqlx::query(update)
                    .bind(kind)
                    .bind(i64::from(mode.associations))
                    .bind(mode.scope.as_slice())
                    .bind(mode.inner.as_slice())
                    .bind(mode.result.as_slice())
                    .bind(mode.capture.as_ref().map(|v| v.as_slice()))
                    .bind(operation(900).as_str())
                    .execute(&mut raw)
                    .await
                    .is_err()
            );
            let mut tx = raw.begin().await.unwrap();
            let mode_ddl = remove_guard(&mut tx, "hub_count_return_intake_modes_no_update").await;
            let body_ddl = remove_guard(&mut tx, "hub_returns_no_update").await;
            if let Some(body) = changed_capture {
                let ddl = remove_guard(&mut tx, "hub_submissions_no_update").await;
                assert_eq!(sqlx::query("UPDATE hub_submissions SET capture_body=?,capture_hash=? WHERE operation_id=?").bind(body).bind(mode.capture.unwrap().as_slice()).bind(operation(900).as_str()).execute(&mut *tx).await.unwrap().rows_affected(),1);
                restore_guard(&mut tx, ddl).await;
            }
            assert_eq!(
                sqlx::query(update)
                    .bind(kind)
                    .bind(i64::from(mode.associations))
                    .bind(mode.scope.as_slice())
                    .bind(mode.inner.as_slice())
                    .bind(mode.result.as_slice())
                    .bind(mode.capture.as_ref().map(|v| v.as_slice()))
                    .bind(operation(900).as_str())
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
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
        }
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut raw)
                .await
                .unwrap()
                .is_empty()
        );
        raw.close().await.unwrap();
        if fault == 4 {
            assert_eq!(
                return_mode::wrapped_for_test(hub, &op, &mode).unwrap(),
                mode.result
            );
            world
                .db
                .host()
                .transaction(|tx| {
                    Box::pin(async move {
                        let count: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM hub_submissions WHERE operation_id=?",
                        )
                        .bind(operation(900).as_str())
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|_| crate::db::storage_unavailable())?;
                        assert_eq!(count, 1);
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
        } else if fault == 5 {
            world
                .db
                .host()
                .transaction(|tx| {
                    Box::pin(async move {
                        assert_eq!(
                            return_mode::original_on(tx, hub, &op, &command, mode.result).await?,
                            Some(mode)
                        );
                        Ok::<_, SafeError>(())
                    })
                })
                .await
                .unwrap();
        }
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(900))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "mode fault{fault}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_eq!(
            db.hub_return_operation(owner(), operation(900))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "reopened mode fault{fault}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn valid_count_parent_above_return_epoch_rejects_return_and_association_originals() {
    let world = association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let mut raw = world.fixture.raw().await;
    let update = "UPDATE hub_count_interpretation_epochs SET legacy_catalog_revision=legacy_catalog_revision+1 WHERE hub_id=?";
    assert!(
        sqlx::query(update)
            .bind(hub.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let mut tx = raw.begin().await.unwrap();
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *tx)
        .await
        .unwrap();
    let epoch_ddl = remove_guard(&mut tx, "hub_count_interpretation_epochs_no_update").await;
    let marker_ddl = remove_guard(&mut tx, "hub_count_interpretation_markers_no_update").await;
    assert_eq!(
        sqlx::query(update)
            .bind(hub.as_bytes().as_slice())
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    assert_eq!(sqlx::query("UPDATE hub_count_interpretation_markers SET legacy_catalog_revision=legacy_catalog_revision+1 WHERE hub_id=?").bind(hub.as_bytes().as_slice()).execute(&mut *tx).await.unwrap().rows_affected(),3);
    restore_guard(&mut tx, epoch_ddl).await;
    restore_guard(&mut tx, marker_ddl).await;
    tx.commit().await.unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut raw)
            .await
            .unwrap()
            .is_empty()
    );
    let (parent_stock,parent_catalog,child_stock,child_catalog,dependency):(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT p.legacy_stock_revision,p.legacy_catalog_revision,c.legacy_stock_revision,c.legacy_catalog_revision,COALESCE((SELECT max(o.catalog_revision) FROM hub_stock_operations o WHERE o.hub_id=p.hub_id AND o.stock_revision<=p.legacy_stock_revision),1) FROM hub_count_interpretation_epochs p JOIN hub_count_return_epochs c ON c.hub_id=p.hub_id").fetch_one(&mut raw).await.unwrap();
    assert_eq!((parent_stock, child_stock), (1, 1));
    assert!(dependency <= child_catalog && dependency <= parent_catalog);
    assert!(child_catalog < parent_catalog);
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_parent_original_rejections(&world.db, hub).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
    let db = world.fixture.open().await;
    assert_parent_original_rejections(&db, hub).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    db.close().await.unwrap();
}

async fn assert_parent_original_rejections(db: &Database, hub: HubId) {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let parent = crate::hub::corrections::count_mode::epoch_on(tx, hub).await?;
                assert_eq!((parent.stock, parent.catalog), (1, 2));
                assert_eq!(
                    count_return_ledger::epoch_on(tx, hub)
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
    assert_eq!(
        db.hub_return_operation(owner(), operation(900))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_count_return_association_operation(owner(), operation(906))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
}

#[tokio::test]
async fn valid_count_stock_parent_above_return_epoch_rejects_with_equal_catalog_cuts() {
    let world = association_world().await;
    let hub = world.db.load_hub().await.unwrap().unwrap().id();
    let mut raw = world.fixture.raw().await;
    let catalog: i64 = sqlx::query_scalar("SELECT COALESCE(max(catalog_revision),1) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=2")
        .bind(hub.as_bytes().as_slice()).fetch_one(&mut raw).await.unwrap();
    let update = "UPDATE hub_count_interpretation_epochs SET legacy_stock_revision=2,legacy_catalog_revision=? WHERE hub_id=?";
    assert!(
        sqlx::query(update)
            .bind(catalog)
            .bind(hub.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let mut tx = raw.begin().await.unwrap();
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *tx)
        .await
        .unwrap();
    let guards = [
        "hub_count_interpretation_epochs_no_update",
        "hub_count_interpretation_markers_no_update",
        "hub_count_return_epochs_no_update",
        "hub_count_return_intake_modes_no_update",
        "hub_count_return_associations_no_update",
    ];
    let mut ddls = Vec::new();
    for name in guards {
        ddls.push(remove_guard(&mut tx, name).await);
    }
    assert_eq!(
        sqlx::query(update)
            .bind(catalog)
            .bind(hub.as_bytes().as_slice())
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    assert_eq!(sqlx::query("UPDATE hub_count_interpretation_markers SET legacy_stock_revision=2,legacy_catalog_revision=? WHERE hub_id=?").bind(catalog).bind(hub.as_bytes().as_slice()).execute(&mut *tx).await.unwrap().rows_affected(), 3);
    // Equal valid catalog cuts leave only the stock-parent ordering violated.
    // Child tuple dependants move together to preserve actual foreign keys;
    // full body/result authentication is not the oracle for this epoch test.
    for table in [
        "hub_count_return_epochs",
        "hub_count_return_intake_modes",
        "hub_count_return_associations",
    ] {
        assert_eq!(
            sqlx::query(AssertSqlSafe(format!(
                "UPDATE {table} SET legacy_catalog_revision=? WHERE hub_id=?"
            )))
            .bind(catalog)
            .bind(hub.as_bytes().as_slice())
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
            1
        );
    }
    for ddl in ddls {
        restore_guard(&mut tx, ddl).await;
    }
    tx.commit().await.unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut raw)
            .await
            .unwrap()
            .is_empty()
    );
    let (p,c,pc,cc): (i64,i64,i64,i64) = sqlx::query_as("SELECT p.legacy_stock_revision,c.legacy_stock_revision,p.legacy_catalog_revision,c.legacy_catalog_revision FROM hub_count_interpretation_epochs p JOIN hub_count_return_epochs c ON c.hub_id=p.hub_id").fetch_one(&mut raw).await.unwrap();
    assert_eq!((p, c, pc, cc), (2, 1, catalog, catalog));
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_stock_parent_rejections(&world.db, hub, catalog as u64).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
    let db = world.fixture.open().await;
    assert_stock_parent_rejections(&db, hub, catalog as u64).await;
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    db.close().await.unwrap();
}

async fn assert_stock_parent_rejections(db: &Database, hub: HubId, catalog: u64) {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let parent = crate::hub::corrections::count_mode::epoch_on(tx, hub).await?;
                assert_eq!((parent.stock, parent.catalog), (2, catalog));
                assert_eq!(
                    count_return_ledger::epoch_on(tx, hub)
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
    assert_eq!(
        db.hub_return_operation(owner(), operation(900))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_count_return_association_operation(owner(), operation(906))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
}
