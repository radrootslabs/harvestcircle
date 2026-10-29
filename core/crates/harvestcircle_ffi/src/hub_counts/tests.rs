use super::*;
use crate::{
    HarvestCircleAppCore, HubAcceptanceStatusDto, HubLocationPurposeDto, HubOccurrenceKindDto,
    HubOpeningInputDto, HubPhysicalIdentificationDto, HubPopulationOriginDto, HubReceiptInputDto,
    HubReferenceEditDto, HubReferenceInputDto, HubReferenceKindDto, HubSourceCertaintyDto,
    HubSourceInputDto, HubStockQueryDto, HubTraceSnapshotQueryDto, RequestContextDto,
};
use harvestcircle_application::DurableRequestId;
use std::sync::Arc;

fn context(core: &HarvestCircleAppCore) -> RequestContextDto {
    RequestContextDto {
        request_id: DurableRequestId::new_v7().as_str().to_owned(),
        expected_revision: core.snapshot().revision,
        deadline_millis: 5000,
    }
}
fn unknown_source() -> HubSourceInputDto {
    HubSourceInputDto {
        existing_source_id: None,
        issuer_id: None,
        kind: None,
        code: None,
        provenance: None,
        certainty: HubSourceCertaintyDto::Unknown,
    }
}
fn cutoff() -> HubOccurrenceDto {
    HubOccurrenceDto {
        kind: HubOccurrenceKindDto::Known,
        start_unix_s: Some(
            i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .unwrap(),
        ),
        end_unix_s: None,
    }
}
fn query() -> HubStockQueryDto {
    HubStockQueryDto {
        expected_catalog_revision: None,
        expected_stock_revision: None,
        offset: 0,
        limit: 64,
        location_id: None,
        trace_id: None,
        text: String::new(),
    }
}
async fn fixture() -> (Arc<HarvestCircleAppCore>, Vec<String>, u64) {
    let core = crate::commands::test_core(true).await;
    core.bootstrap().await.unwrap();
    let imported = core
        .import_identity(
            context(&core),
            b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_vec(),
        )
        .await
        .unwrap();
    core.activate_identity(imported.snapshot.identities[0].public_key_hex.clone())
        .await
        .unwrap();
    core.initialize_hub(context(&core)).await.unwrap();
    let mut ids: Vec<String> = Vec::new();
    let mut revision = 1;
    for (kind, label, purpose) in [
        (HubReferenceKindDto::Product, "Fictional apples", None),
        (HubReferenceKindDto::Pack, "Whole crate", None),
        (
            HubReferenceKindDto::Location,
            "Receiving",
            Some(HubLocationPurposeDto::Receiving),
        ),
        (
            HubReferenceKindDto::Location,
            "A3",
            Some(HubLocationPurposeDto::Storage),
        ),
    ] {
        let product_id = if kind == HubReferenceKindDto::Pack {
            Some(ids[0].clone())
        } else {
            None
        };
        let saved = core
            .apply_hub_reference(
                context(&core),
                HubReferenceEditDto {
                    expected_catalog_revision: revision,
                    record_id: None,
                    expected_record_revision: None,
                    value: HubReferenceInputDto {
                        kind,
                        label: label.to_owned(),
                        product_id,
                        nominal_grams: None,
                        location_purpose: purpose,
                    },
                },
            )
            .await
            .unwrap();
        revision = saved.catalog_revision;
        ids.push(saved.current.id);
    }
    (core, ids, revision)
}
fn observation(
    stock: u64,
    catalog: u64,
    group: String,
    group_revision: u64,
    counted: u32,
) -> HubCountObserveInputDto {
    HubCountObserveInputDto {
        expected_stock_revision: stock,
        expected_catalog_revision: catalog,
        group_id: group,
        expected_group_revision: group_revision,
        counted_whole_packs: counted,
        cutoff: cutoff(),
        observation: "Fictional on-hand count".into(),
    }
}
fn posting(review: &HubCountReviewDto) -> HubCountPostInputDto {
    HubCountPostInputDto {
        expected_stock_revision: review.stock_revision,
        expected_catalog_revision: review.catalog_revision,
        count_id: review.observation.id.clone(),
        reviewed_activity: true,
        surplus_identification: None,
        reason: "Reviewed present count and native activity".into(),
    }
}
#[tokio::test]
async fn hub_count_ffi_preserves_twenty_until_post_then_exports_distinct_shortage_and_original_work()
 {
    let (core, ids, catalog) = fixture().await;
    let receipt_context = context(&core);
    let received = core
        .receive_hub_stock(
            receipt_context.clone(),
            HubReceiptInputDto {
                expected_stock_revision: 1,
                expected_catalog_revision: catalog,
                variant_id: ids[1].clone(),
                supplier_id: None,
                producer_id: None,
                source: unknown_source(),
                whole_packs: 20,
                receiving_id: ids[2].clone(),
                planned_location_id: Some(ids[3].clone()),
                occurrence: HubOccurrenceDto {
                    kind: HubOccurrenceKindDto::Unknown,
                    start_unix_s: None,
                    end_unix_s: None,
                },
                observation: "Fictional receipt of twenty whole packs".into(),
            },
        )
        .await
        .unwrap();
    let saved_context = context(&core);
    let observed = core
        .observe_hub_count(
            saved_context.clone(),
            observation(
                received.stock_revision,
                catalog,
                received.initial_position.group_id.clone(),
                received.initial_position.revision,
                18,
            ),
        )
        .await
        .unwrap();
    assert_eq!(observed.recorded_whole_packs, 20);
    assert_eq!(observed.counted_whole_packs, 18);
    assert_eq!(observed.operation_id, saved_context.request_id);
    let before = core.hub_stock(query()).await.unwrap();
    assert_eq!(before.rows[0].position.whole_packs, 20);
    assert_eq!(before.rows[0].put_away.as_ref().unwrap().remaining, 20);
    let review = core.hub_count_review(observed.id.clone()).await.unwrap();
    assert!(review.ready && review.blockers.is_empty());
    assert_eq!(
        review.difference,
        HubCountDifferenceDto::Shortage { whole_packs: 2 }
    );
    let input = posting(&review);
    let post_context = context(&core);
    let posted = core
        .post_hub_count(post_context.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(posted.operation_id, post_context.request_id);
    assert_eq!(posted.observation, observed);
    assert_eq!(posted.before.whole_packs, 20);
    assert_eq!(posted.on_hand_after, 18);
    assert_eq!(posted.work_changes.len(), 1);
    assert_eq!(posted.work_changes[0].retired_whole_packs, 2);
    assert_eq!(posted.work_changes[0].after.quantity.completed, 0);
    assert_eq!(posted.work_changes[0].after.quantity.no_longer_required, 2);
    assert_eq!(posted.work_changes[0].after.quantity.remaining, 18);
    assert_eq!(posted.removed_work[0].removed_whole_packs, 2);
    assert_eq!(
        posted.removed_work[0].removed_progress,
        HubWorkProgressDto::NoLongerRequired
    );
    assert!(posted.surplus.is_none());
    assert_eq!(
        core.post_hub_count(post_context.clone(), input.clone())
            .await
            .unwrap(),
        posted
    );
    assert_eq!(
        core.hub_count_post_operation(post_context.request_id.clone())
            .await
            .unwrap(),
        Some(posted.clone())
    );
    assert_eq!(
        core.hub_count_observation_operation(saved_context.request_id)
            .await
            .unwrap(),
        Some(observed)
    );
    assert_eq!(
        core.hub_receipt_operation(receipt_context.request_id)
            .await
            .unwrap(),
        Some(received.clone())
    );
    let stock = core.hub_stock(query()).await.unwrap();
    assert_eq!(stock.rows[0].position.whole_packs, 18);
    let trace = core
        .hub_trace_snapshot(HubTraceSnapshotQueryDto {
            trace_id: received.trace.id,
            expected_stock_revision: None,
            expected_catalog_revision: None,
        })
        .await
        .unwrap();
    assert_eq!(
        (
            trace.on_hand_whole_packs,
            trace.departed_whole_packs,
            trace.counted_loss_whole_packs
        ),
        (18, 0, 2)
    );
    assert!(trace.exits.is_empty());
    assert_eq!(trace.count_losses[0].reconciliation_id, posted.id);
    let mut changed = input;
    changed.reason = "Different original review reason".into();
    assert!(
        core.post_hub_count(post_context.clone(), changed)
            .await
            .is_err()
    );
    assert_eq!(core.hub_stock(query()).await.unwrap(), stock);
    core.sign_out().await.unwrap();
    assert!(core.hub_count_review(posted.observation.id).await.is_err());
    assert!(
        core.hub_count_post_operation(post_context.request_id)
            .await
            .is_err()
    );
    core.shutdown_v2().await.unwrap();
}
#[tokio::test]
async fn hub_count_ffi_opening_and_surplus_keep_unknown_history_source_and_new_acceptance_explicit()
{
    let (core, ids, catalog) = fixture().await;
    let opening_context = context(&core);
    let input = HubOpeningInputDto {
        expected_stock_revision: 1,
        expected_catalog_revision: catalog,
        variant_id: ids[1].clone(),
        location_id: ids[3].clone(),
        whole_packs: 8,
        identification: HubPhysicalIdentificationDto {
            label: "Separately counted opening eight".into(),
            observation: "Fictional count without receipt history".into(),
        },
        source: unknown_source(),
        history_reference: None,
        counted: cutoff(),
        observation: "Earlier intake and handling are unknown".into(),
    };
    let opened = core
        .record_hub_opening(opening_context.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(
        core.record_hub_opening(opening_context.clone(), input)
            .await
            .unwrap(),
        opened
    );
    assert_eq!(
        core.hub_opening_operation(opening_context.request_id)
            .await
            .unwrap(),
        Some(opened.clone())
    );
    assert_eq!(
        opened.original.population.origin,
        HubPopulationOriginDto::CountedOpening {
            history_reference: None
        }
    );
    assert_eq!(
        opened.original.population.source.certainty,
        HubSourceCertaintyDto::Unknown
    );
    assert_eq!(
        opened.original.acceptance,
        HubAcceptanceStatusDto::ReviewRequired
    );
    assert!(opened.original.group.work.is_empty());
    assert!(
        core.hub_stock(query()).await.unwrap().rows[0]
            .put_away
            .is_none()
    );
    let observed = core
        .observe_hub_count(
            context(&core),
            observation(
                opened.stock_revision,
                catalog,
                opened.original.group.id.clone(),
                opened.original.group.revision,
                10,
            ),
        )
        .await
        .unwrap();
    let review = core.hub_count_review(observed.id.clone()).await.unwrap();
    assert_eq!(
        review.difference,
        HubCountDifferenceDto::Surplus { whole_packs: 2 }
    );
    let mut input = posting(&review);
    input.surplus_identification = Some(HubPhysicalIdentificationDto {
        label: "Two distinct counted surplus packs".into(),
        observation: "Origin of these extra packs is unknown".into(),
    });
    let post_context = context(&core);
    let posted = core
        .post_hub_count(post_context.clone(), input)
        .await
        .unwrap();
    assert_eq!(posted.on_hand_after, 8);
    assert!(posted.work_changes.is_empty() && posted.removed_work.is_empty());
    let surplus = posted.surplus.as_ref().unwrap();
    assert_eq!(surplus.population.original_whole_packs, 2);
    assert_eq!(
        surplus.population.source.certainty,
        HubSourceCertaintyDto::Unknown
    );
    assert_eq!(surplus.acceptance, HubAcceptanceStatusDto::ReviewRequired);
    assert!(surplus.group.work.is_empty());
    assert_eq!(
        surplus.population.origin,
        HubPopulationOriginDto::CountedSurplus {
            count_id: observed.id,
            related_population_id: opened.original.population.id.clone()
        }
    );
    assert_ne!(surplus.population.id, opened.original.population.id);
    assert_eq!(core.hub_stock(query()).await.unwrap().total, 2);
    assert_eq!(
        core.hub_count_post_operation(post_context.request_id)
            .await
            .unwrap(),
        Some(posted)
    );
    core.shutdown_v2().await.unwrap();
}
#[test]
fn hub_count_transport_rejects_invalid_ids_revisions_and_cutoffs_without_losing_zero_observations()
{
    let original = observation(2, 5, "01".repeat(16), 1, 0);
    assert!(original.clone().command().is_ok());
    let mut bad = original.clone();
    bad.group_id = "00".repeat(16);
    assert!(bad.command().is_err());
    let mut bad = original.clone();
    bad.expected_group_revision = 0;
    assert!(bad.command().is_err());
    let mut bad = original.clone();
    bad.cutoff.end_unix_s = Some(1);
    assert!(bad.command().is_err());
    let mut bad = original;
    bad.cutoff.kind = HubOccurrenceKindDto::Unknown;
    assert!(bad.command().is_err());
}
