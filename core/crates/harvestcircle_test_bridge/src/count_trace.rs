//! Encoding-only projections built with checked domain count/account policies.
//! No database, native admission, persisted E/C or historical upgrade is claimed.
use harvestcircle_domain::{
    PublicKey, UnixTimestamp,
    hub::{
        HubId, HubRevision,
        actuals::{
            ActualKnownCountExplanation, ActualKnownCountInventory, ActualKnownDeparture,
            ActualKnownDepartureLine, ActualKnownInterpretation, ActualKnownInterpretations,
        },
        corrections::*,
        counts::*,
        movement::PhysicalIdentification,
        movement::PhysicalStockGroup,
        reference::*,
        returns::{ReturnCommand, ReturnContext, ReturnDepartureClaim, ReturnIdentities},
        stock::{Occurrence, PopulationOrigin, StockPopulation},
        trace::{TraceCountLoss, TracePosition, TraceSnapshot},
        treatment::{AcceptanceStatus, GroupTreatment},
    },
};
use harvestcircle_ffi::HubTraceSnapshotDto;
use std::sync::Arc;

fn id(value: u8) -> HubRecordId {
    HubRecordId::from_bytes([value; 16]).unwrap()
}
fn op(value: u64) -> HubRecordId {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&[1, 144, 0, 0, 0, 0, 112, 0]);
    bytes[8] = 128;
    bytes[9..].copy_from_slice(&value.to_be_bytes()[1..]);
    HubRecordId::from_bytes(bytes).unwrap()
}
fn rev(value: u64) -> HubRevision {
    HubRevision::from_value(value).unwrap()
}
fn at(value: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(value).unwrap()
}
fn packs(value: u32) -> WholePacks {
    WholePacks::from_value(value).unwrap()
}
fn note(value: &str) -> SourceProvenance {
    SourceProvenance::new(value).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}

#[uniffi::export]
pub fn count_trace_encoding_probe() -> HubTraceSnapshotDto {
    HubTraceSnapshotDto::try_from(snapshot(2, true)).unwrap()
}

fn snapshot(rows: usize, unknown_recipient: bool) -> TraceSnapshot {
    world(rows, unknown_recipient).1
}

