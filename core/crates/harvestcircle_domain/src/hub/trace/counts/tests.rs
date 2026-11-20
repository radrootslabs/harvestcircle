use super::*;
use crate::hub::{
    actuals::{ActualKnownCountExplanation, ActualKnownCountInventory, ActualKnownInterpretations},
    corrections::*,
};
use std::sync::Arc;

fn packs(value: u32) -> WholePacks {
    WholePacks::from_value(value).unwrap()
}
fn fixture() -> (TraceSnapshot, CountReturnAssociationPlan) {
    let (catalog, plan) = count_return_trace_policy_fixture();
    let mut snapshot = super::super::tests::fixture();
    let original = &plan.explanation_original;
    let count = &original.count;
    snapshot.hub = original.known_event.hub;
    snapshot.stock_revision = original.explanation.stock_revision;
    snapshot.catalog_revision = catalog.revision();
    snapshot.trace.id = count.before.trace;
    snapshot.trace.variant = original.known_event.variant.clone();
    let crate::hub::reference::ReferenceValue::Pack { product, .. } = snapshot.trace.variant.value
    else {
        panic!("pack")
    };
    snapshot.trace.product = catalog.get(product).unwrap().clone();
    snapshot.trace.initial_location = count.references.location.clone();
    snapshot.trace.recorded_at = count.count.scope.latest_physical_recorded_at;
    snapshot.trace.actor = count.actor;
    snapshot.trace.original_packs = count.before.packs;
    snapshot.exits.clear();
    snapshot.on_hand[0].group = count.before.id;
    snapshot.on_hand[0].revision = count.stock_revision;
    snapshot.on_hand[0].packs = packs(18);
    snapshot.on_hand[0].location = count.references.location.clone();
    snapshot.on_hand[0].treatment.group = count.before.id;
    snapshot.on_hand[0].treatment.packs = packs(18);
    snapshot.count_losses = vec![TraceCountLoss {
        reconciliation: count.id,
        operation: count.operation,
        count: count.count.id,
        population: count.before.trace,
        group: count.before.id,
        stock_revision: count.stock_revision,
        packs: packs(2),
        confirmed_on_hand_at: count.confirmed_on_hand_at,
        recorded_at: count.recorded_at,
        actor: count.actor,
        reason: count.reason.clone(),
    }];
    set_inventory(&mut snapshot, original, plan.before.clone());
    (snapshot, plan)
}
fn set_inventory(
    snapshot: &mut TraceSnapshot,
    original: &CountExplanationPlan,
    account: CountLossQuantityAccount,
) {
    let record = ActualKnownInterpretation {
        stock_revision: snapshot.stock_revision,
        catalog_revision: snapshot.catalog_revision,
        latest_recorded_at: UnixTimestamp::from_seconds(250).unwrap(),
        original: original.known_event.clone(),
        recipient_original: OriginalRecipientAttribution {
            hub: snapshot.hub,
            family: CorrectedDepartureFamily::ExplainedCountLoss,
            operation: original.operation,
            stock_revision: original.explanation.stock_revision,
            catalog_revision: original.known_event.catalog_revision,
            recorded_at: original.recorded_at,
            actor: original.actor,
            recipient: original.known_event.recipient.clone(),
            populations: vec![snapshot.trace.id],
            issued_versions: vec![],
        },
        recipient_history: snapshot.recipient_corrections.clone(),
        quantities: vec![],
        count_explanation: Some(Arc::new(ActualKnownCountExplanation {
            count: original.count.clone(),
            account,
        })),
    };
    snapshot.count_interpretations = Some(
        ActualKnownCountInventory::new(ActualKnownInterpretations {
            hub: snapshot.hub,
            stock_revision: snapshot.stock_revision,
            catalog_revision: snapshot.catalog_revision,
            latest_recorded_at: record.latest_recorded_at,
            records: vec![record],
        })
        .unwrap(),
    );
}

