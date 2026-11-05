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
        count_explanation: None,
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

#[test]
fn measured_native20_physical_interpretation_digest_vectors() {
    let (mut f, mut record) = fixture();
    assert_eq!(
        record.digest().unwrap(),
        [
            48, 90, 162, 49, 91, 106, 217, 101, 74, 157, 26, 127, 92, 86, 153, 230, 41, 54, 237,
            237, 169, 83, 17, 185, 106, 192, 198, 108, 156, 216, 197, 102
        ]
    );
    recipient(&mut f, &mut record, Some(id(151)), 70);
    assert_eq!(
        record.digest().unwrap(),
        [
            54, 199, 12, 61, 222, 12, 18, 138, 132, 57, 171, 29, 75, 246, 207, 103, 209, 186, 68,
            124, 138, 212, 171, 225, 125, 92, 152, 106, 159, 137, 113, 231
        ]
    );
    quantity(&mut f, &mut record, 0, 80);
    assert_eq!(
        record.digest().unwrap(),
        [
            63, 228, 231, 145, 248, 49, 171, 159, 199, 104, 18, 31, 220, 151, 117, 68, 100, 207,
            222, 113, 50, 231, 205, 164, 160, 205, 145, 150, 41, 126, 204, 195
        ]
    );
    recipient(&mut f, &mut record, None, 71);
    assert_eq!(
        record.digest().unwrap(),
        [
            253, 90, 93, 186, 232, 207, 137, 46, 59, 168, 125, 69, 135, 128, 100, 119, 90, 208,
            235, 26, 142, 96, 50, 106, 232, 2, 78, 44, 35, 90, 253, 232
        ]
    );
}