fn world(rows: usize, unknown_recipient: bool) -> (ReferenceCatalog, TraceSnapshot) {
    assert!((1..=HUB_CORRECTION_ROWS / 2).contains(&rows));
    assert!(!unknown_recipient || rows < HUB_CORRECTION_ROWS / 2);
    let hub = HubId::from_bytes([1; 16]).unwrap();
    let mut catalog = ReferenceCatalog::default();
    for (identity, value) in [
        (
            1,
            ReferenceValue::Product {
                label: HubLabel::new("Fictional encoding apples").unwrap(),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Original whole crate").unwrap(),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Location {
                label: HubLabel::new("Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            4,
            ReferenceValue::Party {
                label: HubLabel::new("Original fictional recipient").unwrap(),
            },
        ),
    ] {
        catalog
            .create(catalog.revision(), id(identity), value)
            .unwrap();
    }
    let group = PhysicalStockGroup {
        id: id(10),
        trace: id(11),
        variant: id(2),
        parent: None,
        identification: None,
        location: id(3),
        packs: packs(20),
        revision: rev(1),
        causal_floor: Some(at(40)),
        work: vec![],
    };
    let mut context = CountContext {
        hub,
        stock_revision: rev(2),
        catalog: &catalog,
        group: &group,
        latest_physical_recorded_at: at(50),
        previous_reconciliation: None,
        latest_observation: None,
        unresolved_actual: false,
        uncertain_intent: false,
    };
    let observed = CountObserveCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: catalog.revision(),
        group: group.id,
        expected_group_revision: group.revision,
        counted_packs: packs(18),
        cutoff: Occurrence::Known(at(80)),
        observation: note("Encoding-only independently counted eighteen"),
    }
    .plan(&context.observation_context(), op(20), actor(), at(100))
    .unwrap();
    context.stock_revision = observed.recorded_revision;
    context.latest_observation = Some(observed.id);
    let count = CountPostCommand {
        expected_stock_revision: context.stock_revision,
        expected_catalog_revision: catalog.revision(),
        count: observed.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note("Original shortage <script> is retained as escaped text"),
    }
    .plan(&observed, &context, op(21), actor(), at(110))
    .unwrap();
    let explanations = (0..rows)
        .map(|index| CountLossExplanation {
            operation: op(100 + 2 * index as u64),
            stock_revision: rev(5 + 2 * index as u64),
            packs: packs(1),
        })
        .collect::<Vec<_>>();
    let amendments = explanations
        .iter()
        .enumerate()
        .map(|(index, original)| CountExplanationAmendment {
            operation: op(101 + 2 * index as u64),
            stock_revision: rev(original.stock_revision.value() + 1),
            explanation: original.operation,
            before: packs(1),
            after: packs(if index + 1 == rows { 2 } else { 0 }),
        })
        .collect::<Vec<_>>();
    let mut stock = amendments.last().unwrap().stock_revision;
    let account = CountLossQuantityAccount::restore_corrected(
        OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: group.trace,
            variant: group.variant,
            packs: packs(2),
        },
        explanations.clone(),
        amendments,
        vec![],
    )
    .unwrap();
    let shared = Arc::new(ActualKnownCountExplanation {
        count: count.clone(),
        account,
    });
    let mut records = explanations
        .iter()
        .enumerate()
        .map(|(index, explanation)| {
            let original = ActualKnownDeparture {
                operation: explanation.operation,
                actor: actor(),
                hub,
                stock_revision: explanation.stock_revision,
                catalog_revision: catalog.revision(),
                trace: group.trace,
                variant: catalog.get(group.variant).unwrap().clone(),
                recipient: catalog.get(id(4)).unwrap().clone(),
                occurred: Occurrence::Known(at(70)),
                recorded_at: at(150 + index as i64),
                lines: vec![ActualKnownDepartureLine {
                    group: group.id,
                    location: catalog.get(group.location).unwrap().clone(),
                    packs: packs(1),
                }],
            };
            ActualKnownInterpretation {
                stock_revision: stock,
                catalog_revision: catalog.revision(),
                latest_recorded_at: at(3000),
                recipient_original: OriginalRecipientAttribution {
                    hub,
                    family: CorrectedDepartureFamily::ExplainedCountLoss,
                    operation: original.operation,
                    stock_revision: original.stock_revision,
                    catalog_revision: original.catalog_revision,
                    recorded_at: original.recorded_at,
                    actor: original.actor,
                    recipient: original.recipient.clone(),
                    populations: vec![group.trace],
                    issued_versions: vec![],
                },
                original,
                recipient_history: vec![],
                quantities: vec![],
                count_explanation: Some(Arc::clone(&shared)),
            }
        })
        .collect::<Vec<_>>();
    let mut recipient_corrections = Vec::new();
    if unknown_recipient {
        let selected = records.last_mut().unwrap();
        let correction = RecipientAttributionCommand {
            departure_operation: selected.original.operation,
            expected_stock_revision: stock,
            expected_catalog_revision: catalog.revision(),
            expected_prior: None,
            recipient: None,
            basis: note("Recipient is explicitly unknown after review"),
        }
        .plan(
            &RecipientAttributionContext {
                cut: AttributionCut {
                    hub,
                    stock_revision: stock,
                    catalog: &catalog,
                    latest_recorded_at: at(3000),
                    uncertain_submissions: false,
                },
                original: &selected.recipient_original,
                history: &[],
                linked_issued_versions: &[],
            },
            op(8000),
            actor(),
            at(3100),
        )
        .unwrap();
        stock = correction.stamp.stock_revision;
        selected.recipient_history.push(correction.clone());
        recipient_corrections.push(correction);
    }
    for record in &mut records {
        record.stock_revision = stock;
        record.latest_recorded_at = at(3100);
    }
    let inventory = ActualKnownCountInventory::new(ActualKnownInterpretations {
        hub,
        stock_revision: stock,
        catalog_revision: catalog.revision(),
        latest_recorded_at: at(3100),
        records,
    })
    .unwrap();
    let snapshot = TraceSnapshot {
        count_interpretations: Some(inventory),
        count_return_associations: vec![],
        count_return_uncertain: false,
        source_corrections: vec![],
        recipient_corrections,
        quantity_corrections: vec![],
        hub,
        stock_revision: stock,
        catalog_revision: catalog.revision(),
        trace: StockPopulation {
            id: group.trace,
            variant: catalog.get(group.variant).unwrap().clone(),
            product: catalog.get(id(1)).unwrap().clone(),
            supplier: None,
            producer: None,
            source: SourceAttribution::Unknown,
            source_issuer: None,
            original_packs: packs(20),
            origin: PopulationOrigin::Receipt,
            initial_location: catalog.get(group.location).unwrap().clone(),
            planned_location: None,
            occurred: Occurrence::Known(at(40)),
            recorded_at: at(50),
            actor: actor(),
            observation: note("Encoding-only original twenty"),
        },
        on_hand: vec![TracePosition {
            group: group.id,
            revision: count.stock_revision,
            packs: packs(18),
            location: catalog.get(group.location).unwrap().clone(),
            identification: None,
            treatment: GroupTreatment {
                group: group.id,
                packs: packs(18),
                acceptance: AcceptanceStatus::ReviewRequired,
                acceptance_decision: None,
                active_holds: vec![],
            },
        }],
        exits: vec![],
        dispositions: vec![],
        actual_departures: vec![],
        count_losses: vec![TraceCountLoss {
            reconciliation: count.id,
            operation: count.operation,
            count: count.count.id,
            population: group.trace,
            group: group.id,
            stock_revision: count.stock_revision,
            packs: packs(2),
            confirmed_on_hand_at: count.confirmed_on_hand_at,
            recorded_at: count.recorded_at,
            actor: count.actor,
            reason: count.reason,
        }],
    };
    snapshot.balance().unwrap();
    (catalog, snapshot)
}

