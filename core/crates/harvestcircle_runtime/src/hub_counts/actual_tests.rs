//! Actual reports use the same native actor/custody fence as other mutations.
use super::*;
use harvestcircle_domain::hub::actuals::{
    ActualDuplicateLink, ActualNewDepartureCommand, ActualReportCommand, ActualReportSources,
    ActualReviewFacts,
};
use harvestcircle_storage::{
    HubActualLineageQuery, HubActualReportQuery, HubActualReviewOriginal, HubActualSourceQuery,
};
fn report() -> ActualReportCommand {
    ActualReportCommand {
        variant: None,
        recipient: None,
        packs: Some(packs(2)),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note("Two already-taken fictional crates; no source or time invented"),
    }
}
fn record_id(operation: &DurableRequestId) -> HubRecordId {
    HubRecordId::from_hex(&operation.as_str().replace('-', "")).unwrap()
}
#[tokio::test(flavor = "multi_thread")]
async fn actual_runtime_preserves_raw_reports_reconciles_once_and_links_without_another_effect() {
    let (actor, secrets, owner) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    actor
        .apply_hub_reference(
            DurableRequestId::new_v7(),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create {
                    id: id(5),
                    value: ReferenceValue::Party {
                        label: HubLabel::new("Fictional recipient").unwrap(),
                    },
                },
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let catalog = actor.hub_references().await.unwrap();
    let intake_op = DurableRequestId::new_v7();
    let intake = actor
        .receive_hub_stock(
            intake_op.clone(),
            ReceiptCommand {
                expected_stock_revision: HubRevision::INITIAL,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                supplier: None,
                producer: None,
                source: SourceAttribution::Unknown,
                packs: packs(20),
                receiving: id(3),
                planned_location: Some(id(4)),
                occurred: Occurrence::Known(now()),
                observation: note("Twenty fictional physical crates"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let group = intake.original.position.group;
    let trace = intake.original.trace.id;
    let op = DurableRequestId::new_v7();
    let input = report();
    let saved = actor
        .report_hub_actual(
            op.clone(),
            input.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(
        actor
            .report_hub_actual(
                op.clone(),
                input.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        saved
    );
    let raw = actor
        .hub_actual_reports(HubActualReportQuery::default())
        .await
        .unwrap();
    assert_eq!(raw.total, 1);
    assert_eq!(raw.rows[0].report, saved);
    assert_eq!(
        actor.hub_stock(query()).await.unwrap().rows[0]
            .position
            .packs,
        packs(20)
    );
    let evidence = actor
        .hub_actual_source(HubActualSourceQuery {
            context: HubActualLineageQuery {
                trace,
                expected_revision: Some(raw.revision),
                expected_catalog_revision: Some(raw.catalog_revision),
            },
            operation: intake_op.clone(),
            group,
        })
        .await
        .unwrap();
    assert_eq!(evidence.source.operation, record_id(&intake_op));
    let view = actor.hub_group(group).await.unwrap();
    let command = ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report: saved.original.id,
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group,
            expected_group_revision: view.group.revision,
            packs: packs(2),
            recipient: id(5),
            occurred: Occurrence::Known(now()),
            source_evidence: note("Original physical cohort verified"),
            quantity_evidence: note("Two whole crates verified"),
            recipient_evidence: note("Recipient confirmed independently"),
            timing_evidence: note("Physical time verified"),
        },
        source_operation: record_id(&intake_op),
        source_group: group,
        duplicate_review_basis: note("Complete native known departure history checked"),
    };
    let review_op = DurableRequestId::new_v7();
    let preview = actor
        .hub_actual_departure_preview(review_op.clone(), command.clone())
        .await
        .unwrap();
    assert_eq!(preview.revision, view.revision);
    assert_eq!(preview.catalog_revision, view.catalog_revision);
    assert_eq!(preview.plan.reduction.on_hand_after, packs(18));
    assert!(preview.historical_holds.is_empty());
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, review_op.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        actor.hub_stock(query()).await.unwrap().rows[0]
            .position
            .packs,
        packs(20)
    );
    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor.hub_actual_report_operation(op.clone()).await.unwrap(),
        Some(saved.clone())
    );
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap(),
        raw
    );
    assert_eq!(
        actor
            .reconcile_hub_actual(
                review_op.clone(),
                command.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, review_op.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        actor
            .hub_actual_departure_preview(review_op.clone(), command.clone())
            .await
            .unwrap(),
        preview
    );
    let blocked_report = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .report_hub_actual(
                blocked_report.clone(),
                report(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, blocked_report)
            .await
            .unwrap()
            .is_none()
    );
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();
    let reconciled = actor
        .reconcile_hub_actual(
            review_op.clone(),
            command.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(matches!(
        reconciled.original,
        HubActualReviewOriginal::Departure(_)
    ));
    assert_eq!(
        actor
            .reconcile_hub_actual(
                review_op.clone(),
                command,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        reconciled
    );
    let after = actor.hub_stock(query()).await.unwrap();
    assert_eq!(after.rows[0].position.packs, packs(18));
    assert_eq!(
        after.rows[0]
            .put_away
            .as_ref()
            .unwrap()
            .quantity
            .no_longer_required(),
        packs(2)
    );
    assert_eq!(
        after.rows[0]
            .put_away
            .as_ref()
            .unwrap()
            .quantity
            .remaining(),
        packs(18)
    );
    assert_eq!(
        actor.hub_actual_report_operation(op.clone()).await.unwrap(),
        Some(saved)
    );
    assert_eq!(
        actor
            .hub_actual_review_operation(review_op.clone())
            .await
            .unwrap(),
        Some(reconciled)
    );
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap()
            .total,
        0
    );
    let duplicate = actor
        .report_hub_actual(
            DurableRequestId::new_v7(),
            report(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let link = ActualDuplicateLink {
        report: duplicate.original.id,
        known_operation: record_id(&review_op),
        known_trace: trace,
        known_variant: id(2),
        verification: note("Same already-recorded two-crate event independently verified"),
    };
    let link_op = DurableRequestId::new_v7();
    let link_preview = actor
        .hub_actual_link_preview(link_op.clone(), link.clone())
        .await
        .unwrap();
    assert_eq!(link_preview.plan.known.operation, record_id(&review_op));
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, link_op.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap()
            .total,
        1
    );
    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor
            .link_hub_actual(
                link_op.clone(),
                link.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, link_op.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap().rows, after.rows);
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();
    assert_eq!(
        actor
            .hub_actual_link_preview(link_op.clone(), link.clone())
            .await
            .unwrap(),
        link_preview
    );
    let linked = actor
        .link_hub_actual(
            link_op.clone(),
            link.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(matches!(
        linked.original,
        HubActualReviewOriginal::Duplicate(_)
    ));
    assert_eq!(
        actor
            .link_hub_actual(
                link_op.clone(),
                link,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        linked
    );
    let final_page = actor.hub_stock(query()).await.unwrap();
    assert_eq!(final_page.rows, after.rows);
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        actor
            .hub_actual_review_operation(link_op.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    actor.activate_identity(owner).await.unwrap();
    assert_eq!(
        actor.hub_actual_review_operation(link_op).await.unwrap(),
        Some(linked)
    );
    assert_eq!(
        actor.hub_stock(query()).await.unwrap().rows,
        final_page.rows
    );
    actor.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn actual_runtime_stale_generation_and_expired_deadline_preserve_no_admission_or_effect() {
    let (actor, _, owner) = fixture().await;
    let old = actor.session_generation();
    actor.sign_out().await.unwrap();
    actor.activate_identity(owner).await.unwrap();
    assert_ne!(old, actor.session_generation());
    let before = actor.hub_stock(query()).await.unwrap();
    let stale = DurableRequestId::new_v7();
    let result = actor
        .dispatch_durable(
            RuntimeCommand::HubActual {
                generation: old,
                command: HubActualCommand::Report {
                    operation: stale.clone(),
                    command: Box::new(report()),
                },
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await;
    assert_eq!(
        result.err().unwrap().code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, stale)
            .await
            .unwrap()
            .is_none()
    );
    let expired = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .report_hub_actual(
                expired.clone(),
                report(),
                actor.snapshot().revision(),
                Duration::ZERO
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    // Lookup absence is observed for this forced pre-execution fixture only;
    // it never establishes safe replay after a real restore or lost reply.
    assert!(
        actor
            .hub_actual_report_operation(expired.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        actor
            ._adapter
            .database()
            .hub_submission(owner, expired)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap(), before);
    assert_eq!(
        actor
            .hub_actual_reports(HubActualReportQuery::default())
            .await
            .unwrap()
            .total,
        0
    );
    let saved = actor
        .report_hub_actual(
            DurableRequestId::new_v7(),
            ActualReportCommand {
                packs: None,
                occurred: Occurrence::Known(UnixTimestamp::from_seconds(500).unwrap()),
                ..report()
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(saved.original.report.packs, None);
    assert_eq!(saved.original.report.sources, ActualReportSources::Unknown);
    assert_eq!(
        saved.original.report.occurred,
        Occurrence::Known(UnixTimestamp::from_seconds(500).unwrap())
    );
    assert_eq!(
        saved.original.recording_clock_assessment(),
        harvestcircle_domain::hub::actuals::ActualReportedClockAssessment::RecordingClockConflict
    );
    assert_eq!(
        actor
            .hub_actual_report_operation(saved.operation.clone())
            .await
            .unwrap(),
        Some(saved)
    );
    assert!(actor.hub_stock(query()).await.unwrap().rows.is_empty());
    actor.close().await.unwrap();
}
