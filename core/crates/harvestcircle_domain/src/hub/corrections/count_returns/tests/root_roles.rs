use super::*;

// Deliberately synthesized projection-prefix probes. These isolate typed root
// ownership and do not claim full native originals for the unrelated row.
fn fixture() -> Fixture {
    let mut f = Fixture::with_quantity(2);
    let mut prior = f.plan().unwrap().association;
    f.original.known_event.stock_revision = HubRevision::from_value(21).unwrap();
    f.original.explanation.stock_revision = f.original.known_event.stock_revision;
    f.original.facts.expected_stock_revision = HubRevision::from_value(20).unwrap();
    f.account =
        CountLossQuantityAccount::restore(f.account.original(), vec![f.original.explanation])
            .unwrap();
    f.recipient.stock_revision = f.original.known_event.stock_revision;
    f.cut = f.original.known_event.stock_revision;
    f.clock = at(200);
    prior.operation = id(210);
    prior.id = count_return_association_id(prior.operation).unwrap();
    prior.stock_revision = HubRevision::from_value(20).unwrap();
    prior.return_operation = id(211);
    let ids = ReturnIdentities::from_operation_bytes(prior.return_operation).unwrap();
    prior.returned = ids.returned;
    prior.returned_population = ids.population;
    prior.returned_group = ids.group;
    prior.return_stock_revision = HubRevision::from_value(10).unwrap();
    prior.explanation_operation = id(212);
    prior.explanation_stock_revision = HubRevision::from_value(9).unwrap();
    prior.count_operation = id(213);
    prior.count_stock_revision = HubRevision::from_value(8).unwrap();
    prior.explanation_trace = id(214);
    prior.anchor_group = id(215);
    prior.recorded_at = at(200);
    prior.validate().unwrap();
    f.associations.push(prior);
    f
}

fn plan(f: &Fixture) -> Result<CountReturnAssociationPlan, CountReturnAssociationError> {
    let mut command = f.command();
    command.expected_returns_prior = None; // The prior row belongs to another count.
    command.plan(&f.context(), id(240), f.original.actor, at(250))
}

fn alias(f: &mut Fixture, operation: HubRecordId, revision: HubRevision) {
    let row = &mut f.associations[0];
    row.operation = operation;
    row.id = count_return_association_id(operation).unwrap();
    row.stock_revision = revision;
    row.validate().unwrap();
}

#[test]
fn a_prior_association_cannot_be_the_same_root_as_selected_explanation_at_the_same_revision() {
    let mut f = fixture();
    assert!(plan(&f).is_ok());
    let (op, revision) = (f.original.operation, f.original.known_event.stock_revision);
    alias(&mut f, op, revision);
    assert_eq!(plan(&f), Err(CountReturnAssociationError::InvalidHistory));
}

#[test]
fn classification_and_association_are_distinct_roots_even_when_identity_revision_agree() {
    let mut f = fixture();
    let correction = CountExplanationAmendment {
        operation: id(216),
        stock_revision: f.cut.next().unwrap(),
        explanation: f.original.operation,
        before: packs(2),
        after: packs(1),
    };
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        f.account.explanations().to_vec(),
        vec![correction],
        Vec::new(),
    )
    .unwrap();
    f.cut = correction.stock_revision;
    assert!(plan(&f).is_ok());
    alias(&mut f, correction.operation, correction.stock_revision);
    assert_eq!(plan(&f), Err(CountReturnAssociationError::InvalidHistory));
}

#[test]
fn recipient_correction_and_association_cannot_share_a_root_but_references_can_repeat() {
    let mut f = fixture();
    let correction = RecipientAttributionCommand {
        departure_operation: f.original.operation,
        expected_stock_revision: f.cut,
        expected_catalog_revision: f.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: f.original.known_event.hub,
                stock_revision: f.cut,
                catalog: &f.catalog,
                latest_recorded_at: f.clock,
                uncertain_submissions: false,
            },
            original: &f.recipient,
            history: &[],
            linked_issued_versions: &[],
        },
        id(216),
        f.original.actor,
        at(205),
    )
    .unwrap();
    f.cut = correction.stamp.stock_revision;
    f.clock = correction.stamp.recorded_at;
    let (op, revision) = (correction.stamp.operation, correction.stamp.stock_revision);
    f.recipient_history.push(correction);
    assert!(plan(&f).is_ok());
    alias(&mut f, op, revision);
    assert_eq!(plan(&f), Err(CountReturnAssociationError::InvalidHistory));
}

#[test]
fn repeated_explanation_identity_cannot_claim_a_different_count_owner_or_source() {
    for fault in 0..5 {
        let mut f = fixture();
        assert!(plan(&f).is_ok());
        let row = &mut f.associations[0];
        row.stock_revision = HubRevision::from_value(22).unwrap();
        row.explanation_operation = f.original.operation;
        row.explanation_stock_revision = f.original.known_event.stock_revision;
        row.count_operation = f.account.original().operation;
        row.count_stock_revision = f.account.original().stock_revision;
        row.explanation_trace = f.account.original().trace;
        row.variant = f.account.original().variant;
        row.anchor_group = f.original.count.before.id;
        row.explanation_occurred = f.original.known_event.occurred;
        // Isolate owner/source consistency, not a duplicate Return or graph edge.
        match fault {
            0 => {
                row.count_operation = id(213);
                row.count_stock_revision = HubRevision::from_value(8).unwrap();
            }
            1 => row.explanation_trace = id(214),
            2 => row.variant = id(217),
            3 => row.anchor_group = id(215),
            4 => row.explanation_source_digest = [218; 32],
            _ => unreachable!(),
        }
        row.validate().unwrap();
        f.cut = row.stock_revision;
        assert_eq!(
            plan(&f),
            Err(CountReturnAssociationError::InvalidHistory),
            "fault {fault}"
        );
    }
}

#[test]
fn repeated_canonical_explanation_scope_survives_with_its_original_return_floor() {
    let mut f = fixture();
    let source_digest = plan(&f).unwrap().source.digest().unwrap();
    let row = &mut f.associations[0];
    row.stock_revision = HubRevision::from_value(22).unwrap();
    row.explanation_operation = f.original.operation;
    row.explanation_stock_revision = f.original.known_event.stock_revision;
    row.count_operation = f.account.original().operation;
    row.count_stock_revision = f.account.original().stock_revision;
    row.explanation_trace = f.account.original().trace;
    row.variant = f.account.original().variant;
    row.anchor_group = f.original.count.before.id;
    row.explanation_occurred = f.original.known_event.occurred;
    row.explanation_source_digest = source_digest;
    row.validate().unwrap();
    f.cut = row.stock_revision;
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        f.account.explanations().to_vec(),
        Vec::new(),
        vec![CountExplanationReturns {
            explanation: f.original.operation,
            packs: packs(1),
        }],
    )
    .unwrap();
    let planned = f
        .command()
        .plan(&f.context(), id(240), f.original.actor, at(250))
        .unwrap();
    assert_eq!(
        planned.after.effective_explanations()[0].established_returns,
        packs(2)
    );
}
