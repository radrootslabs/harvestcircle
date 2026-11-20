//! Ordinary native source/recipient effects establish these small ownership
//! worlds. Faults touch only fresh disposable fixtures, never retained witnesses.
use super::*;
use crate::hub::stock::tests::{Fixture, command as receiving, id, operation, owner, query, setup};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        corrections::SourceAttributionCommand,
        reference::{
            SourceAttribution, SourceCertainty, SourceCode, SourceProvenance, SourceReference,
            SourceReferenceKind,
        },
    },
};
use sqlx::Connection;
mod admission_identity;
mod count_traces;
mod maximum;

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Fictional independent native budget scope evidence").unwrap()
}

async fn scope(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(at(150)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(150),
        )
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}

async fn count(db: &Database, trace: HubRecordId, cut: HubRevision) -> Result<usize, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                through_on(tx, hub, trace, cut).await
            })
        })
        .await
        .map_err(map_transaction_error)
}

async fn source_world() -> (
    Fixture,
    Database,
    HubRecordId,
    HubRecordId,
    HubRevision,
    HubRevision,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let first = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), at(100))
        .await
        .unwrap();
    let mut next = receiving(&catalog);
    next.expected_stock_revision = first.original.stock_revision;
    let second = db
        .apply_hub_receipt(owner(), operation(2), next, at(110))
        .await
        .unwrap();
    let scope = scope(&db).await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let before = page.revision;
    let command = SourceAttributionCommand {
        trace: first.original.trace.id,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(90),
                issuer: Some(id(3)),
                kind: SourceReferenceKind::SupplierReference,
                code: SourceCode::new("Fictional source for native budget").unwrap(),
                provenance: note(),
            },
            certainty: SourceCertainty::Established,
        },
        basis: note(),
    };
    db.submit_hub_mutation(
        scope,
        operation(1000),
        HubMutation::SourceAttribution(Box::new(command)),
        at(200),
    )
    .await
    .unwrap();
    let after = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    (
        fixture,
        db,
        first.original.trace.id,
        second.original.trace.id,
        before,
        after,
    )
}

#[tokio::test]
async fn native_source_root_counts_once_only_on_its_own_trace_and_at_its_original_cut() {
    let (_fixture, db, a, b, before, after) = source_world().await;
    assert_eq!(count(&db, a, before).await.unwrap(), 0);
    assert_eq!(count(&db, a, after).await.unwrap(), 1);
    assert_eq!(count(&db, b, after).await.unwrap(), 0);
    db.close().await.unwrap();
}

#[tokio::test]
async fn one_native_multiline_recipient_root_counts_once_for_each_actual_target_population() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    crate::hub::handling::departure::tests::multi_population_departure(&db).await;
    let departure = db
        .hub_departure_operation(owner(), operation(30))
        .await
        .unwrap()
        .unwrap();
    let traces = departure
        .record
        .consumed
        .iter()
        .map(|line| line.before.trace)
        .collect::<BTreeSet<_>>();
    assert!(traces.len() > 1);
    let scope = scope(&db).await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let command = harvestcircle_domain::hub::corrections::RecipientAttributionCommand {
        departure_operation: departure.record.operation,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: None,
        recipient: None,
        basis: note(),
    };
    db.submit_hub_mutation(
        scope,
        operation(1001),
        HubMutation::RecipientAttribution(Box::new(command)),
        at(200),
    )
    .await
    .unwrap();
    let after = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    for trace in traces {
        assert_eq!(count(&db, trace, page.revision).await.unwrap(), 0);
        assert_eq!(count(&db, trace, after).await.unwrap(), 1);
    }
    db.close().await.unwrap();
}

