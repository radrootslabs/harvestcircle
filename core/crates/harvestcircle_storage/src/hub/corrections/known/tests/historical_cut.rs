use super::*;
use crate::hub::stock::tests::{command, now, operation, setup};
use harvestcircle_domain::hub::{actuals::ActualKnownInterpretations, reference::*};

async fn read(
    db: &Database,
    trace: HubRecordId,
    stock: HubRevision,
    catalog: HubRevision,
) -> Result<ActualKnownInterpretations, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                Box::pin(actual_interpretations_at_on(
                    tx,
                    hub.id(),
                    trace,
                    stock,
                    catalog,
                ))
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn historical_empty_cut_rejects_impossible_catalogue_future_trace_and_future_cuts() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let first = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let saved = db
        .hub_actual_known_interpretations(owner(), query(first.original.trace.id))
        .await
        .unwrap()
        .interpretations;
    db.apply_hub_reference(
        owner(),
        operation(210),
        ReferenceCommand {
            expected_catalog_revision: saved.catalog_revision,
            mutation: ReferenceMutation::Revise {
                id: id(4),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Later fictional recipient").unwrap(),
                },
            },
        },
        UnixTimestamp::from_seconds(400).unwrap(),
    )
    .await
    .unwrap();
    let current_catalog = db.hub_references(owner()).await.unwrap();
    let mut next_receipt = command(&current_catalog);
    next_receipt.expected_stock_revision = saved.stock_revision;
    let second = db
        .apply_hub_receipt(
            owner(),
            operation(2),
            next_receipt,
            UnixTimestamp::from_seconds(410).unwrap(),
        )
        .await
        .unwrap();
    let current = db
        .hub_actual_known_interpretations(owner(), query(second.original.trace.id))
        .await
        .unwrap()
        .interpretations;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), query(id(99)))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        read(
            &db,
            first.original.trace.id,
            saved.stock_revision,
            saved.catalog_revision
        )
        .await
        .unwrap(),
        saved
    );
    // First trace still has no departures: rows alone cannot detect this bad pair.
    for (trace, stock, catalog) in [
        (
            first.original.trace.id,
            current.stock_revision,
            saved.catalog_revision,
        ),
        (
            second.original.trace.id,
            saved.stock_revision,
            current.catalog_revision,
        ),
        (
            first.original.trace.id,
            HubRevision::from_value(current.stock_revision.value() + 1).unwrap(),
            current.catalog_revision,
        ),
        (
            first.original.trace.id,
            current.stock_revision,
            HubRevision::from_value(current.catalog_revision.value() + 1).unwrap(),
        ),
        (id(99), current.stock_revision, current.catalog_revision),
    ] {
        assert_eq!(
            read(&db, trace, stock, catalog).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_normal_whole_event_keeps_sealed_paper_before_later_return_and_new_paper() {
    use crate::hub::handling::departure::tests::{
        custody_depart_current_group, multi_population_departure,
    };
    use harvestcircle_domain::hub::returns::{ReturnDepartureClaim, ReturnLinkResolution};
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let departure = multi_population_departure(&db).await;
    assert_eq!(departure.record.consumed.len(), 3);
    let line = &departure.record.consumed[0];
    let trace = line.before.trace;
    let saved = db
        .hub_actual_known_interpretations(owner(), query(trace))
        .await
        .unwrap()
        .interpretations;
    assert_eq!(saved.records.len(), 1);
    assert_eq!(saved.records[0].recipient_original.populations.len(), 2);
    let paper = saved.records[0].recipient_original.issued_versions.clone();
    let returned =
        super::super::super::projection_native_tests::return_funding_tests::record_return(
            &db,
            Some(ReturnDepartureClaim {
                departure: departure.record.id,
                group: Some(line.before.id),
            }),
            100,
        )
        .await;
    assert_eq!(
        returned.original.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    let with_return = db
        .hub_actual_known_interpretations(owner(), query(trace))
        .await
        .unwrap()
        .interpretations;
    let quantities = &with_return.records[0].quantities;
    assert_eq!(
        quantities
            .iter()
            .map(|q| q.established_returns().value())
            .sum::<u32>(),
        2
    );
    assert!(
        saved.records[0]
            .quantities
            .iter()
            .all(|q| q.established_returns().value() == 0)
    );
    let later = custody_depart_current_group(&db, returned.original.identities.group, 1000).await;
    assert!(
        later
            .record
            .paper
            .versions
            .iter()
            .all(|v| !paper.contains(&v.id))
    );
    db.apply_hub_reference(
        owner(),
        operation(2000),
        ReferenceCommand {
            expected_catalog_revision: with_return.catalog_revision,
            mutation: ReferenceMutation::Revise {
                id: id(8),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Later fictional paper recipient").unwrap(),
                },
            },
        },
        UnixTimestamp::from_seconds(400).unwrap(),
    )
    .await
    .unwrap();
    let current = db
        .hub_actual_known_interpretations(owner(), query(trace))
        .await
        .unwrap()
        .interpretations;
    assert_eq!(current.records[0].recipient_original.issued_versions, paper);
    assert_eq!(current.records[0].original, saved.records[0].original);
    assert_eq!(
        current.records[0].quantities,
        with_return.records[0].quantities
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    for cut in [&saved, &with_return, &current] {
        assert_eq!(
            read(&db, trace, cut.stock_revision, cut.catalog_revision)
                .await
                .unwrap(),
            *cut
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    for cut in [&saved, &with_return, &current] {
        assert_eq!(
            read(&db, trace, cut.stock_revision, cut.catalog_revision)
                .await
                .unwrap(),
            *cut
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_cut_still_rejects_global_journal_accounting_corruption() {
    use sqlx::{AssertSqlSafe, Connection};
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let first = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let trace = first.original.trace.id;
    let saved = db
        .hub_actual_known_interpretations(owner(), query(trace))
        .await
        .unwrap()
        .interpretations;
    let mut raw = fixture.raw().await;
    let triggers: Vec<(String, String)> = sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name='hub_stock_state' ORDER BY name").fetch_all(&mut raw).await.unwrap();
    for (name, _) in &triggers {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER \"{}\"",
            name.replace('"', "\"\"")
        )))
        .execute(&mut raw)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE hub_stock_state SET event_count=event_count+1")
        .execute(&mut raw)
        .await
        .unwrap();
    for (_, trigger) in triggers {
        sqlx::query(AssertSqlSafe(trigger))
            .execute(&mut raw)
            .await
            .unwrap();
    }
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        read(&db, trace, saved.stock_revision, saved.catalog_revision)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), query(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
