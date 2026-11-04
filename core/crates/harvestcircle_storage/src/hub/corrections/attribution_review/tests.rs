use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, id, operation, other, owner, query, setup,
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        reference::{
            HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceCertainty,
            SourceCode, SourceProvenance, SourceReference, SourceReferenceKind,
        },
        stock::ReceiptIdentities,
    },
};
use sqlx::{AssertSqlSafe, Connection};
mod actual_tests;
mod origin_tests;

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn source(
    id: HubRecordId,
    issuer: Option<HubRecordId>,
    certainty: SourceCertainty,
) -> SourceAttribution {
    SourceAttribution::Referenced {
        source: SourceReference {
            id,
            issuer,
            kind: SourceReferenceKind::SupplierReference,
            code: SourceCode::new("SAME-SOURCE-CODE").unwrap(),
            provenance: note("Independently identified original source, never code matching"),
        },
        certainty,
    }
}
fn source_id(n: u32) -> HubRecordId {
    ReceiptIdentities::from_operation_bytes(
        crate::hub::actuals::common::operation_id(&operation(n)).unwrap(),
    )
    .unwrap()
    .source
}
async fn world() -> (Fixture, Database, Vec<crate::HubStockReceipt>) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut receipts = Vec::new();
    for n in 1..=3 {
        let mut input = receipt_command(&catalog);
        if n > 1 {
            input.expected_stock_revision =
                db.hub_stock_page(owner(), query()).await.unwrap().revision;
        }
        input.source = if n == 2 {
            source(source_id(2), Some(id(4)), SourceCertainty::Established)
        } else {
            source(
                source_id(1),
                Some(id(3)),
                if n == 1 {
                    SourceCertainty::ReportedPossible
                } else {
                    SourceCertainty::Established
                },
            )
        };
        receipts.push(
            db.apply_hub_receipt(owner(), operation(n), input, at(100))
                .await
                .unwrap(),
        );
    }
    (fixture, db, receipts)
}
async fn source_command(
    db: &Database,
    trace: HubRecordId,
    source: SourceAttribution,
) -> SourceAttributionCommand {
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    SourceAttributionCommand {
        trace,
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        expected_prior: None,
        source,
        basis: note("Independently compared original and complete current source identities"),
    }
}
async fn recipient_command(
    db: &Database,
    target: &DurableRequestId,
    recipient: Option<HubRecordId>,
) -> RecipientAttributionCommand {
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    RecipientAttributionCommand {
        departure_operation: crate::hub::actuals::common::operation_id(target).unwrap(),
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        expected_prior: None,
        recipient,
        basis: note("Whole immutable event and every original issued version reviewed"),
    }
}

