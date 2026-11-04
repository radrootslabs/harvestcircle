//! Fresh published19 native originals for forward interpretation-mode acceptance.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualDuplicateLink, ActualReportCommand, ActualReportSources},
    corrections::{
        AdditionalQuantityCommand, CorrectedLineFacts, PhysicalDebitKey, SourceAttributionCommand,
    },
    reference::{
        SourceAttribution, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
        WholePacks,
    },
};
use radroots_service_sqlite::MigrationBuildIdentity;
mod retain;

const SOURCE: &str = "8549a01302c14c8f7c1bff9437f349d9adaf8454";
const LIB: &str = "189c49b74b4bafc142b00b76b296477931139e72";
const LINK: u32 = 30001;
const ADDITIONAL: u32 = 30003;
const FOUND_ORIGINAL: u32 = 30010;
const FOUND_ADDITIONAL: u32 = 30020;
const FIRST_SOURCE: u32 = 30030;
const UNKNOWN_RECIPIENT: u32 = 30031;
const KNOWN_RECIPIENT: u32 = 30032;
const SECOND_SOURCE: u32 = 30033;
const UNRESOLVED: u32 = 30034;

fn build() -> MigrationBuildIdentity {
    MigrationBuildIdentity::new(
        "0.1.0-alpha",
        SOURCE,
        LIB,
        "1.99.0",
        "aarch64-apple-darwin",
        "populated19-native-interpretation-predecessor",
        1,
        19,
        1,
        1,
        1,
    )
    .unwrap()
}
fn actual_value(receipt: &crate::HubActualReviewReceipt) -> serde_json::Value {
    match &receipt.original {
        crate::HubActualReviewOriginal::Departure(plan) => {
            serde_json::json!({"kind":"departure","plan":plan})
        }
        crate::HubActualReviewOriginal::Duplicate(plan) => {
            assert!(
                plan.interpretation.is_none(),
                "published19 legacy plan has no native20 extension"
            );
            serde_json::json!({"kind":"link","plan":plan})
        }
    }
}
async fn source_command(
    db: &Database,
    trace: HubRecordId,
    reference: u8,
    prior: Option<HubRecordId>,
) -> SourceAttributionCommand {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    SourceAttributionCommand {
        trace,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: prior,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(reference),
                issuer: Some(id(3)),
                kind: SourceReferenceKind::Other,
                code: SourceCode::new(format!("Fictional published19 source {reference}")).unwrap(),
                provenance: note(),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
        basis: note(),
    }
}

async fn native_baseline(db: &Database) -> serde_json::Value {
    let actual = db
        .hub_actual_review_operation(owner(), operation(81))
        .await
        .unwrap()
        .unwrap();
    let link = db
        .hub_actual_review_operation(owner(), operation(LINK))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = &actual.original else {
        panic!("actual")
    };
    let trace = plan.reduction.before.trace;
    let known = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(known.departures.len(), 1);
    assert!(known.unresolved_actual && !known.uncertain_intent);
    let entry = &known.departures[0];
    assert_eq!(entry.original.packs().unwrap().value(), 2);
    assert_eq!(entry.quantities.len(), 1);
    assert_eq!(entry.quantities[0].effective().value(), 0);
    assert_eq!(entry.recipient_history.len(), 2);
    assert!(matches!(
        entry.recipient_history[0].after,
        AmendedRecipient::Unknown
    ));
    assert!(matches!(&entry.recipient_history[1].after,AmendedRecipient::Known(r) if r.id==id(4)));
    let additional = db
        .hub_additional_quantity_original(owner(), operation(ADDITIONAL))
        .await
        .unwrap()
        .unwrap();
    let first_found = db
        .hub_found_correction_original(owner(), operation(FOUND_ORIGINAL + 1))
        .await
        .unwrap()
        .unwrap();
    let second_found = db
        .hub_found_correction_original(owner(), operation(FOUND_ADDITIONAL + 1))
        .await
        .unwrap()
        .unwrap();
    let first_source = db
        .hub_source_attribution_original(owner(), operation(FIRST_SOURCE))
        .await
        .unwrap()
        .unwrap();
    let second_source = db
        .hub_source_attribution_original(owner(), operation(SECOND_SOURCE))
        .await
        .unwrap()
        .unwrap();
    let unknown = db
        .hub_recipient_attribution_original(owner(), operation(UNKNOWN_RECIPIENT))
        .await
        .unwrap()
        .unwrap();
    let current = db
        .hub_recipient_attribution_original(owner(), operation(KNOWN_RECIPIENT))
        .await
        .unwrap()
        .unwrap();
    let unresolved = db
        .hub_actual_report_operation(owner(), operation(UNRESOLVED))
        .await
        .unwrap()
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        20
    );
    serde_json::json!({"actual":actual_value(&actual),"link":actual_value(&link),
        "additional":additional.original,"found_original":first_found.original,"found_additional":second_found.original,
        "first_source":first_source.original,"second_source":second_source.original,
        "unknown_recipient":unknown.original,"current_recipient":current.original,
        "unresolved_raw":unresolved.original,
        "known":{"revision":known.revision,"catalog_revision":known.catalog_revision,
            "original":entry.original,"recipient_original":entry.recipient_original,
            "recipient_history":entry.recipient_history,"quantities":entry.quantities},
        "total_on_hand":20})
}