#[test]
fn count_trace_splits_gross_shortage_without_another_physical_debit_and_keeps_zero_original() {
    let (mut snapshot, plan) = fixture();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand,
            balance.counted_loss,
            balance.actual_departed
        ),
        (packs(18), packs(1), packs(1))
    );
    assert_eq!(snapshot.count_losses[0].packs, packs(2));
    assert_eq!(
        snapshot
            .current_recipient(plan.explanation_original.operation)
            .unwrap(),
        AmendedRecipient::Known(plan.recipient_original.recipient.clone())
    );
    let change = CountExplanationAmendment {
        operation: HubRecordId::from_bytes([249; 16]).unwrap(),
        stock_revision: snapshot.stock_revision.next().unwrap(),
        explanation: plan.explanation_original.operation,
        before: packs(1),
        after: packs(0),
    };
    snapshot.stock_revision = change.stock_revision;
    let account = CountLossQuantityAccount::restore_corrected(
        plan.before.original(),
        plan.before.explanations().to_vec(),
        vec![change],
        vec![],
    )
    .unwrap();
    set_inventory(&mut snapshot, &plan.explanation_original, account);
    // Effective zero retains the original explained recipient in retrieval.
    for text in [
        plan.recipient_original.recipient.value.label().as_str(),
        &plan.recipient_original.recipient.id.to_hex(),
    ] {
        let selection = TraceSelection {
            scope: TraceScope::All,
            text: text.into(),
        };
        assert!(snapshot.includes_source_history(&selection).unwrap());
    }
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand,
            balance.counted_loss,
            balance.actual_departed
        ),
        (packs(18), packs(2), packs(0))
    );
    assert_eq!(
        snapshot
            .count_interpretations
            .as_ref()
            .unwrap()
            .facts()
            .records
            .len(),
        1
    );
}

#[test]
fn count_trace_rejects_wrong_anchor_cutoff_owner_gross_quantity_and_missing_loss() {
    let (snapshot, _) = fixture();
    for fault in 0..9 {
        let mut bad = snapshot.clone();
        match fault {
            0 => bad.count_losses[0].group = HubRecordId::from_bytes([248; 16]).unwrap(),
            1 => bad.count_losses[0].confirmed_on_hand_at = UnixTimestamp::from_seconds(1).unwrap(),
            2 => bad.count_losses[0].packs = packs(1),
            3 => bad.count_losses[0].recorded_at = UnixTimestamp::from_seconds(111).unwrap(),
            4 => bad.count_losses.clear(),
            5 => bad.stock_revision = bad.stock_revision.next().unwrap(),
            6 => {
                bad.count_losses[0].actor = PublicKey::from_hex(
                    "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
                )
                .unwrap()
            }
            7 => bad.count_losses[0].count = HubRecordId::from_bytes([248; 16]).unwrap(),
            _ => bad.catalog_revision = bad.catalog_revision.next().unwrap(),
        }
        assert!(bad.balance().is_err(), "fault {fault}");
    }
}

#[test]
fn count_trace_links_reconcile_exact_return_floors_without_stock_credit_and_survive_depletion() {
    let (mut snapshot, plan) = fixture();
    snapshot.stock_revision = plan.association.stock_revision;
    snapshot
        .count_return_associations
        .push(plan.association.clone());
    set_inventory(
        &mut snapshot,
        &plan.explanation_original,
        plan.after.clone(),
    );
    assert_eq!(snapshot.balance().unwrap().on_hand, packs(18));
    let mut omitted = snapshot.clone();
    omitted.count_return_associations.clear();
    assert!(omitted.balance().is_err());
    let mut altered = snapshot.clone();
    altered.count_return_associations[0].packs = packs(2);
    assert!(altered.balance().is_err());

    snapshot.trace = plan.returned_original.population();
    snapshot.on_hand[0].group = plan.returned_original.group.id;
    snapshot.on_hand[0].revision = plan.returned_original.group.revision;
    snapshot.on_hand[0].packs = packs(1);
    snapshot.on_hand[0].location = plan.returned_original.intake.location.clone();
    snapshot.on_hand[0].treatment.group = plan.returned_original.group.id;
    snapshot.on_hand[0].treatment.packs = packs(1);
    snapshot.count_losses.clear();
    snapshot.count_interpretations = Some(
        ActualKnownCountInventory::new(ActualKnownInterpretations {
            hub: snapshot.hub,
            stock_revision: snapshot.stock_revision,
            catalog_revision: snapshot.catalog_revision,
            latest_recorded_at: plan.association.recorded_at,
            records: vec![],
        })
        .unwrap(),
    );
    assert_eq!(snapshot.balance().unwrap().on_hand, packs(1));
    snapshot.on_hand.clear();
    snapshot.dispositions.push(TraceDisposition {
        id: crate::hub::disposition::DispositionIdentities::from_operation_bytes(
            HubRecordId::from_bytes([246; 16]).unwrap(),
        )
        .unwrap()
        .disposition,
        operation: HubRecordId::from_bytes([246; 16]).unwrap(),
        population: snapshot.trace.id,
        group: plan.returned_original.group.id,
        stock_revision: snapshot.stock_revision.next().unwrap(),
        packs: packs(1),
        location: plan.returned_original.intake.location.clone(),
        kind: crate::hub::disposition::DispositionKind::Destroyed,
        actor: plan.association.actor,
        recorded_at: plan.association.recorded_at,
        occurred: plan.association.return_occurred,
        observation: plan.returned_original.intake.observation.clone(),
    });
    snapshot.stock_revision = snapshot.stock_revision.next().unwrap();
    let mut facts = snapshot
        .count_interpretations
        .as_ref()
        .unwrap()
        .facts()
        .clone();
    facts.stock_revision = snapshot.stock_revision;
    snapshot.count_interpretations = Some(ActualKnownCountInventory::new(facts).unwrap());
    assert_eq!(snapshot.balance().unwrap().on_hand, packs(0));
    assert_eq!(snapshot.count_return_associations.len(), 1);
}

