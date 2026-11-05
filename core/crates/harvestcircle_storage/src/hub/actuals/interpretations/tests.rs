use super::*;
use crate::hub::stock::tests::{Fixture, command, now, operation, other, owner, setup};
use harvestcircle_domain::{SafeErrorCode, hub::HubRevision};
use std::collections::BTreeSet;

#[tokio::test]
async fn catalogue_only_change_advances_catalogue_cut_without_rewriting_stock_recording_clock() {
    use crate::hub::stock::tests::id;
    use harvestcircle_domain::{
        UnixTimestamp,
        hub::reference::{HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let query = HubCorrectionKnownQuery {
        trace: receipt.original.trace.id,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    let original = db
        .hub_actual_known_interpretations(owner(), query.clone())
        .await
        .unwrap();
    db.apply_hub_reference(
        owner(),
        operation(200),
        ReferenceCommand {
            expected_catalog_revision: original.interpretations.catalog_revision,
            mutation: ReferenceMutation::Revise {
                id: id(4),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Later catalogue name, original stock time").unwrap(),
                },
            },
        },
        UnixTimestamp::from_seconds(400).unwrap(),
    )
    .await
    .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let current = db
        .hub_actual_known_interpretations(owner(), query)
        .await
        .unwrap();
    assert_eq!(
        current.interpretations.stock_revision,
        original.interpretations.stock_revision
    );
    assert!(
        current.interpretations.catalog_revision.value()
            > original.interpretations.catalog_revision.value()
    );
    assert_eq!(current.interpretations.latest_recorded_at, now());
    assert_eq!(
        current.interpretations.latest_recorded_at,
        original.interpretations.latest_recorded_at
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn empty_known_cut_retains_clock_owner_and_paired_revision_without_any_write() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let query = HubCorrectionKnownQuery {
        trace: receipt.original.trace.id,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_actual_known_interpretations(owner(), query.clone())
        .await
        .unwrap();
    assert!(evidence.interpretations.records.is_empty());
    assert_eq!(evidence.interpretations.latest_recorded_at, now());
    assert!(!evidence.uncertain_intent && !evidence.unresolved_actual);
    let pinned = HubCorrectionKnownQuery {
        expected_stock_revision: Some(evidence.interpretations.stock_revision),
        expected_catalog_revision: Some(evidence.interpretations.catalog_revision),
        ..query.clone()
    };
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), pinned.clone())
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(
        db.hub_actual_known_interpretations(other(), query.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_actual_known_interpretations(
            owner(),
            HubCorrectionKnownQuery {
                expected_catalog_revision: None,
                ..pinned.clone()
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_actual_known_interpretations(
            owner(),
            HubCorrectionKnownQuery {
                expected_stock_revision: Some(
                    HubRevision::from_value(evidence.interpretations.stock_revision.value() - 1)
                        .unwrap()
                ),
                ..pinned
            }
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), query)
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_interpretation_keeps_whole_normal_event_populations_paper_and_every_line() {
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
    for trace in &traces {
        let evidence = db
            .hub_actual_known_interpretations(
                owner(),
                HubCorrectionKnownQuery {
                    trace: *trace,
                    expected_stock_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(evidence.interpretations.records.len(), 1);
        let entry = &evidence.interpretations.records[0];
        assert!(entry.count_explanation.is_none());
        assert_eq!(
            serde_json::to_vec(entry).unwrap(),
            serde_json::to_vec(&Physical20Capture(entry)).unwrap()
        );
        physical20_link_bytes(entry);
        assert!(
            !String::from_utf8(serde_json::to_vec(&evidence.interpretations).unwrap())
                .unwrap()
                .contains("count_explanation")
        );
        assert_eq!(
            entry
                .recipient_original
                .populations
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            traces
        );
        assert_eq!(
            entry.recipient_original.issued_versions,
            departure
                .record
                .paper
                .versions
                .iter()
                .map(|v| v.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            entry.quantities.len(),
            departure
                .record
                .consumed
                .iter()
                .filter(|line| line.before.trace == *trace)
                .count()
        );
        let view = entry.view().unwrap();
        assert_eq!(view.original_packs, view.effective_packs);
        assert!(!view.recipients.has_unknown());
        assert!(!evidence.uncertain_intent && !evidence.unresolved_actual);
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

// Exact pre-extension field sequence. This compatibility oracle deliberately
// retains the native20 codec, independently of the current struct derive.
struct Physical20Capture<'a>(&'a harvestcircle_domain::hub::actuals::ActualKnownInterpretation);
impl serde::Serialize for Physical20Capture<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let value = self.0;
        let mut record = serializer.serialize_struct("ActualKnownInterpretation", 7)?;
        record.serialize_field("stock_revision", &value.stock_revision)?;
        record.serialize_field("catalog_revision", &value.catalog_revision)?;
        record.serialize_field("latest_recorded_at", &value.latest_recorded_at)?;
        record.serialize_field("original", &value.original)?;
        record.serialize_field("recipient_original", &value.recipient_original)?;
        record.serialize_field("recipient_history", &value.recipient_history)?;
        record.serialize_field("quantities", &value.quantities)?;
        record.end()
    }
}

fn physical20_link_bytes(entry: &harvestcircle_domain::hub::actuals::ActualKnownInterpretation) {
    use harvestcircle_domain::hub::actuals::{
        ActualDuplicateLink, ActualReportCommand, ActualReportSources,
    };
    use harvestcircle_domain::hub::reference::SourceProvenance;
    let raw = ActualReportCommand {
        variant: Some(entry.original.variant.id),
        recipient: Some(entry.original.recipient.id),
        packs: None,
        sources: ActualReportSources::Unknown,
        occurred: entry.original.occurred,
        observation: SourceProvenance::new("Independent physical20 byte compatibility witness")
            .unwrap(),
    }
    .preserve(
        entry.original.hub,
        entry.stock_revision,
        entry.catalog_revision,
        crate::hub::stock::tests::id(240),
        owner(),
        entry.latest_recorded_at,
    )
    .unwrap();
    let mut current = entry.clone();
    current.stock_revision = raw.stock_revision;
    let command = ActualDuplicateLink {
        report: raw.id,
        known_operation: entry.original.operation,
        known_trace: entry.original.trace,
        known_variant: entry.original.variant.id,
        verification: SourceProvenance::new("Verified duplicate physical20 byte witness").unwrap(),
    };
    for qualified in [false, true] {
        let plan = if qualified {
            command.plan_with_interpretation(
                &raw,
                &current,
                None,
                crate::hub::stock::tests::id(241),
                owner(),
                raw.recorded_at,
            )
        } else {
            command.plan(
                &raw,
                &entry.original,
                None,
                crate::hub::stock::tests::id(241),
                owner(),
                raw.recorded_at,
            )
        }
        .unwrap();
        assert!(plan.count_source.is_none());
        assert_eq!(
            serde_json::to_vec(&plan).unwrap(),
            serde_json::to_vec(&Physical20Link(&plan)).unwrap()
        );
    }
}
struct Physical20Link<'a>(&'a harvestcircle_domain::hub::actuals::ActualDuplicateLinkPlan);
impl serde::Serialize for Physical20Link<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let value = self.0;
        let mut record = serializer.serialize_struct(
            "ActualDuplicateLinkPlan",
            if value.interpretation.is_some() { 7 } else { 6 },
        )?;
        record.serialize_field("operation", &value.operation)?;
        record.serialize_field("original", &value.original)?;
        record.serialize_field("known", &value.known)?;
        if let Some(interpretation) = &value.interpretation {
            record.serialize_field("interpretation", interpretation)?;
        }
        record.serialize_field("actor", &value.actor)?;
        record.serialize_field("recorded_at", &value.recorded_at)?;
        record.serialize_field("verification", &value.verification)?;
        record.end()
    }
}