async fn drive_native19(db: &Database) -> serde_json::Value {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 19);
    let (before, target, _) =
        crate::hub::corrections::projection_native_tests::actual_debit(db).await;
    let target_id = crate::hub::actuals::common::operation_id(&target).unwrap();
    let original = db
        .hub_actual_review_operation(owner(), target.clone())
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = &original.original else {
        panic!("actual")
    };
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let raw = |packs, recipient| ActualReportCommand {
        variant: Some(before.variant),
        recipient,
        packs,
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    let crate::HubMutationResult::ActualReported(duplicate) = db
        .submit_hub_mutation(
            scope,
            operation(30000),
            HubMutation::ActualReport(Box::new(raw(
                Some(WholePacks::from_value(2).unwrap()),
                Some(id(3)),
            ))),
            at(190),
        )
        .await
        .unwrap()
    else {
        panic!("duplicate raw")
    };
    let link_command = ActualDuplicateLink {
        report: duplicate.original.id,
        known_operation: target_id,
        known_trace: before.trace,
        known_variant: before.variant,
        verification: note(),
    };
    let crate::HubMutationResult::ActualReviewed(link) = db
        .submit_hub_mutation(
            scope,
            operation(LINK),
            HubMutation::ActualLink(Box::new(link_command.clone())),
            at(191),
        )
        .await
        .unwrap()
    else {
        panic!("link")
    };
    let crate::HubMutationResult::ActualReported(corrected) = db
        .submit_hub_mutation(
            scope,
            operation(30002),
            HubMutation::ActualReport(Box::new(raw(
                Some(WholePacks::from_value(4).unwrap()),
                Some(id(3)),
            ))),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("corrected raw")
    };
    let view = db.hub_group(owner(), before.id).await.unwrap();
    let additional_command = AdditionalQuantityCommand {
        facts: CorrectedLineFacts {
            report: corrected.original.id,
            target_operation: target_id,
            target_group: before.id,
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            expected_prior: None,
            corrected_line_packs: WholePacks::from_value(4).unwrap(),
            recipient: Some(id(3)),
            occurred: Occurrence::Known(at(110)),
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: plan.historical_source.historical().operation,
        source_group: before.id,
        balancing_group: before.id,
        expected_balancing_revision: view.group.revision,
        duplicate_review_basis: note(),
    };
    let crate::HubMutationResult::AdditionalCorrected(additional) = db
        .submit_hub_mutation(
            scope,
            operation(ADDITIONAL),
            HubMutation::AdditionalQuantity(Box::new(additional_command)),
            at(230),
        )
        .await
        .unwrap()
    else {
        panic!("additional")
    };
    assert_eq!(additional.original.amendment.after.value(), 4);
    crate::hub::corrections::additional_write_tests::found(
        db,
        scope,
        &target,
        before.id,
        PhysicalDebitKey {
            operation: target_id,
            group: before.id,
        },
        FOUND_ORIGINAL,
        250,
    )
    .await;
    crate::hub::corrections::additional_write_tests::found(
        db,
        scope,
        &target,
        before.id,
        PhysicalDebitKey {
            operation: crate::hub::actuals::common::operation_id(&operation(ADDITIONAL)).unwrap(),
            group: before.id,
        },
        FOUND_ADDITIONAL,
        260,
    )
    .await;
    let first = source_command(db, before.trace, 90, None).await;
    let crate::HubMutationResult::SourceAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(FIRST_SOURCE),
            HubMutation::SourceAttribution(Box::new(first)),
            at(300),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let unknown = command(db, target_id, None, None).await;
    let crate::HubMutationResult::RecipientAttributed(unknown) = db
        .submit_hub_mutation(
            scope,
            operation(UNKNOWN_RECIPIENT),
            HubMutation::RecipientAttribution(Box::new(unknown)),
            at(320),
        )
        .await
        .unwrap()
    else {
        panic!("unknown")
    };
    let current = command(
        db,
        target_id,
        Some(id(4)),
        Some(unknown.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(_) = db
        .submit_hub_mutation(
            scope,
            operation(KNOWN_RECIPIENT),
            HubMutation::RecipientAttribution(Box::new(current)),
            at(340),
        )
        .await
        .unwrap()
    else {
        panic!("current")
    };
    let second = source_command(db, before.trace, 91, Some(first.original.stamp.operation)).await;
    let crate::HubMutationResult::SourceAttributed(_) = db
        .submit_hub_mutation(
            scope,
            operation(SECOND_SOURCE),
            HubMutation::SourceAttribution(Box::new(second)),
            at(360),
        )
        .await
        .unwrap()
    else {
        panic!("second source")
    };
    let crate::HubMutationResult::ActualReported(_) = db
        .submit_hub_mutation(
            scope,
            operation(UNRESOLVED),
            HubMutation::ActualReport(Box::new(raw(None, None))),
            at(380),
        )
        .await
        .unwrap()
    else {
        panic!("unresolved")
    };
    assert_eq!(
        db.hub_actual_review_operation(owner(), target)
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(LINK))
            .await
            .unwrap()
            .as_ref(),
        Some(link.as_ref())
    );
    let crate::HubMutationResult::ActualReviewed(replayed) = db
        .submit_hub_mutation(
            scope,
            operation(LINK),
            HubMutation::ActualLink(Box::new(link_command)),
            at(400),
        )
        .await
        .unwrap()
    else {
        panic!("same link")
    };
    assert_eq!(replayed, link);
    native_baseline(db).await
}

#[tokio::test]
#[ignore = "Explicit disposable native19 precursor qualification before forward20; never substitute a later schema"]
async fn fresh_native19_legacy_sequence_preserves_actual_link_and_zero_quantity_originals() {
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    let mut fixture = Fixture::new();
    fixture.build = build();
    let db = fixture.open().await;
    let baseline = drive_native19(&db).await;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(native_baseline(&db).await, baseline);
    db.close().await.unwrap();
}
