//! Calls the production exported methods, not an actor-only stand-in.
use super::*;
use crate::{
    HubActualClockAssessmentDto, HubActualDepartureInputDto, HubActualHoldAssessmentDto,
    HubActualHoldTimingDto, HubActualHoldUncertaintyDto, HubActualLinkInputDto,
    HubActualReportFilterDto, HubActualReportInputDto, HubActualReportQueryDto,
    HubActualReviewOriginalDto, HubActualSourceClaimDto, HubActualSourceKindDto,
    HubHandlingInputDto, HubHoldInputDto, HubPhysicalIdentificationDto, HubSplitInputDto,
    HubTreatmentScopeDto,
};
fn unknown() -> HubOccurrenceDto {
    HubOccurrenceDto {
        kind: HubOccurrenceKindDto::Unknown,
        start_unix_s: None,
        end_unix_s: None,
    }
}
fn known(at: i64) -> HubOccurrenceDto {
    HubOccurrenceDto {
        kind: HubOccurrenceKindDto::Known,
        start_unix_s: Some(at),
        end_unix_s: None,
    }
}
fn report() -> HubActualReportInputDto {
    HubActualReportInputDto {
        variant_id: None,
        recipient_id: None,
        whole_packs: None,
        source_kind: HubActualSourceKindDto::Unknown,
        source_claims: vec![],
        occurrence: unknown(),
        observation: "Already-taken fictional report; original fields stay unverified".into(),
    }
}
fn reports() -> HubActualReportQueryDto {
    HubActualReportQueryDto {
        expected_stock_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
        filter: HubActualReportFilterDto::Unresolved,
    }
}
#[tokio::test]
async fn actual_ffi_preserves_raw_reports_and_native_hold_findings_through_preview_effect_and_zero_effect_link()
 {
    for mode in 0..5 {
        let (core, ids, catalog) = fixture().await;
        let recipient = core
            .apply_hub_reference(
                context(&core),
                HubReferenceEditDto {
                    expected_catalog_revision: catalog,
                    record_id: None,
                    expected_record_revision: None,
                    value: HubReferenceInputDto {
                        kind: HubReferenceKindDto::Party,
                        label: "Fictional recipient".into(),
                        product_id: None,
                        nominal_grams: None,
                        location_purpose: None,
                    },
                },
            )
            .await
            .unwrap();
        let now = cutoff().start_unix_s.unwrap();
        let received_context = context(&core);
        let intake = core
            .receive_hub_stock(
                received_context.clone(),
                HubReceiptInputDto {
                    expected_stock_revision: 1,
                    expected_catalog_revision: recipient.catalog_revision,
                    variant_id: ids[1].clone(),
                    supplier_id: None,
                    producer_id: None,
                    source: unknown_source(),
                    whole_packs: 20,
                    receiving_id: ids[2].clone(),
                    planned_location_id: Some(ids[3].clone()),
                    occurrence: known(now - 60),
                    observation: "Twenty originally observed fictional crates".into(),
                },
            )
            .await
            .unwrap();
        let group_id = intake.initial_position.group_id.clone();
        if mode == 4 {
            core.apply_hub_handling(
                context(&core),
                HubHandlingInputDto::Split {
                    input: HubSplitInputDto {
                        expected_stock_revision: intake.stock_revision,
                        expected_catalog_revision: recipient.catalog_revision,
                        group_id: group_id.clone(),
                        expected_group_revision: intake.initial_position.revision,
                        whole_packs: 6,
                        identification: HubPhysicalIdentificationDto {
                            label: "Six separate fictional crates".into(),
                            observation: "Observed before the later hold".into(),
                        },
                        occurrence: known(now - 50),
                        observation: "Earlier physical split narrows the remaining cohort".into(),
                    },
                },
            )
            .await
            .unwrap();
        }
        let before_hold = core.hub_group(group_id.clone()).await.unwrap();

        core.apply_hub_handling(
            context(&core),
            HubHandlingInputDto::PlaceHold {
                input: HubHoldInputDto {
                    expected_stock_revision: before_hold.stock_revision,
                    expected_catalog_revision: before_hold.catalog_revision,
                    group_ids: vec![group_id.clone()],
                    occurrence: if mode == 3 {
                        unknown()
                    } else {
                        known(now - 30)
                    },
                    reason: "Current restriction remains independent of historical reconciliation"
                        .into(),
                },
            },
        )
        .await
        .unwrap();
        let scope = HubTreatmentScopeDto {
            trace_id: intake.trace.id.clone(),
            expected_stock_revision: None,
            expected_catalog_revision: None,
        };
        let treatment = core.hub_treatment(scope.clone()).await.unwrap();
        let report_context = context(&core);
        let saved = core
            .report_hub_actual(report_context.clone(), report())
            .await
            .unwrap();
        assert_eq!(saved.reported, report());
        assert_eq!(saved.clock_assessment, HubActualClockAssessmentDto::Unknown);
        assert_eq!(core.hub_actual_reports(reports()).await.unwrap().total, 1);
        let before = core.hub_stock(query()).await.unwrap();
        assert_eq!(
            before
                .rows
                .iter()
                .map(|row| row.position.whole_packs)
                .sum::<u32>(),
            20
        );
        assert_eq!(
            before
                .rows
                .iter()
                .find(|row| row.position.group_id == group_id)
                .unwrap()
                .put_away
                .as_ref()
                .unwrap()
                .remaining,
            20
        );
        let view = core.hub_group(group_id.clone()).await.unwrap();
        let occurred = match mode {
            1 => known(now - 45),
            2 => HubOccurrenceDto {
                kind: HubOccurrenceKindDto::Interval,
                start_unix_s: Some(now - 45),
                end_unix_s: Some(now - 10),
            },
            _ => known(now - 10),
        };
        let input = HubActualDepartureInputDto {
            report_id: saved.id.clone(),
            expected_stock_revision: view.stock_revision,
            expected_catalog_revision: view.catalog_revision,
            group_id: group_id.clone(),
            expected_group_revision: view.group.revision,
            whole_packs: 2,
            recipient_id: recipient.current.id.clone(),
            occurrence: occurred.clone(),
            source_operation_id: received_context.request_id.clone(),
            source_group_id: group_id.clone(),
            source_evidence: "Original cohort independently verified".into(),
            quantity_evidence: "Two whole crates verified".into(),
            recipient_evidence: "Actual recipient verified".into(),
            timing_evidence: "Actual occurrence reviewed independently of current hold".into(),
            duplicate_review_basis: "Complete native departure originals reviewed".into(),
        };
        let mut review_context = context(&core);
        let preview = core
            .hub_actual_departure_preview(review_context.request_id.clone(), input.clone())
            .await
            .unwrap();
        assert_eq!(preview.report, saved);
        assert_eq!(preview.report.reported.whole_packs, None);
        assert_eq!(preview.proposed.whole_packs, 2);
        assert_eq!(
            preview.report.reported.source_kind,
            HubActualSourceKindDto::Unknown
        );
        let remaining = if mode == 4 { 12 } else { 18 };
        assert_eq!(preview.proposed.on_hand_after_whole_packs, remaining);
        assert_eq!(preview.proposed.reported_occurrence, unknown());
        assert_eq!(preview.proposed.occurrence, occurred);
        assert_eq!(
            preview.proposed.source_operation_id,
            received_context.request_id
        );
        assert_eq!(preview.historical_holds.len(), 1);
        match &preview.historical_holds[0] {
            HubActualHoldAssessmentDto::Verified { timing, .. } => assert_eq!(
                *timing,
                match mode {
                    0 => HubActualHoldTimingDto::EstablishedBreach,
                    1 => HubActualHoldTimingDto::BeforeHold,
                    2 | 3 => HubActualHoldTimingDto::TimingUnknown,
                    _ =>
                        panic!("Remaining fourteen cannot be its complete twenty-pack birth scope"),
                }
            ),
            HubActualHoldAssessmentDto::Unresolved {
                conditional_timing,
                reason,
                ..
            } => {
                assert_eq!(mode, 4);
                assert_eq!(
                    *conditional_timing,
                    HubActualHoldTimingDto::EstablishedBreach
                );
                assert_eq!(*reason, HubActualHoldUncertaintyDto::AnchorCohortUnverified);
            }
        }
        assert_eq!(core.hub_stock(query()).await.unwrap(), before);
        assert!(
            core.hub_actual_review_operation(review_context.request_id.clone())
                .await
                .unwrap()
                .is_none()
        );
        review_context.expected_revision = core.snapshot().revision;
        let committed = core
            .reconcile_hub_actual(review_context.clone(), input.clone())
            .await
            .unwrap();
        let HubActualReviewOriginalDto::Departure {
            report: original_report,
            original,
            historical_holds,
        } = &committed.original
        else {
            panic!("physical reconciliation");
        };
        assert_eq!(**original_report, saved);
        assert_eq!(original_report.reported.recipient_id, None);
        assert_eq!(original.recipient.id, recipient.current.id);
        assert_eq!(original.on_hand_after_whole_packs, remaining);
        assert_eq!(*historical_holds, preview.historical_holds);
        assert_eq!(original.work_changes[0].retired_whole_packs, 2);
        let mut replay = review_context.clone();
        replay.expected_revision = core.snapshot().revision;
        assert_eq!(
            core.reconcile_hub_actual(replay, input).await.unwrap(),
            committed
        );
        assert_eq!(
            core.hub_actual_report_operation(report_context.request_id)
                .await
                .unwrap(),
            Some(saved)
        );
        assert_eq!(
            core.hub_actual_review_operation(review_context.request_id.clone())
                .await
                .unwrap(),
            Some(committed)
        );
        assert_eq!(
            core.hub_receipt_operation(received_context.request_id)
                .await
                .unwrap(),
            Some(intake.clone())
        );
        let after = core.hub_stock(query()).await.unwrap();
        assert_eq!(
            after
                .rows
                .iter()
                .map(|row| row.position.whole_packs)
                .sum::<u32>(),
            18
        );
        assert_eq!(
            after
                .rows
                .iter()
                .find(|row| row.position.group_id == group_id)
                .unwrap()
                .put_away
                .as_ref()
                .unwrap()
                .no_longer_required,
            2
        );
        assert_eq!(
            after
                .rows
                .iter()
                .find(|row| row.position.group_id == group_id)
                .unwrap()
                .put_away
                .as_ref()
                .unwrap()
                .remaining,
            18
        );
        let held_after = core.hub_treatment(scope).await.unwrap();
        assert_eq!(held_after.holds[0].hold, treatment.holds[0].hold);
        assert_eq!(
            held_after.holds[0].created_operation_id,
            treatment.holds[0].created_operation_id
        );
        assert_eq!(
            held_after.holds[0].released_operation_id,
            treatment.holds[0].released_operation_id
        );
        assert_eq!(held_after.holds[0].current_whole_packs, remaining);
        if mode == 4 {
            assert_eq!(held_after.groups.len(), 2);
            let sibling = held_after
                .groups
                .iter()
                .find(|group| group.group_id != group_id)
                .unwrap();
            assert_eq!(sibling.whole_packs, 6);
            assert!(sibling.active_hold_ids.is_empty());
        }

        assert_eq!(
            held_after
                .groups
                .iter()
                .find(|group| group.group_id == group_id)
                .unwrap()
                .active_hold_ids
                .len(),
            1
        );
        assert_eq!(core.hub_actual_reports(reports()).await.unwrap().total, 0);
        let duplicate = core
            .report_hub_actual(context(&core), report())
            .await
            .unwrap();
        let link_context = context(&core);
        let link = HubActualLinkInputDto {
            report_id: duplicate.id,
            known_operation_id: review_context.request_id,
            known_trace_id: intake.trace.id,
            known_variant_id: ids[1].clone(),
            verification: "Same original two-crate departure explicitly verified".into(),
        };
        let link_preview = core
            .hub_actual_link_preview(link_context.request_id.clone(), link.clone())
            .await
            .unwrap();
        assert_eq!(link_preview.proposed.known.whole_packs, 2);
        assert_eq!(core.hub_actual_reports(reports()).await.unwrap().total, 1);
        let linked = core
            .link_hub_actual(link_context.clone(), link)
            .await
            .unwrap();
        assert!(matches!(
            linked.original,
            HubActualReviewOriginalDto::Duplicate { .. }
        ));
        assert_eq!(
            core.hub_actual_review_operation(link_context.request_id)
                .await
                .unwrap(),
            Some(linked)
        );
        assert_eq!(core.hub_stock(query()).await.unwrap().rows, after.rows);
        assert_eq!(core.hub_actual_reports(reports()).await.unwrap().total, 0);
        core.shutdown_v2().await.unwrap();
    }
}
#[tokio::test]
async fn actual_ffi_retains_future_unverified_claim_on_empty_stock_without_inventing_reference_truth()
 {
    let (core, _, _) = fixture().await;
    let future = cutoff().start_unix_s.unwrap() + 3600;
    let input = HubActualReportInputDto {
        whole_packs: None,
        source_kind: HubActualSourceKindDto::Possible,
        source_claims: vec![HubActualSourceClaimDto {
            trace_id: "fe".repeat(16),
            group_id: None,
            location_id: None,
        }],
        occurrence: known(future),
        ..report()
    };
    let request = context(&core);
    let saved = core
        .report_hub_actual(request.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(saved.reported, input);
    assert_eq!(
        saved.clock_assessment,
        HubActualClockAssessmentDto::RecordingClockConflict
    );
    assert_eq!(
        core.hub_actual_report_operation(request.request_id)
            .await
            .unwrap(),
        Some(saved.clone())
    );
    assert_eq!(
        core.hub_actual_reports(reports()).await.unwrap().rows[0].report,
        saved
    );
    assert!(core.hub_stock(query()).await.unwrap().rows.is_empty());
    core.shutdown_v2().await.unwrap();
}