fn count_records(f: &mut Fixture) -> Vec<ActualKnownInterpretation> {
    use crate::hub::corrections::{
        CountExplanationAmendment, CountLossExplanation, CountLossQuantityAccount,
        OriginalCountLossQuantity,
    };
    let (catalog, count) = crate::hub::counts::shortage_policy_fixture();
    let account = CountLossQuantityAccount::restore_corrected(
        OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: count.before.trace,
            variant: count.before.variant,
            packs: packs(2),
        },
        vec![
            CountLossExplanation {
                operation: id(90),
                stock_revision: rev(6),
                packs: packs(1),
            },
            CountLossExplanation {
                operation: id(91),
                stock_revision: rev(7),
                packs: packs(1),
            },
        ],
        vec![CountExplanationAmendment {
            operation: id(92),
            stock_revision: rev(8),
            explanation: id(90),
            before: packs(1),
            after: packs(0),
        }],
        vec![],
    )
    .unwrap();
    advance(f, rev(20));
    account
        .explanations()
        .iter()
        .map(|row| {
            let original = ActualKnownDeparture {
                operation: row.operation,
                actor: owner(),
                hub: count.count.scope.hub,
                stock_revision: row.stock_revision,
                catalog_revision: catalog.revision(),
                trace: count.before.trace,
                variant: count.references.variant.clone(),
                recipient: catalog.get(id(5)).unwrap().clone(),
                occurred: known(70),
                recorded_at: at(120 + (row.stock_revision.value() as i64)),
                lines: vec![ActualKnownDepartureLine {
                    group: count.before.id,
                    location: count.references.location.clone(),
                    packs: row.packs,
                }],
            };
            ActualKnownInterpretation {
                stock_revision: rev(20),
                catalog_revision: f.catalog.revision(),
                latest_recorded_at: at(160),
                recipient_original: OriginalRecipientAttribution {
                    hub: original.hub,
                    family: CorrectedDepartureFamily::ExplainedCountLoss,
                    operation: original.operation,
                    stock_revision: original.stock_revision,
                    catalog_revision: original.catalog_revision,
                    recorded_at: original.recorded_at,
                    actor: original.actor,
                    recipient: original.recipient.clone(),
                    populations: vec![original.trace],
                    issued_versions: vec![],
                },
                original,
                recipient_history: vec![],
                quantities: vec![],
                count_explanation: Some(std::sync::Arc::new(ActualKnownCountExplanation {
                    count: count.clone(),
                    account: account.clone(),
                })),
            }
        })
        .collect()
}
fn validate_records(
    f: &Fixture,
    records: &[ActualKnownInterpretation],
) -> Result<Vec<ActualKnownInterpretationView>, ActualReportError> {
    let source = super::source::witness(f);
    let known: Vec<_> = records.iter().map(|r| r.original.clone()).collect();
    let context = ActualNewDepartureContext {
        review: f.context(),
        historical_source: ActualSourceContext {
            source: &source,
            lineage: &f.graph,
            balancing_group: &f.group,
            subsequent_activity: &[],
        },
        known_departures: &known,
        prior_review: None,
    };
    super::super::interpretation::validate_set(
        &context,
        &ActualKnownInterpretations {
            hub: f.graph.hub(),
            stock_revision: f.graph.revision(),
            catalog_revision: f.catalog.revision(),
            latest_recorded_at: at(160),
            records: records.to_vec(),
        },
        id(230),
        at(200),
    )
}
#[test]
fn count_interpretations_select_each_explanation_and_preserve_zero_and_recipient_history() {
    let (mut f, _) = fixture();
    let mut records = count_records(&mut f);
    let views = validate_records(&f, &records).unwrap();
    assert_eq!(views[0].original_packs, packs(1));
    assert_eq!(views[0].effective_packs, packs(0));
    assert_eq!(views[1].effective_packs, packs(1));
    assert_eq!(
        records[0]
            .count_explanation
            .as_ref()
            .unwrap()
            .account
            .explained(),
        packs(1)
    );
    let before = records[0].digest().unwrap();
    recipient(&mut f, &mut records[0], None, 93);
    records[1].stock_revision = records[0].stock_revision;
    records[1].latest_recorded_at = records[0].latest_recorded_at;
    // Per-event validation retains Unknown and both original/current quantities.
    let view = records[0].view().unwrap();
    assert!(view.recipients.has_unknown());
    assert!(view.recipients.could_match(Some(id(152))));
    assert!(view.could_match_packs(Some(packs(1))));
    assert!(view.could_match_packs(Some(packs(0))));
    assert_ne!(before, records[0].digest().unwrap());
    assert_eq!(records[0].quantities.len(), 0);
}
#[test]
fn mixed_count_and_physical_interpretations_validate_without_merging_effects() {
    let (mut f, mut physical) = fixture();
    let mut records = count_records(&mut f);
    physical.original.stock_revision = rev(13);
    physical.recipient_original.stock_revision = rev(13);
    physical.quantities = accounts(&physical.original);
    physical.stock_revision = f.graph.revision();
    physical.latest_recorded_at = at(160);
    records.push(physical);
    let views = validate_records(&f, &records).unwrap();
    assert_eq!(
        views
            .iter()
            .map(|v| v.effective_packs.value())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let mut bad = records.clone();
    bad[0].quantities = records[2].quantities.clone();
    assert_eq!(
        validate_records(&f, &bad),
        Err(ActualReportError::InvalidHistory)
    );
    bad = records.clone();
    bad[0].count_explanation = None;
    assert_eq!(
        validate_records(&f, &bad),
        Err(ActualReportError::InvalidHistory)
    );
    bad = records;
    bad[2].count_explanation = bad[0].count_explanation.clone();
    bad[2].quantities.clear();
    assert_eq!(
        validate_records(&f, &bad),
        Err(ActualReportError::InvalidHistory)
    );
}
#[test]
fn count_shared_accounts_cannot_disagree_or_reuse_global_operation_roles_or_revisions() {
    use crate::hub::corrections::CountLossQuantityAccount;
    let (mut f, _) = fixture();
    let records = count_records(&mut f);
    assert!(validate_records(&f, &records).is_ok());
    for fault in 0..7 {
        let mut bad = records.clone();
        let value = std::sync::Arc::make_mut(bad[1].count_explanation.as_mut().unwrap());
        match fault {
            0 => value.count.reason = note("Different retained count reason"),
            1 => {
                value.account = CountLossQuantityAccount::restore(
                    value.account.original(),
                    value.account.explanations().to_vec(),
                )
                .unwrap()
            }
            2 => {
                bad[0].recipient_original.operation = id(92);
            }
            3 => {
                let mut amendments = value.account.amendments().to_vec();
                amendments[0].operation = id(230);
                value.account = CountLossQuantityAccount::restore_corrected(
                    value.account.original(),
                    value.account.explanations().to_vec(),
                    amendments,
                    vec![],
                )
                .unwrap();
                bad[0].count_explanation = bad[1].count_explanation.clone();
            }
            4 => {
                bad[0].original.stock_revision = rev(7);
                bad[0].recipient_original.stock_revision = rev(7);
            }
            5 => {
                value.count.catalog_revision = rev(100);
            }
            _ => {
                value.count.recorded_at = at(201);
            }
        }
        assert!(validate_records(&f, &bad).is_err(), "fault {fault}");
    }
}
#[test]
fn count_interpretation_digest_covers_original_history_current_totals_and_count_witness() {
    use crate::hub::corrections::{CountExplanationReturns, CountLossQuantityAccount};
    let (mut f, _) = fixture();
    let record = count_records(&mut f).remove(1);
    let digest = record.digest().unwrap();
    for fault in 0..4 {
        let mut changed = record.clone();
        let value = std::sync::Arc::make_mut(changed.count_explanation.as_mut().unwrap());
        match fault {
            0 => value.count.reason = note("Different independently retained count basis"),
            1 => {
                value.account = CountLossQuantityAccount::restore_corrected(
                    value.account.original(),
                    value.account.explanations().to_vec(),
                    value.account.amendments().to_vec(),
                    vec![CountExplanationReturns {
                        explanation: record.original.operation,
                        packs: packs(1),
                    }],
                )
                .unwrap()
            }
            2 => {
                let mut amendments = value.account.amendments().to_vec();
                amendments[0].operation = id(94);
                value.account = CountLossQuantityAccount::restore_corrected(
                    value.account.original(),
                    value.account.explanations().to_vec(),
                    amendments,
                    vec![],
                )
                .unwrap();
            }
            _ => changed.latest_recorded_at = at(170),
        }
        assert_ne!(changed.digest().unwrap(), digest, "fault {fault}");
    }
}

#[test]
fn a_shared_count_explanation_cannot_be_impersonated_by_a_physical_original() {
    let (mut f, mut physical) = fixture();
    let mut records = count_records(&mut f);
    let second = records.pop().unwrap();
    physical.original.operation = second.original.operation;
    physical.original.stock_revision = second.original.stock_revision;
    physical.recipient_original.operation = second.original.operation;
    physical.recipient_original.stock_revision = second.original.stock_revision;
    physical.quantities = accounts(&physical.original);
    physical.stock_revision = f.graph.revision();
    records.push(physical);
    assert_eq!(
        validate_records(&f, &records),
        Err(ActualReportError::InvalidHistory)
    );
}

#[test]
fn one_explanation_cannot_belong_to_two_distinct_count_accounts() {
    use crate::hub::{
        corrections::CountLossQuantityAccount,
        counts::{count_reconciliation_id, count_record_id},
    };
    let (mut f, _) = fixture();
    let mut records = count_records(&mut f);
    // Move classification history beyond both independent count-post revisions;
    // avoid the raw report's separate revision while retaining identical shared IDs.
    for record in &mut records {
        let value = std::sync::Arc::make_mut(record.count_explanation.as_mut().unwrap());
        let mut explanations = value.account.explanations().to_vec();
        for row in &mut explanations {
            row.stock_revision = rev(row.stock_revision.value() + 8);
        }
        // No amendments: their already count-bound roles must not mask the
        // independently missing explanation ownership check.
        let amendments = vec![];
        value.account = CountLossQuantityAccount::restore_corrected(
            value.account.original(),
            explanations,
            amendments,
            vec![],
        )
        .unwrap();
        record.original.stock_revision = rev(record.original.stock_revision.value() + 8);
        record.recipient_original.stock_revision = record.original.stock_revision;
    }
    let value = std::sync::Arc::make_mut(records[1].count_explanation.as_mut().unwrap());
    value.count.operation = id(22);
    value.count.id = count_reconciliation_id(id(22)).unwrap();
    value.count.stock_revision = rev(9);
    value.count.count.operation = id(23);
    value.count.count.id = count_record_id(id(23)).unwrap();
    value.count.count.recorded_revision = rev(8);
    value.count.count.scope.stock_revision = rev(7);
    value.count.before.id = id(110);
    value.count.count.scope.group = id(110);
    value.count.validate().unwrap();
    let mut original = value.account.original();
    original.operation = value.count.operation;
    original.reconciliation = value.count.id;
    original.stock_revision = value.count.stock_revision;
    value.account = CountLossQuantityAccount::restore_corrected(
        original,
        value.account.explanations().to_vec(),
        value.account.amendments().to_vec(),
        vec![],
    )
    .unwrap();
    records[1].original.lines[0].group = id(110);
    assert!(records[0].view().is_ok());
    assert!(records[1].view().is_ok());
    assert_eq!(
        validate_records(&f, &records),
        Err(ActualReportError::InvalidHistory)
    );
}

#[test]
fn count_observation_is_an_original_bound_dependency_for_ids_and_revisions() {
    use crate::hub::counts::count_record_id;
    for collision in [id(230), id(180), id(90)] {
        let (mut f, _) = fixture();
        let mut records = count_records(&mut f);
        for record in &mut records {
            let value = std::sync::Arc::make_mut(record.count_explanation.as_mut().unwrap());
            value.count.count.operation = collision;
            value.count.count.id = count_record_id(collision).unwrap();
            value.count.validate().unwrap();
        }
        if collision == id(90) {
            assert_eq!(records[0].view(), Err(ActualReportError::InvalidHistory));
        } else {
            assert!(records.iter().all(|record| record.view().is_ok()));
            assert_eq!(
                validate_records(&f, &records),
                Err(ActualReportError::InvalidIdentity)
            );
        }
    }
    let (mut f, mut physical) = fixture();
    let mut records = count_records(&mut f);
    let observation_revision = records[0]
        .count_explanation
        .as_ref()
        .unwrap()
        .count
        .count
        .recorded_revision;
    physical.original.stock_revision = observation_revision;
    physical.recipient_original.stock_revision = observation_revision;
    physical.quantities = accounts(&physical.original);
    physical.stock_revision = f.graph.revision();
    assert!(physical.view().is_ok());
    records.push(physical);
    assert_eq!(
        validate_records(&f, &records),
        Err(ActualReportError::InvalidHistory)
    );
}

mod count_capture;