#[test]
fn count_trace_combined_roots_reject_association_aliases_of_source_recipient_and_classification() {
    use crate::hub::reference::{
        SourceAttribution, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
    };
    let (mut snapshot, plan) = fixture();
    let (catalog, _) = count_return_trace_policy_fixture();
    let id = |n| HubRecordId::from_bytes([n; 16]).unwrap();
    let at = |n| UnixTimestamp::from_seconds(n).unwrap();
    snapshot.stock_revision = plan.association.stock_revision;
    snapshot
        .count_return_associations
        .push(plan.association.clone());
    let recipient = RecipientAttributionCommand {
        departure_operation: plan.explanation_original.operation,
        expected_stock_revision: snapshot.stock_revision,
        expected_catalog_revision: snapshot.catalog_revision,
        expected_prior: None,
        recipient: None,
        basis: plan.explanation_original.facts.recipient_evidence.clone(),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: snapshot.hub,
                stock_revision: snapshot.stock_revision,
                catalog: &catalog,
                latest_recorded_at: plan.association.recorded_at,
                uncertain_submissions: false,
            },
            original: &plan.recipient_original,
            history: &[],
            linked_issued_versions: &[],
        },
        id(241),
        plan.association.actor,
        at(210),
    )
    .unwrap();
    snapshot.stock_revision = recipient.stamp.stock_revision;
    snapshot.recipient_corrections.push(recipient);
    let source = OriginalSourceAttribution {
        hub: snapshot.hub,
        operation: plan.command.source_operation,
        trace: snapshot.trace.id,
        stock_revision: HubRevision::from_value(2).unwrap(),
        catalog_revision: catalog.revision(),
        recorded_at: snapshot.trace.recorded_at,
        actor: snapshot.trace.actor,
        source: AmendedSource {
            source: snapshot.trace.source.clone(),
            issuer: None,
        },
    };
    let correction = SourceAttributionCommand {
        trace: snapshot.trace.id,
        expected_stock_revision: snapshot.stock_revision,
        expected_catalog_revision: snapshot.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(242),
                issuer: None,
                kind: SourceReferenceKind::SupplierReference,
                code: SourceCode::new("Fictional source correction").unwrap(),
                provenance: plan.command.source_evidence.clone(),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
        basis: plan.command.source_evidence.clone(),
    }
    .plan(
        &SourceAttributionContext {
            cut: AttributionCut {
                hub: snapshot.hub,
                stock_revision: snapshot.stock_revision,
                catalog: &catalog,
                latest_recorded_at: at(210),
                uncertain_submissions: false,
            },
            original: &source,
            history: &[],
            known_sources: &[],
            issued_versions: &[],
            affected: &[AttributionAffectedDeparture {
                operation: plan.explanation_original.operation,
                populations: vec![snapshot.trace.id],
                recipients: vec![
                    AmendedRecipient::Known(plan.recipient_original.recipient.clone()),
                    AmendedRecipient::Unknown,
                ],
                issued_versions: vec![],
            }],
        },
        id(243),
        plan.association.actor,
        at(220),
    )
    .unwrap();
    snapshot.stock_revision = correction.stamp.stock_revision;
    snapshot.source_corrections.push(correction);
    let classification = CountExplanationAmendment {
        operation: id(244),
        stock_revision: snapshot.stock_revision.next().unwrap(),
        explanation: plan.explanation_original.operation,
        before: packs(1),
        after: packs(2),
    };
    snapshot.stock_revision = classification.stock_revision;
    let account = CountLossQuantityAccount::restore_corrected(
        plan.before.original(),
        plan.before.explanations().to_vec(),
        vec![classification],
        vec![CountExplanationReturns {
            explanation: plan.explanation_original.operation,
            packs: packs(1),
        }],
    )
    .unwrap();
    set_inventory(&mut snapshot, &plan.explanation_original, account);
    assert_eq!(snapshot.balance().unwrap().counted_loss, packs(0));
    assert_eq!(
        snapshot
            .current_recipient(plan.explanation_original.operation)
            .unwrap(),
        AmendedRecipient::Unknown
    );
    for (operation, revision) in [
        (
            id(241),
            snapshot.recipient_corrections[0].stamp.stock_revision,
        ),
        (id(243), snapshot.source_corrections[0].stamp.stock_revision),
        (id(244), classification.stock_revision),
        (
            plan.explanation_original.operation,
            plan.explanation_original.explanation.stock_revision,
        ),
        (id(245), snapshot.source_corrections[0].stamp.stock_revision),
    ] {
        let mut bad = snapshot.clone();
        let association = &mut bad.count_return_associations[0];
        association.operation = operation;
        association.id = count_return_association_id(operation).unwrap();
        association.stock_revision = revision;
        assert!(bad.balance().is_err(), "root {operation:?}");
    }
}

