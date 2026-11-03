use super::*;
use crate::hub::handling::departure::tests::{
    custody_depart_current_group, custody_departure_fixture,
};
use crate::hub::stock::tests::{Fixture, id, now, operation, owner, query};
use harvestcircle_domain::hub::movement::PhysicalIdentification;
use harvestcircle_domain::hub::reference::{
    HubLabel, SourceAttribution, SourceProvenance, WholePacks,
};
use harvestcircle_domain::hub::returns::{ReturnDepartureClaim, ReturnLinkResolution};
use harvestcircle_domain::hub::stock::{Occurrence, PopulationOrigin};
use harvestcircle_domain::hub::treatment::AcceptanceStatus;
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
async fn command(db: &Database, departure: &DepartureRecord, n: u32, label: &str) -> ReturnCommand {
    let stock = db.hub_stock_page(owner(), query()).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    ReturnCommand {
        expected_stock_revision: stock.revision,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(6),
        returned_by: Some(id(9)),
        source: SourceAttribution::Unknown,
        packs: packs(n),
        identification: PhysicalIdentification {
            label: HubLabel::new(label).unwrap(),
            observation: SourceProvenance::new(
                "Independently observed physically identified returned crates",
            )
            .unwrap(),
        },
        planned_location: None,
        departure: Some(ReturnDepartureClaim {
            departure: departure.id,
            group: Some(departure.consumed[0].before.id),
        }),
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new(
            "Actual returned custody, independent of original recipient and work",
        )
        .unwrap(),
    }
}
#[tokio::test]
async fn native_return_two_preserves_departure_six_and_creates_separate_review_required_population()
{
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) = custody_departure_fixture(&db).await;
    let input = command(&db, &departure.record, 2, "Returned two native").await;
    let result = db
        .apply_hub_return(owner(), operation(100), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(result.original.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(
        result.original.intake.returned_by.as_ref().unwrap().id,
        id(9)
    );
    assert_eq!(
        result
            .original
            .linkage
            .departure
            .as_ref()
            .unwrap()
            .recipient
            .id,
        id(8)
    );
    assert_eq!(
        result.original.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    assert_eq!(
        result.original.population().origin,
        PopulationOrigin::Returned {
            returned: result.original.identities.returned
        }
    );
    assert!(result.original.intake_work.is_none());
    let old = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let new = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace: result.original.identities.population,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(old.projection.summary.departure_eligible, packs(14));
    assert_eq!(new.projection.summary.review_required, packs(2));
    assert_eq!(new.projection.summary.departure_eligible, packs(0));
    assert_eq!(new.projection.summary.restricted, packs(0));
    assert!(new.holds.is_empty());

    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        16
    );
    assert_eq!(
        page.rows
            .iter()
            .find(|r| r.trace.id == intake.original.trace.id)
            .unwrap()
            .position
            .packs,
        packs(14)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(intake)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departure)
    );
    assert_eq!(
        db.apply_hub_return(owner(), operation(100), input, now())
            .await
            .unwrap(),
        result
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_return_operation(owner(), operation(100))
            .await
            .unwrap(),
        Some(result)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
}

#[tokio::test]
async fn return_optional_work_is_fresh_pending_and_cumulative_overclaim_stays_unresolved() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) = custody_departure_fixture(&db).await;
    let mut input = command(&db, &departure.record, 2, "Returned fresh work two").await;
    input.planned_location = Some(id(7));
    let first = db
        .apply_hub_return(owner(), operation(100), input, now())
        .await
        .unwrap();
    let work = first.original.intake_work.as_ref().unwrap();
    assert_ne!(work.id, intake.original.trace.id);
    assert_eq!(work.quantity.required(), packs(2));
    assert_eq!(work.quantity.completed(), packs(0));
    assert_eq!(work.quantity.remaining(), packs(2));
    let current = db
        .hub_group(owner(), first.original.identities.group)
        .await
        .unwrap();
    assert_eq!(current.group.work.len(), 1);
    assert_eq!(current.group.work[0].requirement, *work);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let row = page
        .rows
        .iter()
        .find(|r| r.trace.id == first.original.identities.population)
        .unwrap();
    assert_eq!(row.put_away.as_ref().unwrap().quantity, work.quantity);
    let input = command(&db, &departure.record, 5, "Returned five unresolved").await;
    let second = db
        .apply_hub_return(owner(), operation(101), input, now())
        .await
        .unwrap();
    assert_eq!(
        second.original.linkage.resolution,
        ReturnLinkResolution::Unresolved(
            harvestcircle_domain::hub::returns::UnresolvedReturnLink::QuantityConflict
        )
    );
    assert_eq!(
        second
            .original
            .linkage
            .claimed_source
            .as_ref()
            .unwrap()
            .previously_established_returns,
        packs(2)
    );
    assert_eq!(second.original.intake.packs, packs(5));
    assert_eq!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        21
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(100))
            .await
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departure)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn repeated_native_return_departure_cycles_link_the_new_exit_and_keep_reads_bounded() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, original_departure) = custody_departure_fixture(&db).await;
    let mut departure = original_departure.clone();
    let mut originals = Vec::new();
    for n in 0..12u32 {
        let input = command(
            &db,
            &departure.record,
            2,
            &format!("Cycle {} returned two", n + 1),
        )
        .await;
        let result = db
            .apply_hub_return(owner(), operation(100 + n), input, now())
            .await
            .unwrap();
        assert_eq!(
            result
                .original
                .linkage
                .departure
                .as_ref()
                .unwrap()
                .departure,
            departure.record.id
        );
        if n > 0 {
            assert_ne!(departure.record.id, original_departure.record.id);
        }
        assert_eq!(
            result
                .original
                .linkage
                .claimed_source
                .as_ref()
                .unwrap()
                .previously_established_returns,
            packs(0)
        );
        let group = result.original.identities.group;
        originals.push(result);
        departure = custody_depart_current_group(&db, group, 1000 + n * 10).await;
        assert_eq!(departure.record.consumed[0].before.packs, packs(2));
        assert_eq!(
            db.hub_stock_page(owner(), query())
                .await
                .unwrap()
                .rows
                .iter()
                .map(|r| r.position.packs.value())
                .sum::<u32>(),
            14
        );
    }
    for original in originals {
        assert_eq!(
            db.hub_return_operation(owner(), original.operation.clone())
                .await
                .unwrap(),
            Some(original)
        );
    }
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(original_departure)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn trace_and_impact_keep_returned_source_independent_and_include_all_return_candidates() {
    use harvestcircle_domain::hub::reference::{
        SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
    };
    use harvestcircle_domain::hub::trace::{TraceScope, TraceSelection};
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let raw = HubRecordId::from_hex(&operation(1).as_str().replace('-', "")).unwrap();
    let x = harvestcircle_domain::hub::stock::ReceiptIdentities::from_operation_bytes(raw)
        .unwrap()
        .source;
    let source = |source_id, code| SourceAttribution::Referenced {
        source: SourceReference {
            id: source_id,
            issuer: Some(id(5)),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new(code).unwrap(),
            provenance: SourceProvenance::new("Independent manually recorded fictional source")
                .unwrap(),
        },
        certainty: SourceCertainty::Established,
    };
    let (intake, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture_with_source(
            &db,
            Some(source(x, "SOURCE-X")),
        )
        .await;
    let mut input = command(
        &db,
        &departure.record,
        2,
        "Return with independent source Y",
    )
    .await;
    let y = identities(&operation(100)).unwrap().source;
    input.source = source(y, "SOURCE-Y");
    let returned = db
        .apply_hub_return(owner(), operation(100), input, now())
        .await
        .unwrap();
    let q = |scope, offset, limit| crate::HubTraceQuery {
        expected_revision: None,
        expected_catalog_revision: None,
        selection: TraceSelection {
            scope,
            text: String::new(),
        },
        offset,
        limit,
    };
    let page = db
        .hub_trace_page(owner(), q(TraceScope::SourceImpact(x), 0, 64))
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert!(
        page.rows
            .iter()
            .any(|r| r.trace.id == intake.original.trace.id)
    );
    let row = page
        .rows
        .iter()
        .find(|r| r.trace.id == returned.original.identities.population)
        .unwrap();
    assert!(matches!(&row.trace.source,SourceAttribution::Referenced{source,..}if source.id==y));
    assert!(!row.trace.source_needs_review());
    assert!(!TraceScope::Unresolved.includes(&row.trace));
    let none = db
        .hub_trace_page(owner(), q(TraceScope::Unresolved, 0, 64))
        .await
        .unwrap();
    assert_eq!(none.total, 0);
    let exact = db
        .hub_trace_page(
            owner(),
            q(
                TraceScope::Receipt(returned.original.identities.population),
                0,
                64,
            ),
        )
        .await
        .unwrap();
    assert_eq!(exact.total, 1);
    let first = db
        .hub_trace_page(owner(), q(TraceScope::SourceImpact(x), 0, 1))
        .await
        .unwrap();
    assert_eq!(first.total, 2);
    assert!(first.more);
    let second = db
        .hub_trace_page(
            owner(),
            crate::HubTraceQuery {
                expected_revision: Some(first.revision),
                expected_catalog_revision: Some(first.catalog_revision),
                ..q(TraceScope::SourceImpact(x), 1, 1)
            },
        )
        .await
        .unwrap();
    assert!(!second.more);
    assert_ne!(first.rows[0].trace.id, second.rows[0].trace.id);
    let history = db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert!(
        history.rows.iter().any(
            |r| r.operation == operation(100) && r.kind == crate::HubTraceHistoryKind::Returned
        )
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn corrupted_return_seals_and_source_context_fail_closed_in_originals_and_next_admission() {
    use crate::hub::custody::tests::{corrupt, material};
    use harvestcircle_domain::SafeErrorCode;
    for (field, trigger, sql, projection_corrupt) in [
        (
            "population seal",
            "hub_returns_no_update",
            "UPDATE hub_returns SET population_hash=zeroblob(32)",
            true,
        ),
        (
            "full seal",
            "hub_returns_no_update",
            "UPDATE hub_returns SET result_hash=zeroblob(32)",
            false,
        ),
        (
            "captured prior total",
            "hub_return_sources_no_update",
            "UPDATE hub_return_sources SET previously_returned_packs=1",
            false,
        ),
    ] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let (_, departure) = custody_departure_fixture(&db).await;
        let input = command(&db, &departure.record, 2, field).await;
        let first = db
            .apply_hub_return(owner(), operation(100), input, now())
            .await
            .unwrap();
        let next = command(
            &db,
            &departure.record,
            1,
            "Next return must not ignore damaged prior",
        )
        .await;
        corrupt(&fixture, trigger, sql).await;
        if projection_corrupt {
            assert_eq!(
                db.hub_stock_page(owner(), query())
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::StorageCorrupt
            );
        }
        assert_eq!(
            db.hub_return_operation(owner(), operation(100))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            db.hub_trace_snapshot(
                owner(),
                crate::HubTraceSnapshotQuery {
                    trace: first.original.identities.population,
                    expected_revision: None,
                    expected_catalog_revision: None,
                }
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::StorageCorrupt
        );
        let before = material(&fixture).await;
        assert_eq!(
            db.apply_hub_return(owner(), operation(101), next, now())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn return_late_failure_rolls_back_every_native_row_and_same_intent_can_commit_once() {
    use crate::hub::custody::tests::material;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) = custody_departure_fixture(&db).await;
    let mut input = command(&db, &departure.record, 2, "Rollback returned crates").await;
    input.planned_location = Some(id(7));
    let before = material(&fixture).await;
    let attempted = input.clone();
    assert!(
        db.host()
            .transaction(|tx| Box::pin(async move {
                // Inject failure only after the complete production writer and its readback.
                write::apply_on(tx, owner(), operation(100), attempted, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            }))
            .await
            .is_err()
    );
    assert_eq!(material(&fixture).await, before);
    assert!(
        db.hub_return_operation(owner(), operation(100))
            .await
            .unwrap()
            .is_none()
    );
    let committed = db
        .apply_hub_return(owner(), operation(100), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        db.apply_hub_return(owner(), operation(100), input, now())
            .await
            .unwrap(),
        committed
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn return_foreign_and_stale_requests_leave_every_native_row_unchanged() {
    use crate::hub::custody::tests::material;
    use crate::hub::stock::tests::other;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) = custody_departure_fixture(&db).await;
    let input = command(&db, &departure.record, 2, "Current returned crates").await;
    let before = material(&fixture).await;
    assert!(
        db.apply_hub_return(other(), operation(100), input.clone(), now())
            .await
            .is_err()
    );
    assert_eq!(material(&fixture).await, before);
    db.apply_hub_return(owner(), operation(100), input.clone(), now())
        .await
        .unwrap();
    let after = material(&fixture).await;
    assert_eq!(
        db.apply_hub_return(owner(), operation(101), input, now())
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::HubConflict
    );
    assert_eq!(material(&fixture).await, after);
    db.close().await.unwrap();
}

#[tokio::test]
async fn unknown_ambiguous_and_unrecorded_returns_never_gain_an_old_source_when_a_later_exit_appears()
 {
    use harvestcircle_domain::hub::departure::DepartureIdentities;
    use harvestcircle_domain::hub::returns::UnresolvedReturnLink;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, original_departure) = custody_departure_fixture(&db).await;
    let mut unknown = command(
        &db,
        &original_departure.record,
        1,
        "No claimed old departure",
    )
    .await;
    unknown.departure = None;
    let unknown = db
        .apply_hub_return(owner(), operation(100), unknown, now())
        .await
        .unwrap();
    assert_eq!(
        unknown.original.linkage.resolution,
        ReturnLinkResolution::Unknown
    );
    assert!(unknown.original.linkage.plausible_sources.is_empty());
    let mut ambiguous = command(
        &db,
        &original_departure.record,
        1,
        "Known departure without group attribution",
    )
    .await;
    ambiguous.departure.as_mut().unwrap().group = None;
    let ambiguous = db
        .apply_hub_return(owner(), operation(101), ambiguous, now())
        .await
        .unwrap();
    assert_eq!(
        ambiguous.original.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::OriginalGroupUnknown)
    );
    assert_eq!(ambiguous.original.linkage.plausible_sources.len(), 1);
    assert!(ambiguous.original.linkage.claimed_source.is_none());
    let later_operation = operation(2003);
    let later = DepartureIdentities::from_operation_bytes(
        HubRecordId::from_hex(&later_operation.as_str().replace('-', "")).unwrap(),
    )
    .unwrap();
    let mut unrecorded = command(
        &db,
        &original_departure.record,
        1,
        "Old claim absent at original recording",
    )
    .await;
    unrecorded.departure = Some(ReturnDepartureClaim {
        departure: later.departure,
        group: Some(intake.original.position.group),
    });
    let absent = db
        .apply_hub_return(owner(), operation(102), unrecorded, now())
        .await
        .unwrap();
    assert_eq!(
        absent.original.linkage.resolution,
        ReturnLinkResolution::Unrecorded
    );
    assert!(absent.original.linkage.departure.is_none());
    assert!(absent.original.linkage.plausible_sources.is_empty());
    let newly_recorded =
        custody_depart_current_group(&db, intake.original.position.group, 2000).await;
    assert_eq!(newly_recorded.record.id, later.departure);
    assert_eq!(
        db.hub_return_operation(owner(), operation(102))
            .await
            .unwrap(),
        Some(absent)
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(100))
            .await
            .unwrap(),
        Some(unknown)
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(101))
            .await
            .unwrap(),
        Some(ambiguous)
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(original_departure)
    );
    db.close().await.unwrap();
}