#[uniffi::export]
pub fn count_return_trace_encoding_probe() -> HubTraceSnapshotDto {
    HubTraceSnapshotDto::try_from(returned_snapshot()).unwrap()
}

fn returned_snapshot() -> TraceSnapshot {
    let (catalog, original) = world(2, true);
    let inventory = original.count_interpretations.as_ref().unwrap();
    let explanation = inventory.facts().records.last().unwrap();
    let count = &explanation.count_explanation.as_ref().unwrap().count;
    let returned = ReturnCommand {
        expected_stock_revision: original.stock_revision,
        expected_catalog_revision: original.catalog_revision,
        variant: original.trace.variant.id,
        location: original.trace.initial_location.id,
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: packs(1),
        identification: PhysicalIdentification {
            label: HubLabel::new("Encoding-only separate returned crate").unwrap(),
            observation: note("No persisted Return is claimed"),
        },
        planned_location: None,
        departure: Some(ReturnDepartureClaim {
            departure: count_explanation_id(explanation.original.operation).unwrap(),
            group: Some(count.before.id),
        }),
        occurred: Occurrence::Unknown,
        observation: note("Unknown Return occurrence remains unknown"),
    }
    .plan(
        &ReturnContext {
            hub: original.hub,
            stock_revision: original.stock_revision,
            catalog: &catalog,
            departure: None,
            established_return_totals: &[],
            current_allowances: None,
        },
        ReturnIdentities::from_operation_bytes(op(9000)).unwrap(),
        actor(),
        at(3200),
    )
    .unwrap();
    let stock = rev(returned.stock_revision.value() + 1);
    // Fixed, synthetic projection for encoding only. Full original/capture and
    // source-witness authentication remain separate native qualification.
    let association = CountReturnAssociation {
        id: count_return_association_id(op(9001)).unwrap(),
        operation: op(9001),
        hub: original.hub,
        stock_revision: stock,
        catalog_revision: original.catalog_revision,
        actor: actor(),
        recorded_at: at(3300),
        return_operation: returned.identities.operation,
        return_stock_revision: returned.stock_revision,
        returned: returned.identities.returned,
        returned_population: returned.identities.population,
        returned_group: returned.identities.group,
        explanation_operation: explanation.original.operation,
        explanation_stock_revision: explanation.original.stock_revision,
        explanation_trace: original.trace.id,
        count_operation: count.operation,
        count_stock_revision: count.stock_revision,
        anchor_group: count.before.id,
        variant: original.trace.variant.id,
        packs: packs(1),
        return_occurred: returned.intake.occurred,
        explanation_occurred: explanation.original.occurred,
        timing: returned.linkage.timing,
        return_original_digest: [7; 32],
        explanation_source_digest: [8; 32],
    };
    association.validate().unwrap();
    let snapshot = TraceSnapshot {
        count_interpretations: Some(
            ActualKnownCountInventory::new(ActualKnownInterpretations {
                hub: original.hub,
                stock_revision: stock,
                catalog_revision: original.catalog_revision,
                latest_recorded_at: at(3300),
                records: vec![],
            })
            .unwrap(),
        ),
        count_return_associations: vec![association],
        count_return_uncertain: false,
        source_corrections: vec![],
        recipient_corrections: vec![],
        quantity_corrections: vec![],
        hub: original.hub,
        stock_revision: stock,
        catalog_revision: original.catalog_revision,
        trace: returned.population(),
        on_hand: vec![TracePosition {
            group: returned.group.id,
            revision: returned.group.revision,
            packs: packs(1),
            location: returned.intake.location.clone(),
            identification: returned.group.identification.clone(),
            treatment: GroupTreatment {
                group: returned.group.id,
                packs: packs(1),
                acceptance: returned.acceptance,
                acceptance_decision: None,
                active_holds: vec![],
            },
        }],
        exits: vec![],
        count_losses: vec![],
        dispositions: vec![],
        actual_departures: vec![],
    };
    snapshot.balance().unwrap();
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_return_encoding_keeps_the_foreign_explanation_distinct_with_empty_local_inventory()
    {
        let snapshot = returned_snapshot();
        let dto = HubTraceSnapshotDto::try_from(snapshot.clone()).unwrap();
        let counts = dto.count_interpretations.unwrap();
        assert!(counts.accounts.is_empty() && counts.explanations.is_empty());
        assert_eq!(dto.count_return_associations.len(), 1);
        let link = &dto.count_return_associations[0];
        assert_eq!(link.returned_population_id, snapshot.trace.id.to_hex());
        assert_ne!(link.returned_population_id, link.explanation_trace_id);
        assert_ne!(link.return_operation_id, link.explanation_operation_id);
        assert_eq!(dto.on_hand_whole_packs, 1);
        let output = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
        assert!(
            output
                .html
                .contains("Later whole-intake Return associations")
        );
        assert!(output.html.contains("Original Return occurrence"));
    }
    #[test]
    fn count_read_encoding_preserves_shared_zero_current_unknown_and_escaped_original_export() {
        let snapshot = snapshot(2, true);
        let dto = HubTraceSnapshotDto::try_from(snapshot.clone()).unwrap();
        let counts = dto.count_interpretations.unwrap();
        assert_eq!(counts.accounts.len(), 1);
        assert_eq!(counts.explanations.len(), 2);
        assert_eq!(
            counts.explanations[0].interpretation.effective_whole_packs,
            0
        );
        assert_eq!(
            counts.explanations[1].interpretation.original_whole_packs,
            1
        );
        assert_eq!(
            counts.explanations[1].interpretation.effective_whole_packs,
            2
        );
        assert!(counts.explanations[1].interpretation.recipient.is_none());
        assert_eq!(counts.accounts[0].classifications.len(), 2);
        let export = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
        assert!(
            export
                .html
                .contains("Current recipient:</strong> Unknown recipient")
        );
        assert!(export.html.contains("&lt;script&gt;"));
        assert!(!export.html.contains("<script>"));
    }
    #[test]
    fn maximum_shared_count_read_has_one_account_and_bounded_complete_or_capacity_export() {
        let started = std::time::Instant::now();
        let snapshot = snapshot(HUB_CORRECTION_ROWS / 2, false);
        let dto = HubTraceSnapshotDto::try_from(snapshot.clone()).unwrap();
        let counts = dto.count_interpretations.unwrap();
        assert_eq!(counts.accounts.len(), 1);
        assert_eq!(counts.explanations.len(), HUB_CORRECTION_ROWS / 2);
        assert_eq!(
            counts.accounts[0].classifications.len(),
            HUB_CORRECTION_ROWS / 2
        );
        match harvestcircle_application::hub_output::trace_output(&snapshot) {
            Ok(output) => {
                println!("maximum count export complete bytes={}", output.html.len());
                assert!(
                    output.html.len()
                        <= harvestcircle_application::hub_output::LOCAL_OUTPUT_MAX_BYTES
                );
                assert_eq!(
                    output
                        .html
                        .matches("Retained explanation and current meaning")
                        .count(),
                    HUB_CORRECTION_ROWS / 2
                );
            }
            Err(error) => {
                assert_eq!(
                    error,
                    harvestcircle_application::hub_output::LocalOutputError::Capacity
                );
                println!(
                    "maximum count export=Capacity at existing {} byte limit",
                    harvestcircle_application::hub_output::LOCAL_OUTPUT_MAX_BYTES
                );
            }
        }
        println!("maximum count DTO/export elapsed={:?}", started.elapsed());
    }
}