#[tokio::test]
async fn source_registry_keeps_same_code_distinct_issuers_and_reuse_without_mutation_after_reopen()
{
    let (fixture, db, receipts) = world().await;
    let command = source_command(
        &db,
        receipts[0].original.trace.id,
        receipts[1].original.trace.source.clone(),
    )
    .await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let review = db
        .hub_source_attribution_review(owner(), operation(1900), command.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(review.known_sources.len(), 2);
    assert!(
        review
            .known_sources
            .contains(&receipts[0].original.trace.source)
    );
    assert!(
        review
            .known_sources
            .contains(&receipts[1].original.trace.source)
    );
    assert!(review.affected.is_empty());
    let HubAttributionReviewOutcome::Candidate(plan) = &review.outcome else {
        panic!("source candidate {:?}", review.outcome)
    };
    assert_eq!(plan.before.source, receipts[0].original.trace.source);
    assert_eq!(plan.after.source, receipts[1].original.trace.source);
    for target in [
        harvestcircle_domain::hub::corrections::CorrectionImpact::Source(source_id(1)),
        harvestcircle_domain::hub::corrections::CorrectionImpact::Source(source_id(2)),
        harvestcircle_domain::hub::corrections::CorrectionImpact::UnresolvedSource(
            receipts[0].original.trace.id,
        ),
    ] {
        assert!(plan.impact.contains(target));
    }
    let third = source_command(
        &db,
        receipts[2].original.trace.id,
        SourceAttribution::Unknown,
    )
    .await;
    let third_review = db
        .hub_source_attribution_review(owner(), operation(1901), third, at(200))
        .await
        .unwrap();
    assert_eq!(
        third_review.original.source.source,
        receipts[2].original.trace.source
    );
    assert_ne!(
        third_review.original.source.source,
        review.original.source.source
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap(),
        review
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn source_identity_conflict_no_change_unknown_and_possible_stay_distinct() {
    let (fixture, db, receipts) = world().await;
    let command = source_command(
        &db,
        receipts[0].original.trace.id,
        receipts[0].original.trace.source.clone(),
    )
    .await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::Blocked(AttributionError::NoChange)
    );
    let mut conflict = command.clone();
    let SourceAttribution::Referenced {
        source: reference, ..
    } = &mut conflict.source
    else {
        unreachable!()
    };
    reference.provenance = note("Changed content under the same immutable source ID");
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), conflict, at(200))
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::Blocked(AttributionError::SourceIdentityConflict)
    );
    for after in [
        SourceAttribution::Unknown,
        source(id(60), None, SourceCertainty::ReportedPossible),
        source(id(61), Some(id(3)), SourceCertainty::Established),
    ] {
        let mut next = command.clone();
        next.source = after.clone();
        let review = db
            .hub_source_attribution_review(owner(), operation(1900), next, at(200))
            .await
            .unwrap();
        let HubAttributionReviewOutcome::Candidate(plan) = review.outcome else {
            panic!("distinct attribution")
        };
        assert_eq!(plan.after.source, after);
        assert!(plan.impact.contains(
            harvestcircle_domain::hub::corrections::CorrectionImpact::UnresolvedSource(
                command.trace
            )
        ));
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn whole_event_source_and_recipient_comparisons_keep_all_populations_and_issued_versions() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let departure = crate::hub::handling::departure::tests::multi_population_departure(&db).await;
    assert_eq!(departure.record.consumed.len(), 3);
    let trace = departure.record.consumed[0].before.trace;
    let source_command = source_command(
        &db,
        trace,
        source(id(60), Some(id(3)), SourceCertainty::Established),
    )
    .await;
    let recipient_command = recipient_command(&db, &departure.operation, None).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let sources = db
        .hub_source_attribution_review(owner(), operation(1900), source_command.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(sources.affected.len(), 1);
    assert_eq!(sources.affected[0].populations.len(), 2);
    assert_eq!(
        sources.affected[0].issued_versions,
        departure
            .record
            .paper
            .versions
            .iter()
            .map(|v| v.id)
            .collect::<Vec<_>>()
    );
    let HubAttributionReviewOutcome::Candidate(source_plan) = &sources.outcome else {
        panic!("source plan")
    };
    for population in &sources.affected[0].populations {
        assert!(source_plan.impact.contains(
            harvestcircle_domain::hub::corrections::CorrectionImpact::Population(*population)
        ));
    }
    let recipients = db
        .hub_recipient_attribution_review(
            owner(),
            operation(1901),
            recipient_command.clone(),
            at(200),
        )
        .await
        .unwrap();
    assert_eq!(
        recipients.original.populations,
        sources.affected[0].populations
    );
    assert_eq!(
        recipients.linked_issued_versions,
        sources.affected[0].issued_versions
    );
    let HubAttributionReviewOutcome::Candidate(recipient_plan) = &recipients.outcome else {
        panic!("recipient plan")
    };
    assert_eq!(recipient_plan.after, AmendedRecipient::Unknown);
    assert_eq!(
        recipient_plan.before,
        AmendedRecipient::Known(recipients.original.recipient.clone())
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), source_command, at(200))
            .await
            .unwrap(),
        sources
    );
    assert_eq!(
        db.hub_recipient_attribution_review(owner(), operation(1901), recipient_command, at(200))
            .await
            .unwrap(),
        recipients
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn owner_cuts_used_id_reference_and_reference_clock_are_checked_without_mutation() {
    let (fixture, db, receipts) = world().await;
    let command = source_command(
        &db,
        receipts[0].original.trace.id,
        SourceAttribution::Unknown,
    )
    .await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_source_attribution_review(other(), operation(1900), command.clone(), at(200))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = command.clone();
    stale.expected_stock_revision = HubRevision::INITIAL;
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), stale, at(200))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::OperationAlreadyUsed
    );
    let mut invalid = command.clone();
    invalid.source = source(id(60), Some(id(1)), SourceCertainty::Established);
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), invalid, at(200))
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::Blocked(AttributionError::InvalidReference)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(1800),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(3),
                expected_record_revision: catalog.get(id(3)).unwrap().revision,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Later party label, immutable old issuer remains")
                        .unwrap(),
                },
            },
        },
        at(250),
    )
    .await
    .unwrap();
    let command = source_command(&db, command.trace, SourceAttribution::Unknown).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1900), command, at(240))
            .await
            .unwrap()
            .outcome,
        HubAttributionReviewOutcome::Blocked(AttributionError::InvalidHistory)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn unrelated_registry_content_creator_and_orphan_faults_cannot_hide_behind_trace_selection() {
    for fault in 0..4 {
        let (fixture, db, receipts) = world().await;
        let command = source_command(
            &db,
            receipts[1].original.trace.id,
            SourceAttribution::Unknown,
        )
        .await;
        let mut raw = fixture.raw().await;
        if fault == 3 {
            // A later reuse can disappear while both registry identities and
            // their original creators remain valid. The operation is still an
            // original population and must never silently fall out of scope.
            origin_tests::remove_population(&mut raw, &operation(3)).await;
        } else if fault == 2 {
            sqlx::query("INSERT INTO hub_source_references (source_id,hub_id,issuer_id,issuer_revision,kind,source_code,provenance,created_operation_id) SELECT ?,hub_id,issuer_id,issuer_revision,kind,'ORPHAN','No canonical original association',created_operation_id FROM hub_source_references LIMIT 1")
                .bind(id(99).as_bytes().as_slice()).execute(&mut raw).await.unwrap();
        } else {
            let triggers:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name='hub_source_references' ORDER BY name").fetch_all(&mut raw).await.unwrap();
            for (name, _) in &triggers {
                sqlx::query(AssertSqlSafe(format!(
                    "DROP TRIGGER \"{}\"",
                    name.replace('"', "\"\"")
                )))
                .execute(&mut raw)
                .await
                .unwrap();
            }
            if fault == 0 {
                sqlx::query(
                    "UPDATE hub_source_references SET source_code='DAMAGED' WHERE source_id=?",
                )
                .bind(source_id(1).as_bytes().as_slice())
                .execute(&mut raw)
                .await
                .unwrap();
            } else {
                sqlx::query(
                    "UPDATE hub_source_references SET created_operation_id=? WHERE source_id=?",
                )
                .bind(operation(3).as_str())
                .bind(source_id(1).as_bytes().as_slice())
                .execute(&mut raw)
                .await
                .unwrap();
            }
            for (_, ddl) in triggers {
                sqlx::query(AssertSqlSafe(ddl))
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
        }
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_source_attribution_review(owner(), operation(1900), command, at(200))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "fault {fault}"
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}