#[test]
fn count_trace_root_audit_allows_repeated_lines_of_the_same_normal_event() {
    let mut snapshot = super::super::tests::fixture();
    let mut second = snapshot.exits[0].clone();
    second.group = HubRecordId::from_bytes([246; 16]).unwrap();
    second.packs = packs(2);
    snapshot.exits[0].packs = packs(3);
    snapshot.exits.push(second);
    snapshot.count_interpretations = Some(
        ActualKnownCountInventory::new(ActualKnownInterpretations {
            hub: snapshot.hub,
            stock_revision: snapshot.stock_revision,
            catalog_revision: snapshot.catalog_revision,
            latest_recorded_at: UnixTimestamp::from_seconds(250).unwrap(),
            records: vec![],
        })
        .unwrap(),
    );
    assert_eq!(snapshot.balance().unwrap().departed, packs(5));
}

#[test]
fn incoming_count_link_cannot_assign_a_local_count_original_to_another_population() {
    let (mut snapshot, plan) = fixture();
    snapshot.trace = plan.returned_original.population();
    snapshot.stock_revision = plan.association.stock_revision;
    snapshot.on_hand.clear();
    let local_count = HubRecordId::from_bytes([245; 16]).unwrap();
    let local_revision = HubRevision::from_value(6).unwrap();
    snapshot.count_losses = vec![TraceCountLoss {
        reconciliation: crate::hub::counts::count_reconciliation_id(local_count).unwrap(),
        operation: local_count,
        count: HubRecordId::from_bytes([244; 16]).unwrap(),
        population: snapshot.trace.id,
        group: plan.returned_original.group.id,
        stock_revision: local_revision,
        packs: packs(1),
        confirmed_on_hand_at: UnixTimestamp::from_seconds(140).unwrap(),
        recorded_at: UnixTimestamp::from_seconds(145).unwrap(),
        actor: plan.association.actor,
        reason: plan.explanation_original.count.reason.clone(),
    }];
    snapshot.count_return_associations.push(plan.association);
    snapshot.count_interpretations = Some(
        ActualKnownCountInventory::new(ActualKnownInterpretations {
            hub: snapshot.hub,
            stock_revision: snapshot.stock_revision,
            catalog_revision: snapshot.catalog_revision,
            latest_recorded_at: UnixTimestamp::from_seconds(200).unwrap(),
            records: vec![],
        })
        .unwrap(),
    );
    assert_eq!(snapshot.balance().unwrap().counted_loss, packs(1));
    let association = &mut snapshot.count_return_associations[0];
    association.count_operation = local_count;
    association.count_stock_revision = local_revision;
    association.validate().unwrap(); // Its own consistency cannot establish the count owner.
    assert_eq!(snapshot.balance(), Err(TraceError::InvalidSnapshot));
}

#[test]
fn repeated_trace_links_keep_one_explanation_source_identity_and_exact_cumulative_floor() {
    let (mut snapshot, _) = fixture();
    let (_, plans) = count_return_trace_two_returns_fixture();
    let second = &plans[1];
    snapshot.stock_revision = second.association.stock_revision;
    snapshot.count_return_associations =
        plans.iter().map(|plan| plan.association.clone()).collect();
    set_inventory(
        &mut snapshot,
        &second.explanation_original,
        second.after.clone(),
    );
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand,
            balance.actual_departed,
            balance.counted_loss
        ),
        (packs(18), packs(2), packs(0))
    );
    let mut forged = snapshot.clone();
    forged.count_return_associations[1].explanation_source_digest[0] ^= 1;
    forged.count_return_associations[1].validate().unwrap();
    assert_eq!(forged.balance(), Err(TraceError::InvalidSnapshot));
}