#[tokio::test]
async fn mixed_native_source_and_recipient_roots_share_only_their_actual_trace_budgets() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    crate::hub::handling::departure::tests::multi_population_departure(&db).await;
    let departure = db
        .hub_departure_operation(owner(), operation(30))
        .await
        .unwrap()
        .unwrap();
    let traces = departure
        .record
        .consumed
        .iter()
        .map(|line| line.before.trace)
        .collect::<BTreeSet<_>>();
    assert!(traces.len() > 1);
    let selected = *traces.first().unwrap();
    let scope = scope(&db).await;
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(1000),
        HubMutation::SourceAttribution(Box::new(SourceAttributionCommand {
            trace: selected,
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            expected_prior: None,
            basis: note(),
            source: SourceAttribution::Referenced {
                source: SourceReference {
                    id: id(90),
                    issuer: Some(id(3)),
                    kind: SourceReferenceKind::SupplierReference,
                    code: SourceCode::new("Fictional mixed-budget source").unwrap(),
                    provenance: note(),
                },
                certainty: SourceCertainty::Established,
            },
        })),
        at(200),
    )
    .await
    .unwrap();
    let source_cut = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    for trace in &traces {
        assert_eq!(
            count(&db, *trace, source_cut).await.unwrap(),
            usize::from(*trace == selected)
        );
    }
    db.submit_hub_mutation(
        scope,
        operation(1001),
        HubMutation::RecipientAttribution(Box::new(
            harvestcircle_domain::hub::corrections::RecipientAttributionCommand {
                departure_operation: departure.record.operation,
                expected_stock_revision: source_cut,
                expected_catalog_revision: page.catalog_revision,
                expected_prior: None,
                recipient: None,
                basis: note(),
            },
        )),
        at(210),
    )
    .await
    .unwrap();
    let after = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    for trace in traces {
        assert_eq!(
            count(&db, trace, after).await.unwrap(),
            1 + usize::from(trace == selected)
        );
        assert_eq!(count(&db, trace, page.revision).await.unwrap(), 0);
    }
    db.close().await.unwrap();
}

fn trigger(name: &str) -> &'static str {
    include_str!("../schema-v19.sql")
        .split(";\n")
        .find(|sql| sql.trim().starts_with(&format!("CREATE TRIGGER {name} ")))
        .unwrap()
}

#[tokio::test]
async fn moved_final_scope_missing_body_and_deleted_scope_links_cannot_manufacture_room() {
    for fault in 0..3 {
        let (fixture, db, a, b, before, after) = source_world().await;
        db.close().await.unwrap();
        let mut raw = fixture.raw().await;
        if fault == 0 {
            sqlx::query("DROP TRIGGER hub_corrections_no_update")
                .execute(&mut raw)
                .await
                .unwrap();
            sqlx::query("UPDATE hub_corrections SET subject_id=? WHERE operation_id=?")
                .bind(b.as_bytes().as_slice())
                .bind(operation(1000).as_str())
                .execute(&mut raw)
                .await
                .unwrap();
            sqlx::raw_sql(trigger("hub_corrections_no_update"))
                .execute(&mut raw)
                .await
                .unwrap();
        } else {
            sqlx::query("DROP TRIGGER hub_correction_scope_no_delete")
                .execute(&mut raw)
                .await
                .unwrap();
            sqlx::query("DELETE FROM hub_correction_scope_links WHERE correction_operation_id=?")
                .bind(operation(1000).as_str())
                .execute(&mut raw)
                .await
                .unwrap();
            sqlx::raw_sql(trigger("hub_correction_scope_no_delete"))
                .execute(&mut raw)
                .await
                .unwrap();
            if fault == 1 {
                sqlx::query("DROP TRIGGER hub_corrections_no_delete")
                    .execute(&mut raw)
                    .await
                    .unwrap();
                sqlx::query("DELETE FROM hub_corrections WHERE operation_id=?")
                    .bind(operation(1000).as_str())
                    .execute(&mut raw)
                    .await
                    .unwrap();
                sqlx::raw_sql(trigger("hub_corrections_no_delete"))
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
        }
        raw.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(count(&db, a, before).await.unwrap(), 0); // Future damaged body excluded.
        if fault == 2 {
            // Follow-up links are not the canonical accounting ownership index.
            assert_eq!(count(&db, a, after).await.unwrap(), 1);
            assert_eq!(count(&db, b, after).await.unwrap(), 0);
        } else {
            assert_eq!(
                count(&db, a, after).await.unwrap_err().code(),
                SafeErrorCode::StorageCorrupt
            );
            assert_eq!(
                count(&db, b, after).await.unwrap_err().code(),
                SafeErrorCode::StorageCorrupt
            );
        }
        db.close().await.unwrap();
    }
}
