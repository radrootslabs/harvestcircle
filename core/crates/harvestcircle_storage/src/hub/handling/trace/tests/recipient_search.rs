use super::*;

async fn normal_world() -> (Fixture, Database, Vec<HubRecordId>) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_reference(
        owner(),
        operation(700),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Create {
                id: id(8),
                value: ReferenceValue::Party {
                    label: HubLabel::new("Distinct unamended normal buyer").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let a = receive(&db, 1, source(1, 5, SourceCertainty::Established)).await;
    let b = receive(&db, 2, SourceAttribution::Unknown).await;
    let groups = [a.original.position.group, b.original.position.group];
    accept(&db, groups[0], 10).await;
    accept(&db, groups[1], 11).await;
    let paper = issue_to(&db, &groups, 20, id(8)).await;
    depart(&db, &paper, 30).await;
    let mut traces = vec![a.original.trace.id, b.original.trace.id];
    traces.sort();
    (fixture, db, traces)
}

async fn matching(db: &Database, text: &str, offset: u32) -> HubTracePage {
    let mut query = HubTraceQuery {
        selection: TraceSelection {
            scope: TraceScope::All,
            text: text.into(),
        },
        limit: 1,
        ..HubTraceQuery::default()
    };
    let first = db.hub_trace_page(owner(), query.clone()).await.unwrap();
    if offset == 0 {
        return first;
    }
    query.offset = offset;
    query.expected_revision = Some(first.revision);
    query.expected_catalog_revision = Some(first.catalog_revision);
    db.hub_trace_page(owner(), query).await.unwrap()
}

#[tokio::test]
async fn unamended_whole_event_recipient_finds_both_populations_with_paging_and_reopen() {
    let (fixture, db, traces) = normal_world().await;
    for text in ["Distinct unamended normal buyer", &id(8).to_hex()] {
        for offset in 0..2 {
            let page = matching(&db, text, offset).await;
            assert_eq!(page.total, 2);
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0].trace.id, traces[offset as usize]);
            assert_eq!(page.more, offset == 0);
            assert!(page.rows[0].recipient_corrections.is_empty());
            assert_eq!(page.rows[0].balance.on_hand.value(), 0);
        }
    }
    let current = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(701),
        ReferenceCommand {
            expected_catalog_revision: current.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(8),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Current renamed buyer only").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        matching(&db, "Distinct unamended normal buyer", 0)
            .await
            .total,
        2
    );
    assert_eq!(
        matching(&db, "Current renamed buyer only", 0).await.total,
        0
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        matching(&db, "Distinct unamended normal buyer", 1)
            .await
            .rows[0]
            .trace
            .id,
        traces[1]
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn unamended_actual_recipient_uses_reviewed_original_not_receipt_or_raw_report() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, _, _) =
        crate::hub::corrections::projection_native_tests::actual_debit_to(&db, 8).await;
    for text in ["Distinct unamended actual buyer", &id(8).to_hex()] {
        let page = matching(&db, text, 0).await;
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].trace.id, before.trace);
        assert!(page.rows[0].recipient_corrections.is_empty());
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        matching(&db, "Distinct unamended actual buyer", 0)
            .await
            .total,
        1
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn unrelated_corrupt_departure_rejects_before_text_filter_without_writing() {
    let (fixture, db, _) = normal_world().await;
    let mut raw = fixture.raw().await;
    sqlx::query("DROP TRIGGER hub_departure_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_departures SET result_hash=zeroblob(32)")
        .execute(&mut raw)
        .await
        .unwrap();
    let guard = super::super::super::departure::schema::MIGRATION_SQL
        .split(";\n")
        .find(|sql| sql.starts_with("CREATE TRIGGER hub_departure_no_update "))
        .unwrap();
    sqlx::query(guard).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let mut query = HubTraceQuery::default();
    query.selection.text = "No matching fields anywhere".into();
    assert_eq!(
        db.hub_trace_page(owner(), query).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
