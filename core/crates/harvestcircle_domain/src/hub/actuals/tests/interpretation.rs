use super::*;
use crate::hub::{
    corrections::{
        AttributionCut, CorrectedDepartureFamily, DepartureQuantityAccount,
        DepartureQuantityAmendment, DepartureQuantityDelta, OriginalDepartureQuantity,
        OriginalRecipientAttribution, RecipientAttributionCommand, RecipientAttributionContext,
    },
    reference::{HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue},
};
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn advance(f: &mut Fixture, revision: HubRevision) {
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        revision,
        f.graph.nodes().to_vec(),
    )
    .unwrap();
    f.facts.expected_stock_revision = revision;
    f.facts.expected_catalog_revision = f.catalog.revision();
}
fn fixture() -> (Fixture, ActualKnownInterpretation) {
    let mut f = Fixture::new(known(120));
    for (n, label) in [
        (151, "Other fictional party B"),
        (152, "Other fictional party C"),
    ] {
        ReferenceCommand {
            expected_catalog_revision: f.catalog.revision(),
            mutation: ReferenceMutation::Create {
                id: id(n),
                value: ReferenceValue::Party {
                    label: HubLabel::new(label).unwrap(),
                },
            },
        }
        .apply(&mut f.catalog)
        .unwrap();
    }
    let original = ActualKnownDeparture {
        actor: owner(),
        operation: id(240),
        hub: f.graph.hub(),
        stock_revision: rev(f.original.stock_revision.value() + 1),
        catalog_revision: f.catalog.revision(),
        trace: f.group.trace,
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(f.facts.recipient).unwrap().clone(),
        occurred: known(120),
        recorded_at: at(160),
        lines: vec![ActualKnownDepartureLine {
            group: id(245),
            location: f.catalog.get(f.group.location).unwrap().clone(),
            packs: packs(2),
        }],
    };
    advance(&mut f, rev(original.stock_revision.value() + 1));
    let record = ActualKnownInterpretation {
        stock_revision: f.graph.revision(),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        recipient_original: OriginalRecipientAttribution {
            hub: original.hub,
            family: CorrectedDepartureFamily::ReviewedActual,
            operation: original.operation,
            stock_revision: original.stock_revision,
            catalog_revision: original.catalog_revision,
            recorded_at: original.recorded_at,
            actor: original.actor,
            recipient: original.recipient.clone(),
            populations: vec![original.trace],
            issued_versions: vec![],
        },
        quantities: accounts(&original),
        original,
        recipient_history: vec![],
    };
    record.view().unwrap();
    (f, record)
}
fn accounts(original: &ActualKnownDeparture) -> Vec<DepartureQuantityAccount> {
    original
        .lines
        .iter()
        .map(|line| {
            DepartureQuantityAccount::restore(
                OriginalDepartureQuantity {
                    operation: original.operation,
                    group: line.group,
                    trace: original.trace,
                    variant: original.variant.id,
                    stock_revision: original.stock_revision,
                    packs: line.packs,
                },
                vec![],
                packs(0),
            )
            .unwrap()
        })
        .collect()
}
fn recipient(
    f: &mut Fixture,
    record: &mut ActualKnownInterpretation,
    value: Option<HubRecordId>,
    n: u8,
) {
    let change = RecipientAttributionCommand {
        departure_operation: record.original.operation,
        expected_stock_revision: record.stock_revision,
        expected_catalog_revision: record.catalog_revision,
        expected_prior: record.recipient_history.last().map(|r| r.stamp.operation),
        recipient: value,
        basis: note("Independently reviewed whole-event recipient, no physical effect"),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: record.original.hub,
                stock_revision: record.stock_revision,
                catalog: &f.catalog,
                latest_recorded_at: record.latest_recorded_at,
                uncertain_submissions: false,
            },
            original: &record.recipient_original,
            history: &record.recipient_history,
            linked_issued_versions: &record.recipient_original.issued_versions,
        },
        id(n),
        owner(),
        at(165 + i64::from(record.recipient_history.len() as u32)),
    )
    .unwrap();
    record.stock_revision = change.stamp.stock_revision;
    record.latest_recorded_at = change.stamp.recorded_at;
    record.recipient_history.push(change);
    advance(f, record.stock_revision);
}
fn quantity(f: &mut Fixture, record: &mut ActualKnownInterpretation, after: u32, n: u8) {
    let original = record.quantities[0].original();
    let before = original.packs;
    record.stock_revision = rev(record.stock_revision.value() + 1);
    let delta = if after < before.value() {
        DepartureQuantityDelta::IdentifiedFoundRemainder(packs(before.value() - after))
    } else {
        DepartureQuantityDelta::AdditionalDeparture(packs(after - before.value()))
    };
    record.quantities[0] = DepartureQuantityAccount::restore(
        original,
        vec![DepartureQuantityAmendment {
            operation: id(n),
            stock_revision: record.stock_revision,
            before,
            after: packs(after),
            delta,
        }],
        packs(0),
    )
    .unwrap();
    advance(f, record.stock_revision);
}
fn plan(
    f: &Fixture,
    known: &[ActualKnownDeparture],
    records: &[ActualKnownInterpretation],
) -> Result<ActualNewDeparturePlan, ActualReportError> {
    let source = super::source::witness(f);
    ActualNewDepartureCommand {
        facts: f.facts.clone(),
        source_operation: source.operation,
        source_group: source.group.id,
        duplicate_review_basis: note(
            "Native originals and every interpretation independently reviewed",
        ),
    }
    .plan_with_interpretations(
        &ActualNewDepartureContext {
            review: f.context(),
            historical_source: ActualSourceContext {
                source: &source,
                lineage: &f.graph,
                balancing_group: &f.group,
                subsequent_activity: &[],
            },
            known_departures: known,
            prior_review: None,
        },
        &ActualKnownInterpretations {
            hub: f.graph.hub(),
            stock_revision: f.graph.revision(),
            catalog_revision: f.catalog.revision(),
            latest_recorded_at: records
                .first()
                .map_or(f.original.recorded_at, |r| r.latest_recorded_at),
            records: records.to_vec(),
        },
        id(230),
        owner(),
        at(200),
    )
}
fn link(f: &Fixture, record: &ActualKnownInterpretation) -> ActualDuplicateLink {
    ActualDuplicateLink {
        report: f.original.id,
        known_operation: record.original.operation,
        known_trace: record.original.trace,
        known_variant: record.original.variant.id,
        verification: note("Explicit same original event; no second physical effect"),
    }
}
#[test]
fn original_current_and_unknown_recipients_remain_duplicates_at_effective_zero() {
    let (mut f, mut record) = fixture();
    let original_party = record.original.recipient.id;
    recipient(&mut f, &mut record, Some(id(151)), 70);
    quantity(&mut f, &mut record, 0, 80);
    let view = record.view().unwrap();
    assert_eq!(view.original_packs, packs(2));
    assert_eq!(view.effective_packs, packs(0));
    for party in [original_party, id(151)] {
        f.facts.recipient = party;
        assert_eq!(
            plan(&f, &[record.original.clone()], &[record.clone()]),
            Err(ActualReportError::DuplicateReviewRequired)
        );
    }
    f.facts.recipient = id(152);
    assert!(plan(&f, &[record.original.clone()], &[record.clone()]).is_ok());
    recipient(&mut f, &mut record, None, 71);
    assert!(record.view().unwrap().recipients.has_unknown());
    assert_eq!(
        plan(&f, &[record.original.clone()], &[record]),
        Err(ActualReportError::DuplicateReviewRequired)
    );
}
#[test]
fn explicit_links_keep_original_and_current_packs_and_every_recipient_without_a_physical_delta() {
    for total in [0, 4] {
        let (mut f, mut record) = fixture();
        recipient(&mut f, &mut record, Some(id(151)), 70);
        quantity(&mut f, &mut record, total, 80);
        f.original.report.recipient = Some(id(151));
        f.original.report.packs = if total == 0 { None } else { Some(packs(total)) };
        let command = link(&f, &record);
        assert!(
            command
                .plan(
                    &f.original,
                    &record.original,
                    None,
                    id(231),
                    owner(),
                    at(200)
                )
                .is_err()
        );
        let plan = command
            .plan_with_interpretation(&f.original, &record, None, id(231), owner(), at(200))
            .unwrap();
        assert_eq!(plan.known, record.original);
        assert_eq!(plan.original, f.original);
        assert_eq!(plan.interpretation.as_deref(), Some(&record));
        let input = command.fingerprint(f.graph.hub(), owner(), plan.operation);
        let digest = actual_duplicate_result_digest(
            &input,
            &command,
            &plan,
            rev(record.stock_revision.value() + 1),
            record.catalog_revision,
        )
        .unwrap();
        let mut changed = plan.clone();
        changed.interpretation.as_mut().unwrap().latest_recorded_at = at(161);
        assert!(
            actual_duplicate_result_digest(
                &input,
                &command,
                &changed,
                rev(record.stock_revision.value() + 1),
                record.catalog_revision
            )
            .is_err()
        );
        assert_ne!(digest, [0; 32]);
        f.original.report.packs = Some(packs(1));
        assert_eq!(
            command.plan_with_interpretation(&f.original, &record, None, id(231), owner(), at(200)),
            Err(ActualReportError::ConflictingEvidence)
        );
        f.original.report.packs = Some(packs(0));
        assert_eq!(
            command.plan_with_interpretation(&f.original, &record, None, id(231), owner(), at(200)),
            Err(ActualReportError::InvalidInput)
        );
    }
}
#[test]
fn qualified_baseline_cannot_be_missing_extra_duplicate_or_from_a_different_cut() {
    let (mut f, record) = fixture();
    f.facts.recipient = id(152);
    let known = [record.original.clone()];
    assert!(plan(&f, &known, std::slice::from_ref(&record)).is_ok());
    assert_eq!(
        plan(&f, &known, &[]),
        Err(ActualReportError::InvalidNativeContext)
    );
    assert!(plan(&f, &known, &[record.clone(), record.clone()]).is_err());
    assert!(
        plan(
            &f,
            &[known[0].clone(), known[0].clone()],
            &[record.clone(), record.clone()]
        )
        .is_err()
    );
    for fault in 0..7 {
        let mut bad = record.clone();
        match fault {
            0 => bad.stock_revision = rev(bad.stock_revision.value() + 1),
            1 => bad.catalog_revision = rev(bad.catalog_revision.value() + 1),
            2 => bad.latest_recorded_at = at(201),
            3 => bad.original.operation = id(244),
            4 => bad.recipient_original.populations.clear(),
            5 => bad.quantities.clear(),
            _ => bad.recipient_original.recipient = f.catalog.get(id(151)).unwrap().clone(),
        };
        assert!(plan(&f, &known, &[bad]).is_err(), "fault {fault}");
    }
}
#[test]
fn corrupted_unrelated_interpretation_cannot_hide_behind_an_earlier_duplicate() {
    let (f, first) = fixture();
    let mut second = first.clone();
    second.original.operation = id(241);
    second.original.stock_revision = rev(second.original.stock_revision.value() + 1);
    second.recipient_original.stock_revision = second.original.stock_revision;
    second.original.lines[0].group = id(246);
    second.recipient_original.operation = id(241);
    second.quantities = accounts(&second.original);
    second.view().unwrap();
    let known = [first.original.clone(), second.original.clone()];
    second.recipient_original.populations.clear();
    assert_eq!(
        plan(&f, &known, &[first, second]),
        Err(ActualReportError::InvalidHistory)
    );
}
#[test]
fn whole_event_population_entries_cannot_have_different_recipient_heads_or_clocks() {
    let (mut f, mut first) = fixture();
    first.recipient_original.family = CorrectedDepartureFamily::Normal;
    first.recipient_original.populations.push(id(153));
    first.recipient_original.issued_versions.push(id(154));
    recipient(&mut f, &mut first, Some(id(151)), 70);
    f.facts.recipient = id(152);
    let mut second = first.clone();
    second.original.trace = id(153);
    second.original.lines[0].group = id(246);
    second.quantities = accounts(&second.original);
    let known = [first.original.clone(), second.original.clone()];
    assert!(plan(&f, &known, &[first.clone(), second.clone()]).is_ok());
    second.recipient_history.clear();
    second.view().unwrap();
    assert_eq!(
        plan(&f, &known, &[first.clone(), second.clone()]),
        Err(ActualReportError::InvalidHistory)
    );
    second = first.clone();
    second.original.trace = id(153);
    second.original.lines[0].group = id(246);
    second.original.occurred = super::known(121);
    second.quantities = accounts(&second.original);
    second.view().unwrap();
    let known = [first.original.clone(), second.original.clone()];
    assert_eq!(
        plan(&f, &known, &[first, second]),
        Err(ActualReportError::InvalidHistory)
    );
}
#[test]
fn whole_line_quantity_inventory_and_both_totals_are_required() {
    let (_, mut record) = fixture();
    let mut line = record.original.lines[0].clone();
    line.group = id(246);
    line.packs = packs(3);
    record.original.lines.push(line);
    record.quantities = accounts(&record.original);
    record.view().unwrap();
    let before = record.quantities[0].original();
    record.quantities[0] = DepartureQuantityAccount::restore(
        before,
        vec![DepartureQuantityAmendment {
            operation: id(80),
            stock_revision: record.stock_revision,
            before: before.packs,
            after: packs(0),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(before.packs),
        }],
        packs(0),
    )
    .unwrap();
    let view = record.view().unwrap();
    assert_eq!(view.original_packs, packs(5));
    assert_eq!(view.effective_packs, packs(3));
    assert!(view.could_match_packs(Some(packs(5))));
    assert!(view.could_match_packs(Some(packs(3))));
    assert!(!view.could_match_packs(Some(packs(4))));
    record.quantities.pop();
    assert!(record.view().is_err());
}
#[test]
fn interpretation_hash_covers_cut_original_reference_whole_scope_and_history_basis() {
    let (mut f, mut record) = fixture();
    recipient(&mut f, &mut record, Some(id(151)), 70);
    let digest = record.digest().unwrap();
    for fault in 0..6 {
        let mut bad = record.clone();
        match fault {
            0 => bad.latest_recorded_at = at(180),
            1 => bad.stock_revision = rev(bad.stock_revision.value() + 1),
            2 => {
                bad.original.lines[0].location.value = ReferenceValue::Location {
                    label: HubLabel::new("Changed original location body").unwrap(),
                    purpose: crate::hub::reference::LocationPurpose::Receiving,
                }
            }
            3 => {
                bad.recipient_history[0].stamp.basis =
                    note("Different independently asserted basis")
            }
            4 => bad.recipient_original.populations.push(id(155)),
            _ => {
                bad.recipient_history[0].stamp.actor = PublicKey::from_hex(
                    "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
                )
                .unwrap()
            }
        };
        assert!(
            bad.digest().is_err() || bad.digest().unwrap() != digest,
            "fault {fault}"
        );
    }
}
#[test]
fn an_amendment_operation_cannot_also_be_a_different_known_original() {
    let (mut f, mut first) = fixture();
    recipient(&mut f, &mut first, Some(id(151)), 70);
    f.facts.recipient = id(152);
    let mut second = first.clone();
    second.original.operation = id(70);
    second.original.lines[0].group = id(246);
    second.recipient_original.operation = id(70);
    second.recipient_history.clear();
    second.quantities = accounts(&second.original);
    second.view().unwrap();
    let known = [first.original.clone(), second.original.clone()];
    assert_eq!(
        plan(&f, &known, &[first, second]),
        Err(ActualReportError::InvalidHistory)
    );
}

