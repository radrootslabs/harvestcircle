use super::super::historical_prefix::{damage, snapshot};
use super::*;

async fn document_world() -> (
    Fixture,
    Database,
    HubRevision,
    serde_json::Value,
    HubRevision,
    serde_json::Value,
    HubRevision,
) {
    let (fixture, db, scope, input, paper) = issued_world().await;
    let old_cut = input.expected_stock_revision;
    let old = snapshot(&db, Some(old_cut)).await.unwrap();
    assert_eq!(old["issued"].as_array().unwrap().len(), 1);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut lines = Vec::new();
    for line in &paper.record.versions[0].material.lines {
        let view = db.hub_group(owner(), line.group).await.unwrap();
        lines.push(DispatchLineInput {
            group: line.group,
            expected_group_revision: view.group.revision,
            packs: line.packs,
        });
    }
    let crate::HubMutationResult::Dispatch(replaced) = db
        .submit_hub_mutation(
            scope,
            operation(1906),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: DispatchAction::Replace {
                    preparation: paper.record.id,
                    expected_revision: paper.record.revision,
                    recipient: id(9),
                    lines: DispatchSelection::new(lines).unwrap(),
                },
                observation: SourceProvenance::new(
                    "Later issued version retains the first immutable material",
                )
                .unwrap(),
            })),
            at(190),
        )
        .await
        .unwrap()
    else {
        panic!("replacement")
    };
    assert_eq!(replaced.record.versions.len(), 2);
    let next = command(&db, input.trace, source(), None).await;
    let crate::HubMutationResult::SourceAttributed(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(next)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let source_cut = posted.original.stamp.stock_revision;
    let with_source = snapshot(&db, Some(source_cut)).await.unwrap();
    assert_eq!(with_source["issued"].as_array().unwrap().len(), 2);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let crate::HubMutationResult::Dispatch(closed) = db
        .submit_hub_mutation(
            scope,
            operation(1907),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: DispatchAction::MarkUnused {
                    preparation: paper.record.id,
                    expected_revision: replaced.record.revision,
                },
                observation: SourceProvenance::new(
                    "Later explicit unused closure is not an old dependency",
                )
                .unwrap(),
            })),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("closure")
    };
    (
        fixture,
        db,
        old_cut,
        old,
        source_cut,
        with_source,
        closed.record.updated.stock_revision,
    )
}

#[tokio::test]
async fn historical_source_document_prefix_keeps_old_issue_before_replacement_source_change_and_closure_after_reopen()
 {
    let (fixture, db, old_cut, old, source_cut, with_source, last) = document_world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(snapshot(&db, Some(old_cut)).await.unwrap(), old);
    assert_eq!(snapshot(&db, Some(source_cut)).await.unwrap(), with_source);
    assert_eq!(snapshot(&db, Some(last)).await.unwrap(), with_source);
    assert_eq!(snapshot(&db, None).await.unwrap(), with_source);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(snapshot(&db, Some(old_cut)).await.unwrap(), old);
    assert_eq!(snapshot(&db, None).await.unwrap(), with_source);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_source_document_prefix_excludes_later_replacement_or_closure_corruption_and_current_rejects()
 {
    for future in [1906, 1907] {
        let (fixture, db, old_cut, old, source_cut, with_source, last) = document_world().await;
        damage(
            &fixture,
            "hub_dispatch_changes",
            "UPDATE hub_dispatch_changes SET result_hash=zeroblob(32) WHERE operation_id=?",
            operation(future).as_str(),
            true,
        )
        .await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(snapshot(&db, Some(old_cut)).await.unwrap(), old);
        if future == 1907 {
            assert_eq!(snapshot(&db, Some(source_cut)).await.unwrap(), with_source);
        }
        assert_eq!(
            snapshot(&db, Some(last)).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            snapshot(&db, None).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
