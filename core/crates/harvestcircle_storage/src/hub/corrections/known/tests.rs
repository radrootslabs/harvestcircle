use super::*;
use crate::hub::stock::tests::{Fixture, id, other, owner};
use harvestcircle_domain::{SafeErrorCode, hub::corrections::PhysicalCorrectionError};
mod historical_cut;

fn query(trace: HubRecordId) -> HubCorrectionKnownQuery {
    HubCorrectionKnownQuery {
        trace,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    }
}

#[tokio::test]
async fn complete_normal_original_retains_all_populations_and_issued_versions_without_writes() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let departure = crate::hub::handling::departure::tests::multi_population_departure(&db).await;
    let traces: BTreeSet<_> = departure
        .record
        .consumed
        .iter()
        .map(|line| line.before.trace)
        .collect();
    assert_eq!(traces.len(), 2);
    let before = crate::hub::custody::tests::material(&fixture).await;
    for trace in traces.iter().copied() {
        let evidence = db
            .hub_correction_known_departures(owner(), query(trace))
            .await
            .unwrap();
        assert_eq!(evidence.departures.len(), 1);
        let known = &evidence.departures[0];
        assert_eq!(
            known
                .recipient_original
                .populations
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            traces
        );
        assert_eq!(
            known.recipient_original.issued_versions,
            departure
                .record
                .paper
                .versions
                .iter()
                .map(|v| v.id)
                .collect::<Vec<_>>()
        );
        let lines = departure
            .record
            .consumed
            .iter()
            .filter(|line| line.before.trace == trace)
            .count();
        assert_eq!(known.original.lines.len(), lines);
        assert_eq!(known.quantities.len(), lines);
        assert!(known.quantities.iter().all(|q| q.amendments().is_empty()));
        assert_eq!(known.funding.len(), lines);
        assert_eq!(known.as_domain().original, &known.original);
        assert!(!evidence.uncertain_intent && !evidence.unresolved_actual);
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    let evidence = db
        .hub_correction_known_departures(owner(), query(*traces.first().unwrap()))
        .await
        .unwrap();
    assert_eq!(
        evidence.departures[0].recipient_original.populations.len(),
        2
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn found_and_returns_preserve_gross_event_and_exact_physical_reservations_after_reopen() {
    let (fixture, db, receipt) =
        super::super::projection_native_tests::found_review_tests::posting_tests::posted_fixture()
            .await;
    let trace = receipt.original.allocations[0].group.trace;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_correction_known_departures(owner(), query(trace))
        .await
        .unwrap();
    assert_eq!(evidence.departures.len(), 1);
    let known = &evidence.departures[0];
    assert_eq!(known.original.lines[0].packs.value(), 6);
    assert_eq!(known.quantities[0].original().packs.value(), 6);
    assert_eq!(known.quantities[0].amendments().len(), 1);
    assert_eq!(known.quantities[0].established_returns().value(), 4);
    assert_eq!(known.funding[0].recovered().value(), 2);
    assert_eq!(known.funding[0].returned().value(), 4);
    assert_eq!(known.funding[0].available().unwrap().value(), 0);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_correction_known_departures(owner(), query(trace))
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn owner_cut_and_unknown_population_denials_do_not_mutate_evidence() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, _) = crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    let trace = intake.original.position.trace;
    let evidence = db
        .hub_correction_known_departures(owner(), query(trace))
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_correction_known_departures(other(), query(trace))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut input = query(trace);
    input.expected_stock_revision = Some(evidence.revision);
    assert_eq!(
        db.hub_correction_known_departures(owner(), input.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    input.expected_catalog_revision = Some(evidence.catalog_revision);
    assert_eq!(
        db.hub_correction_known_departures(owner(), input.clone())
            .await
            .unwrap(),
        evidence
    );
    input.expected_stock_revision = Some(HubRevision::INITIAL);
    assert_eq!(
        db.hub_correction_known_departures(owner(), input)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert!(
        db.hub_correction_known_departures(owner(), query(id(245)))
            .await
            .is_err()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn unresolved_return_is_retained_as_scope_uncertainty_not_an_invented_reservation() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, _) = crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
    super::super::projection_native_tests::return_funding_tests::record_return(&db, None, 120)
        .await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_correction_known_departures(owner(), query(intake.original.position.trace))
        .await
        .unwrap();
    let known = &evidence.departures[0];
    assert_eq!(known.quantities[0].established_returns().value(), 0);
    assert_eq!(
        known.funding[0].available().unwrap_err(),
        PhysicalCorrectionError::UnresolvedReturnScope
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn damaged_original_seal_or_trace_filter_cannot_hide_a_known_event() {
    use sqlx::{AssertSqlSafe, Connection};
    for (table, update) in [
        (
            "hub_departures",
            "UPDATE hub_departures SET result_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_stock_events",
            "UPDATE hub_stock_events SET trace_id=? WHERE operation_id=?",
        ),
    ] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let departure =
            crate::hub::handling::departure::tests::multi_population_departure(&db).await;
        let trace = departure.record.consumed[0].before.trace;
        let other_trace = departure
            .record
            .consumed
            .iter()
            .find(|line| line.before.trace != trace)
            .unwrap()
            .before
            .trace;
        let mut raw = fixture.raw().await;
        let triggers: Vec<(String, String)> = sqlx::query_as(
            "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY name",
        )
        .bind(table)
        .fetch_all(&mut raw)
        .await
        .unwrap();
        assert!(!triggers.is_empty());
        for (name, _) in &triggers {
            sqlx::query(AssertSqlSafe(format!(
                "DROP TRIGGER \"{}\"",
                name.replace('"', "\"\"")
            )))
            .execute(&mut raw)
            .await
            .unwrap();
        }
        // Use an existing foreign population, preserving foreign keys; restore
        // exact immutable triggers before exercising the native reader.
        let update = if table == "hub_stock_events" {
            sqlx::query(update).bind(other_trace.as_bytes().as_slice())
        } else {
            sqlx::query(update)
        };
        update
            .bind(departure.operation.as_str())
            .execute(&mut raw)
            .await
            .unwrap();
        for (_, sql) in triggers {
            sqlx::query(AssertSqlSafe(sql))
                .execute(&mut raw)
                .await
                .unwrap();
        }
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_correction_known_departures(owner(), query(trace))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