#[test]
fn selected_interpretation_is_bound_even_when_original_and_current_values_are_equal() {
    let (f, record) = fixture();
    let command = link(&f, &record);
    let old = command
        .plan(
            &f.original,
            &record.original,
            None,
            id(231),
            owner(),
            at(200),
        )
        .unwrap();
    let new = command
        .plan_with_interpretation(&f.original, &record, None, id(231), owner(), at(200))
        .unwrap();
    let input = command.fingerprint(f.graph.hub(), owner(), new.operation);
    let revision = rev(record.stock_revision.value() + 1);
    assert_eq!(old.known, new.known);
    assert_eq!(old.original, new.original);
    let old_hash =
        actual_duplicate_result_digest(&input, &command, &old, revision, record.catalog_revision)
            .unwrap();
    let new_hash =
        actual_duplicate_result_digest(&input, &command, &new, revision, record.catalog_revision)
            .unwrap();
    assert_ne!(old_hash, new_hash);
    assert!(
        actual_duplicate_result_digest(
            &input,
            &command,
            &new,
            rev(revision.value() + 1),
            record.catalog_revision
        )
        .is_err()
    );
    let mut changed = new;
    changed.interpretation.as_mut().unwrap().latest_recorded_at = at(180);
    assert_ne!(
        actual_duplicate_result_digest(
            &input,
            &command,
            &changed,
            revision,
            record.catalog_revision
        )
        .unwrap(),
        new_hash
    );
}
#[test]
fn interpretation_capacities_reject_growth_before_following_history() {
    let (mut f, mut record) = fixture();
    recipient(&mut f, &mut record, Some(id(151)), 70);
    let mut excessive = record.clone();
    excessive.recipient_history =
        vec![record.recipient_history[0].clone(); crate::hub::corrections::HUB_CORRECTION_ROWS + 1];
    assert_eq!(excessive.view(), Err(ActualReportError::Capacity));
    excessive = record.clone();
    excessive.quantities =
        vec![record.quantities[0].clone(); crate::hub::dispatch::HUB_DISPATCH_LINES + 1];
    assert_eq!(excessive.view(), Err(ActualReportError::Capacity));
    let count = crate::hub::stock::HUB_STOCK_EVENT_CAPACITY + 1;
    let known = vec![record.original.clone(); count];
    let records = vec![record; count];
    assert_eq!(plan(&f, &known, &records), Err(ActualReportError::Capacity));
}

#[test]
fn proposed_operations_are_disjoint_from_recipient_and_quantity_history() {
    for recipient_change in [false, true] {
        let (mut f, mut record) = fixture();
        if recipient_change {
            recipient(&mut f, &mut record, Some(id(151)), 230);
        } else {
            quantity(&mut f, &mut record, 4, 230);
        }
        f.facts.recipient = id(152);
        record.view().unwrap();
        assert_eq!(
            plan(&f, &[record.original.clone()], &[record]),
            Err(ActualReportError::InvalidIdentity)
        );
        let (mut f, mut record) = fixture();
        if recipient_change {
            recipient(&mut f, &mut record, Some(id(151)), 231);
        } else {
            quantity(&mut f, &mut record, 4, 231);
        }
        let command = link(&f, &record);
        assert!(
            command
                .plan_with_interpretation(&f.original, &record, None, id(232), owner(), at(200))
                .is_ok()
        );
        assert_eq!(
            command.plan_with_interpretation(&f.original, &record, None, id(231), owner(), at(200)),
            Err(ActualReportError::InvalidIdentity)
        );
    }
}
#[test]
fn different_native_operations_cannot_share_revisions_across_original_recipient_or_quantity_roles()
{
    for fault in 0..4 {
        let (mut f, mut first) = fixture();
        if fault == 1 || fault == 2 {
            recipient(&mut f, &mut first, Some(id(151)), 70);
        } else if fault == 3 {
            quantity(&mut f, &mut first, 4, 80);
        }
        let mut second = first.clone();
        second.original.operation = id(241);
        second.original.lines[0].group = id(246);
        second.recipient_original.operation = id(241);
        second.recipient_history.clear();
        second.original.stock_revision = rev(first.original.stock_revision.value() + 1);
        second.recipient_original.stock_revision = second.original.stock_revision;
        second.quantities = accounts(&second.original);
        if fault == 0 {
            second.original.stock_revision = first.original.stock_revision;
            second.recipient_original.stock_revision = first.original.stock_revision;
            second.quantities = accounts(&second.original);
        }
        if fault == 1 {
            second.original.stock_revision = first.stock_revision;
            second.recipient_original.stock_revision = first.stock_revision;
            second.quantities = accounts(&second.original);
        }
        if fault >= 2 {
            let original = second.quantities[0].original();
            second.quantities[0] = DepartureQuantityAccount::restore(
                original,
                vec![DepartureQuantityAmendment {
                    operation: id(81),
                    stock_revision: first.stock_revision,
                    before: original.packs,
                    after: packs(4),
                    delta: DepartureQuantityDelta::AdditionalDeparture(packs(2)),
                }],
                packs(0),
            )
            .unwrap();
        }
        first.view().unwrap();
        second.view().unwrap();
        f.facts.recipient = id(152);
        let known = [first.original.clone(), second.original.clone()];
        assert_eq!(
            plan(&f, &known, &[first, second]),
            Err(ActualReportError::InvalidHistory),
            "fault {fault}"
        );
    }
}
#[test]
fn one_selected_interpretation_cannot_reuse_a_revision_for_different_amendments() {
    let (mut f, mut record) = fixture();
    recipient(&mut f, &mut record, Some(id(151)), 70);
    let original = record.quantities[0].original();
    record.quantities[0] = DepartureQuantityAccount::restore(
        original,
        vec![DepartureQuantityAmendment {
            operation: id(80),
            stock_revision: record.recipient_history[0].stamp.stock_revision,
            before: original.packs,
            after: packs(4),
            delta: DepartureQuantityDelta::AdditionalDeparture(packs(2)),
        }],
        packs(0),
    )
    .unwrap();
    assert_eq!(record.view(), Err(ActualReportError::InvalidHistory));
    assert_eq!(
        link(&f, &record).plan_with_interpretation(
            &f.original,
            &record,
            None,
            id(232),
            owner(),
            at(200)
        ),
        Err(ActualReportError::InvalidHistory)
    );
}
