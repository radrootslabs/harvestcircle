use super::*;
use crate::hub::movement::{WorkAllocation, WorkProgress, WorkRequirement};
use crate::hub::reference::{HubLabel, ReferenceValue};
use crate::hub::stock::QuantityWork;
use crate::hub::treatment::{AcceptanceDecision, AcceptanceStatus, Hold, PhysicalAction};
fn id(n: u64) -> HubRecordId {
    let mut b = [0; 16];
    b[8..].copy_from_slice(&n.to_be_bytes());
    HubRecordId::from_bytes(b).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new(
        "Independently observed separately identified remainder, original overstatement reviewed",
    )
    .unwrap()
}
fn identity(s: &str) -> PhysicalIdentification {
    PhysicalIdentification {
        label: HubLabel::new(s).unwrap(),
        observation: note(),
    }
}
fn target() -> OriginalDepartureQuantity {
    OriginalDepartureQuantity {
        operation: id(200),
        group: id(100),
        trace: id(90),
        variant: id(2),
        stock_revision: rev(10),
        packs: packs(6),
    }
}
fn catalog() -> ReferenceCatalog {
    let mut c = ReferenceCatalog::default();
    for (n, v) in [
        (
            1,
            ReferenceValue::Product {
                label: HubLabel::new("Product").unwrap(),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Original pack").unwrap(),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Location {
                label: HubLabel::new("Current inspected location").unwrap(),
                purpose: crate::hub::reference::LocationPurpose::Storage,
            },
        ),
    ] {
        c.create(c.revision(), id(n), v).unwrap();
    }
    c
}
fn before(group: u64, quantity: u32) -> PhysicalStockGroup {
    PhysicalStockGroup {
        id: id(group),
        trace: id(90),
        variant: id(2),
        parent: (group == 101).then_some(id(100)),
        identification: Some(identity(if group == 100 {
            "original-A"
        } else {
            "original-B"
        })),
        location: id(3),
        packs: packs(quantity),
        revision: rev(3),
        causal_floor: Some(at(70)),
        work: Vec::new(),
    }
}
fn base() -> OriginalPhysicalDebit {
    OriginalPhysicalDebit {
        key: PhysicalDebitKey {
            operation: id(200),
            group: id(100),
        },
        kind: PhysicalDebitKind::NormalDeparture,
        target: target(),
        before: before(100, 6),
        packs: packs(6),
        stock_revision: rev(10),
        recorded_at: at(100),
    }
}
fn extra() -> OriginalPhysicalDebit {
    OriginalPhysicalDebit {
        key: PhysicalDebitKey {
            operation: id(201),
            group: id(101),
        },
        kind: PhysicalDebitKind::AdditionalQuantity,
        target: target(),
        before: before(101, 14),
        packs: packs(2),
        stock_revision: rev(11),
        recorded_at: at(110),
    }
}
fn reservation() -> EstablishedReturnReservation {
    EstablishedReturnReservation {
        operation: id(202),
        debit: base().key,
        packs: packs(2),
        stock_revision: rev(12),
        recorded_at: at(120),
    }
}
fn addition() -> DepartureQuantityAmendment {
    DepartureQuantityAmendment {
        operation: id(201),
        stock_revision: rev(11),
        before: packs(6),
        after: packs(8),
        delta: DepartureQuantityDelta::AdditionalDeparture(packs(2)),
    }
}
fn graph(revision: u64, a: u32, b: u32) -> PhysicalLineage {
    PhysicalLineage::new(
        HubId::from_bytes([1; 16]).unwrap(),
        id(90),
        id(2),
        rev(revision),
        vec![
            PhysicalLineageNode {
                recovery: None,
                id: id(100),
                parent: None,
                trace: id(90),
                variant: id(2),
                created_revision: rev(2),
                packs: packs(a),
            },
            PhysicalLineageNode {
                recovery: None,
                id: id(101),
                parent: Some(id(100)),
                trace: id(90),
                variant: id(2),
                created_revision: rev(3),
                packs: packs(b),
            },
        ],
    )
    .unwrap()
}
fn stamp(n: u64, r: u64) -> TreatmentStamp {
    TreatmentStamp {
        id: id(n),
        stock_revision: rev(r),
        actor: actor(),
        basis: crate::hub::reference::SampleHandlingBasis::WHOLE_PACK_V1,
        recorded_at: at(80),
        occurred: Occurrence::Known(at(80)),
        observation: note(),
    }
}
struct Fixture {
    catalog: ReferenceCatalog,
    target: DepartureQuantityAccount,
    funding: Vec<PhysicalDebitFunding>,
    graph: PhysicalLineage,
    treatment: TreatmentState,
    active: Vec<PhysicalIdentification>,
}
impl Fixture {
    fn new() -> Self {
        let target =
            DepartureQuantityAccount::restore(target(), vec![addition()], packs(2)).unwrap();
        let funding = vec![
            PhysicalDebitFunding::restore(base(), vec![], vec![reservation()], false).unwrap(),
            PhysicalDebitFunding::restore(extra(), vec![], vec![], false).unwrap(),
        ];
        let graph = graph(20, 0, 12);
        let old = super::tests::graph(3, 6, 14);
        let accepted = AcceptanceDecision {
            stamp: stamp(300, 4),
            scope: old.capture(&[id(100), id(101)]).unwrap(),
            status: AcceptanceStatus::Accepted,
        };
        let old_a = super::tests::graph(4, 6, 14);
        let hold_a = Hold {
            stamp: stamp(301, 5),
            scope: old_a.capture(&[id(100)]).unwrap(),
            release: None,
        };
        let old_b = super::tests::graph(5, 6, 14);
        let hold_b = Hold {
            stamp: stamp(302, 6),
            scope: old_b.capture(&[id(101)]).unwrap(),
            release: None,
        };
        let treatment =
            TreatmentState::restore(&graph, vec![accepted], vec![hold_a, hold_b]).unwrap();
        Self {
            catalog: catalog(),
            target,
            funding,
            graph,
            treatment,
            active: vec![identity("original-B")],
        }
    }
    fn context(&self) -> FoundStockContext<'_> {
        FoundStockContext {
            family: CorrectedDepartureFamily::Normal,
            hub: self.graph.hub(),
            stock_revision: rev(20),
            catalog: &self.catalog,
            latest_recorded_at: at(150),
            target: &self.target,
            funding: &self.funding,
            lineage: &self.graph,
            treatment: &self.treatment,
            active_identifications: &self.active,
            uncertain_submissions: false,
        }
    }
    fn command(&self, allocations: Vec<FoundStockInput>) -> FoundStockCommand {
        FoundStockCommand {
            target_operation: id(200),
            target_group: id(100),
            expected_stock_revision: rev(20),
            expected_catalog_revision: self.catalog.revision(),
            allocations,
            basis: note(),
        }
    }
}
fn found(original: OriginalPhysicalDebit, n: u32, label: &str) -> FoundStockInput {
    FoundStockInput {
        cohort_evidence: note(),
        debit: original.key,
        packs: packs(n),
        location: id(3),
        identification: identity(label),
    }
}

#[test]
fn return_reservation_stays_on_a_even_when_b_keeps_event_total_above_returns() {
    let fixture = Fixture::new();
    assert_eq!(fixture.target.effective(), packs(8));
    assert_eq!(fixture.target.established_returns(), packs(2));
    assert_eq!(fixture.funding[0].available().unwrap(), packs(4));
    assert_eq!(fixture.funding[1].available().unwrap(), packs(2));
    // Aggregate 8-6=2 still covers the two returns. Exact A funding correctly refuses.
    let command = fixture.command(vec![found(base(), 6, "independent-A-found")]);
    assert_eq!(
        command.plan(&fixture.context(), id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidQuantity)
    );
    assert_eq!(fixture.target.effective(), packs(8));
    assert_eq!(fixture.funding[0].returned(), packs(2));
}

#[test]
fn classification_originals_cannot_be_claimed_as_an_ordinary_physical_debit() {
    let f = Fixture::new();
    let command = f.command(vec![found(base(), 1, "independent-found")]);
    let mut c = f.context();
    c.family = CorrectedDepartureFamily::ExplainedCountLoss;
    assert_eq!(
        command.plan(&c, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::UnsupportedFamily)
    );
    c.family = CorrectedDepartureFamily::ReviewedActual;
    assert_eq!(
        command.plan(&c, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
}
#[test]
fn multi_parent_recovery_keeps_zero_parent_separate_children_holds_and_review_required() {
    let fixture = Fixture::new();
    let input = vec![
        found(base(), 4, "found-from-A"),
        found(extra(), 2, "found-from-B"),
    ];
    let plan = fixture
        .command(input)
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    assert_eq!(plan.original_target, target());
    assert_eq!(plan.amendment.before, packs(8));
    assert_eq!(plan.amendment.after, packs(2));
    assert_eq!(plan.allocations.len(), 2);
    assert_eq!(plan.allocations[0].group.parent, Some(id(100)));
    assert_eq!(plan.allocations[1].group.parent, Some(id(101)));
    assert_ne!(plan.allocations[0].group.id, plan.allocations[1].group.id);
    assert_eq!(plan.after_lineage.node(id(100)).unwrap().packs, packs(0));
    let treatment = plan
        .after_treatment
        .project(&plan.after_lineage, fixture.catalog.handling_basis())
        .unwrap();
    for (index, allocation) in plan.allocations.iter().enumerate() {
        assert_eq!(allocation.group.causal_floor, Some(at(200)));
        assert!(allocation.group.work.is_empty());
        assert_eq!(allocation.credit.allocation, u8::try_from(index).unwrap());
        assert_eq!(allocation.credit.recorded_at, at(200));
        let current = treatment
            .groups
            .iter()
            .find(|g| g.group == allocation.group.id)
            .unwrap();
        assert_eq!(current.acceptance, AcceptanceStatus::ReviewRequired);
        assert!(!current.allows(
            fixture.catalog.handling_basis(),
            PhysicalAction::NormalDeparture
        ));
        assert_eq!(
            current.active_holds,
            vec![id(if index == 0 { 301 } else { 302 })]
        );
    }
    assert_eq!(fixture.graph.node(id(100)).unwrap().packs, packs(0));
    assert_eq!(fixture.treatment.decisions().len(), 1);
    assert_eq!(
        plan.review_required.stamp.occurred,
        Occurrence::Known(at(200))
    );
    assert_eq!(plan.review_required.scope.anchors().len(), 2);
}

#[test]
fn post_debit_remainder_hold_does_not_gain_a_separately_observed_found_cohort() {
    let mut fixture = Fixture::new();
    let post = graph(15, 0, 12);
    let mut stamp = stamp(303, 16);
    stamp.recorded_at = at(140);
    let mut holds = fixture.treatment.holds().to_vec();
    holds.push(Hold {
        stamp,
        scope: post.capture(&[id(101)]).unwrap(),
        release: None,
    });
    fixture.treatment = TreatmentState::restore(
        &fixture.graph,
        fixture.treatment.decisions().to_vec(),
        holds,
    )
    .unwrap();
    let input = found(extra(), 2, "independent-B-found");
    let plan = fixture
        .command(vec![input.clone()])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    assert_eq!(plan.allocations[0].cohort_evidence, input.cohort_evidence);
    let projected = plan
        .after_treatment
        .project(&plan.after_lineage, fixture.catalog.handling_basis())
        .unwrap();
    let child = projected
        .groups
        .iter()
        .find(|g| g.group == plan.allocations[0].group.id)
        .unwrap();
    assert_eq!(child.active_holds, vec![id(302)]);
    assert_eq!(child.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(
        projected
            .groups
            .iter()
            .find(|g| g.group == id(101))
            .unwrap()
            .active_holds,
        vec![id(302), id(303)]
    );
}
#[test]
fn existing_recovery_and_returns_exhaust_only_their_exact_scope() {
    let credit = FoundFundingCredit {
        operation: id(210),
        allocation: 0,
        debit: base().key,
        packs: packs(3),
        stock_revision: rev(13),
        recorded_at: at(130),
    };
    let funding =
        PhysicalDebitFunding::restore(base(), vec![credit], vec![reservation()], false).unwrap();
    assert_eq!(funding.recovered(), packs(3));
    assert_eq!(funding.available().unwrap(), packs(1));
    let too_much = FoundFundingCredit {
        packs: packs(5),
        ..credit
    };
    assert_eq!(
        PhysicalDebitFunding::restore(base(), vec![too_much], vec![reservation()], false),
        Err(PhysicalCorrectionError::ReturnConflict)
    );
    let exhausted = FoundFundingCredit {
        packs: packs(4),
        ..credit
    };
    assert_eq!(
        PhysicalDebitFunding::restore(base(), vec![exhausted], vec![reservation()], false)
            .unwrap()
            .available()
            .unwrap(),
        packs(0)
    );
}
#[test]
fn ambiguous_return_funding_cannot_be_spent_as_if_no_return_existed() {
    let mut fixture = Fixture::new();
    fixture.funding[0] =
        PhysicalDebitFunding::restore(base(), vec![], vec![reservation()], true).unwrap();
    assert_eq!(
        fixture.command(vec![found(base(), 1, "new-found")]).plan(
            &fixture.context(),
            id(400),
            actor(),
            at(200)
        ),
        Err(PhysicalCorrectionError::UnresolvedReturnScope)
    );
    assert!(
        fixture
            .command(vec![found(extra(), 1, "new-from-B")])
            .plan(&fixture.context(), id(400), actor(), at(200))
            .is_ok()
    );
}
#[test]
fn funding_credit_key_identity_and_native_revision_are_not_interchangeable() {
    let credit = FoundFundingCredit {
        operation: id(210),
        allocation: 0,
        debit: base().key,
        packs: packs(1),
        stock_revision: rev(13),
        recorded_at: at(130),
    };
    assert_eq!(
        PhysicalDebitFunding::restore(base(), vec![credit, credit], vec![], false),
        Err(PhysicalCorrectionError::Duplicate)
    );
    assert_eq!(
        PhysicalDebitFunding::restore(
            base(),
            vec![FoundFundingCredit {
                debit: extra().key,
                ..credit
            }],
            vec![],
            false
        ),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    assert_eq!(
        PhysicalDebitFunding::restore(
            base(),
            vec![FoundFundingCredit {
                operation: id(200),
                ..credit
            }],
            vec![],
            false
        ),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    assert_eq!(
        PhysicalDebitFunding::restore(
            base(),
            vec![
                credit,
                FoundFundingCredit {
                    operation: id(211),
                    allocation: 1,
                    ..credit
                }
            ],
            vec![],
            false
        ),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    assert_eq!(
        PhysicalDebitFunding::restore(base(), vec![], vec![reservation(), reservation()], false),
        Err(PhysicalCorrectionError::Duplicate)
    );
}
#[test]
fn multiple_allocations_cannot_spend_the_same_remaining_pack_twice() {
    let fixture = Fixture::new();
    let command = fixture.command(vec![
        found(base(), 3, "found-A-first"),
        found(base(), 2, "found-A-second"),
    ]);
    assert_eq!(
        command.plan(&fixture.context(), id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidQuantity)
    );
    let command = fixture.command(vec![
        found(base(), 2, "found-A-first"),
        found(base(), 2, "found-A-second"),
    ]);
    let plan = command
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    assert_eq!(plan.allocations[0].available_before, packs(4));
    assert_eq!(plan.allocations[1].available_before, packs(2));
}
#[test]
fn active_and_historical_funding_labels_cannot_identify_new_children() {
    let fixture = Fixture::new();
    for label in ["original-A", "original-B"] {
        assert_eq!(
            fixture.command(vec![found(base(), 1, label)]).plan(
                &fixture.context(),
                id(400),
                actor(),
                at(200)
            ),
            Err(PhysicalCorrectionError::IdentityConflict)
        );
    }
    assert_eq!(
        fixture
            .command(vec![
                found(base(), 1, "same-new-label"),
                found(extra(), 1, "same-new-label")
            ])
            .plan(&fixture.context(), id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::IdentityConflict)
    );
}
#[test]
fn stale_wrong_target_and_unknown_cut_cannot_create_a_child() {
    let fixture = Fixture::new();
    let command = fixture.command(vec![found(base(), 1, "new-found")]);
    let mut context = fixture.context();
    context.stock_revision = rev(19);
    assert_eq!(
        command.plan(&context, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::StaleRevision)
    );
    context.stock_revision = rev(20);
    context.uncertain_submissions = true;
    assert_eq!(
        command.plan(&context, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::UncertainSubmission)
    );
    context.uncertain_submissions = false;
    assert_eq!(
        command.plan(&context, id(400), actor(), at(149)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    assert_eq!(
        FoundStockCommand {
            target_group: id(999),
            ..command
        }
        .plan(&context, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidScope)
    );
}
#[test]
fn original_and_added_physical_scopes_are_complete_and_cannot_be_cross_funded() {
    let mut fixture = Fixture::new();
    fixture.funding.pop();
    assert_eq!(
        fixture.command(vec![found(base(), 1, "new-found")]).plan(
            &fixture.context(),
            id(400),
            actor(),
            at(200)
        ),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    let mut fixture = Fixture::new();
    fixture.funding.push(fixture.funding[0].clone());
    assert_eq!(
        fixture.command(vec![found(base(), 1, "new-found")]).plan(
            &fixture.context(),
            id(400),
            actor(),
            at(200)
        ),
        Err(PhysicalCorrectionError::Duplicate)
    );
    let foreign = OriginalPhysicalDebit {
        target: OriginalDepartureQuantity {
            operation: id(999),
            ..target()
        },
        ..base()
    };
    assert_eq!(
        PhysicalDebitFunding::restore(foreign, vec![], vec![], false),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
}

#[test]
fn recovered_stock_does_not_resurrect_completed_or_retired_requirements() {
    let mut fixture = Fixture::new();
    let mut original = base();
    let completed = WorkRequirement {
        id: id(500),
        trace: id(90),
        variant: id(2),
        target: Some(fixture.catalog.get(id(3)).unwrap().clone()),
        quantity: QuantityWork::restore(packs(20), packs(6), packs(0), packs(14)).unwrap(),
        revision: rev(1),
    };
    let retired = WorkRequirement {
        id: id(501),
        quantity: QuantityWork::restore(packs(20), packs(0), packs(6), packs(14)).unwrap(),
        ..completed.clone()
    };
    original.before.work = vec![
        WorkAllocation {
            requirement: completed,
            progress: WorkProgress::Completed,
        },
        WorkAllocation {
            requirement: retired,
            progress: WorkProgress::NoLongerRequired,
        },
    ];
    let immutable = original.clone();
    fixture.funding[0] =
        PhysicalDebitFunding::restore(original, vec![], vec![reservation()], false).unwrap();
    let plan = fixture
        .command(vec![found(base(), 1, "new-observed-found")])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    assert!(plan.allocations[0].group.work.is_empty());
    assert_eq!(plan.allocations[0].funding, immutable);
    assert_eq!(fixture.funding[0].original(), &immutable);
    assert_eq!(
        immutable.before.work[0].requirement.quantity.completed(),
        packs(6)
    );
    assert_eq!(
        immutable.before.work[1]
            .requirement
            .quantity
            .no_longer_required(),
        packs(6)
    );
}
#[test]
fn resurrected_or_expanded_parents_are_not_valid_recovery_inputs() {
    for (a, b) in [(1, 12), (0, 13)] {
        let mut fixture = Fixture::new();
        fixture.graph = graph(20, a, b);
        assert_eq!(
            fixture.command(vec![found(base(), 1, "new-found")]).plan(
                &fixture.context(),
                id(400),
                actor(),
                at(200)
            ),
            Err(PhysicalCorrectionError::InvalidHistory)
        );
    }
}
#[test]
fn funding_clock_must_fit_the_captured_cut_and_native_journal_order() {
    let fixture = Fixture::new();
    let command = fixture.command(vec![found(base(), 1, "new-found")]);
    let mut context = fixture.context();
    context.latest_recorded_at = at(119);
    assert_eq!(
        command.plan(&context, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    let mut fixture = Fixture::new();
    fixture.funding[1] = PhysicalDebitFunding::restore(
        OriginalPhysicalDebit {
            recorded_at: at(90),
            ..extra()
        },
        vec![],
        vec![],
        false,
    )
    .unwrap();
    assert_eq!(
        fixture.command(vec![found(base(), 1, "new-found")]).plan(
            &fixture.context(),
            id(400),
            actor(),
            at(200)
        ),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
}
#[test]
fn initial_review_is_distinct_from_ordinary_acceptance_and_later_review_keeps_holds() {
    use crate::hub::treatment::{
        TreatmentAction, TreatmentChange, TreatmentCommand, TreatmentSelection,
    };
    let fixture = Fixture::new();
    let plan = fixture
        .command(vec![found(base(), 1, "new-found")])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    let review = &plan.review_required;
    let mut ordinary = fixture.treatment.decisions().to_vec();
    ordinary.push(AcceptanceDecision {
        stamp: review.stamp.clone(),
        scope: review.scope.clone(),
        status: AcceptanceStatus::ReviewRequired,
    });
    assert!(
        TreatmentState::restore(
            &plan.after_lineage,
            ordinary,
            fixture.treatment.holds().to_vec()
        )
        .is_err()
    );
    let command = TreatmentCommand {
        expected_stock_revision: rev(21),
        expected_catalog_revision: fixture.catalog.revision(),
        action: TreatmentAction::SetAcceptance {
            selection: TreatmentSelection::new(vec![plan.allocations[0].group.id]).unwrap(),
            status: AcceptanceStatus::Accepted,
        },
        occurred: Occurrence::Known(at(201)),
        observation: note(),
    };
    let later = command
        .plan(
            &fixture.catalog,
            &plan.after_lineage,
            &plan.after_treatment,
            id(600),
            actor(),
            at(201),
        )
        .unwrap();
    let TreatmentChange::Acceptance(decision) = later.change else {
        panic!("explicit acceptance required")
    };
    let after_graph = PhysicalLineage::new(
        plan.hub,
        id(90),
        id(2),
        rev(22),
        plan.after_lineage.nodes().to_vec(),
    )
    .unwrap();
    let mut decisions = plan.after_treatment.decisions().to_vec();
    decisions.push(*decision);
    let state = TreatmentState::restore_with_initial_reviews(
        &after_graph,
        decisions,
        plan.after_treatment.holds().to_vec(),
        plan.after_treatment.initial_reviews().to_vec(),
    )
    .unwrap();
    let projection = state
        .project(&after_graph, fixture.catalog.handling_basis())
        .unwrap();
    let child = projection
        .groups
        .iter()
        .find(|g| g.group == plan.allocations[0].group.id)
        .unwrap();
    assert_eq!(state.initial_reviews().len(), 1);
    assert_eq!(child.acceptance, AcceptanceStatus::Accepted);
    assert_eq!(child.active_holds, vec![id(301)]);
    assert!(!child.allows(
        fixture.catalog.handling_basis(),
        PhysicalAction::NormalDeparture
    ));
}
#[test]
fn birth_review_history_survives_a_depleted_child_and_rejects_malformed_births() {
    let fixture = Fixture::new();
    let plan = fixture
        .command(vec![found(base(), 1, "new-found")])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    let mut nodes = plan.after_lineage.nodes().to_vec();
    nodes
        .iter_mut()
        .find(|n| n.id == plan.allocations[0].group.id)
        .unwrap()
        .packs = packs(0);
    let graph = PhysicalLineage::new(plan.hub, id(90), id(2), rev(22), nodes).unwrap();
    let state = TreatmentState::restore_with_initial_reviews(
        &graph,
        fixture.treatment.decisions().to_vec(),
        fixture.treatment.holds().to_vec(),
        vec![plan.review_required.clone()],
    )
    .unwrap();
    assert_eq!(state.initial_reviews().len(), 1);
    assert_eq!(state.record_count(), 4);
    let bad = [
        InitialGroupReview {
            stamp: TreatmentStamp {
                occurred: Occurrence::Unknown,
                ..plan.review_required.stamp.clone()
            },
            ..plan.review_required.clone()
        },
        InitialGroupReview {
            stamp: TreatmentStamp {
                stock_revision: rev(22),
                ..plan.review_required.stamp.clone()
            },
            ..plan.review_required.clone()
        },
        InitialGroupReview {
            scope: plan.after_lineage.capture(&[id(101)]).unwrap(),
            ..plan.review_required.clone()
        },
    ];
    for review in bad {
        assert!(
            TreatmentState::restore_with_initial_reviews(
                &plan.after_lineage,
                fixture.treatment.decisions().to_vec(),
                fixture.treatment.holds().to_vec(),
                vec![review]
            )
            .is_err()
        );
    }
    assert!(
        TreatmentState::restore_with_initial_reviews(
            &plan.after_lineage,
            fixture.treatment.decisions().to_vec(),
            fixture.treatment.holds().to_vec(),
            vec![plan.review_required.clone(), plan.review_required.clone()]
        )
        .is_err()
    );
    assert!(
        TreatmentState::restore_with_initial_reviews(
            &plan.after_lineage,
            vec![],
            vec![],
            vec![plan.review_required; crate::hub::treatment::HUB_TREATMENT_RECORDS_PER_TRACE + 1]
        )
        .is_err()
    );
}

#[test]
fn prior_recovery_requires_its_retained_child_and_birth_review_before_another_plan() {
    let fixture = Fixture::new();
    let first = fixture
        .command(vec![found(base(), 1, "found-first")])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    let target =
        DepartureQuantityAccount::restore(target(), vec![addition(), first.amendment], packs(2))
            .unwrap();
    let funding = vec![
        PhysicalDebitFunding::restore(
            base(),
            vec![first.allocations[0].credit],
            vec![reservation()],
            false,
        )
        .unwrap(),
        fixture.funding[1].clone(),
    ];
    let active = vec![identity("original-B"), identity("found-first")];
    let mut context = FoundStockContext {
        family: CorrectedDepartureFamily::Normal,
        hub: first.hub,
        stock_revision: rev(21),
        catalog: &fixture.catalog,
        latest_recorded_at: at(200),
        target: &target,
        funding: &funding,
        lineage: &first.after_lineage,
        treatment: &first.after_treatment,
        active_identifications: &active,
        uncertain_submissions: false,
    };
    let command = FoundStockCommand {
        expected_stock_revision: rev(21),
        ..fixture.command(vec![found(base(), 1, "found-second")])
    };
    let next = command.plan(&context, id(401), actor(), at(201)).unwrap();
    assert_eq!(next.after_treatment.initial_reviews().len(), 2);
    assert_eq!(next.amendment.after, packs(6));
    let mut nodes = first.after_lineage.nodes().to_vec();
    nodes
        .iter_mut()
        .find(|n| n.id == first.allocations[0].group.id)
        .unwrap()
        .recovery
        .as_mut()
        .unwrap()
        .debit_operation = id(999);
    let incorrect_origin = PhysicalLineage::new(first.hub, id(90), id(2), rev(21), nodes).unwrap();
    context.lineage = &incorrect_origin;
    assert_eq!(
        command.plan(&context, id(401), actor(), at(201)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    context.lineage = &first.after_lineage;
    assert_eq!(
        command.plan(&context, id(400), actor(), at(201)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    context.treatment = &fixture.treatment;
    assert_eq!(
        command.plan(&context, id(401), actor(), at(201)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    let missing = PhysicalLineage::new(
        first.hub,
        id(90),
        id(2),
        rev(21),
        fixture.graph.nodes().to_vec(),
    )
    .unwrap();
    context.lineage = &missing;
    assert_eq!(
        command.plan(&context, id(401), actor(), at(201)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
}
#[test]
fn all_sixty_four_allocations_have_distinct_stable_ids_and_one_fixed_review() {
    let c = catalog();
    let original = OriginalDepartureQuantity {
        packs: packs(100),
        ..target()
    };
    let account = DepartureQuantityAccount::restore(original, vec![], packs(0)).unwrap();
    let raw = OriginalPhysicalDebit {
        target: original,
        before: before(100, 100),
        packs: packs(100),
        ..base()
    };
    let funding = [PhysicalDebitFunding::restore(raw.clone(), vec![], vec![], false).unwrap()];
    let graph = PhysicalLineage::new(
        HubId::from_bytes([1; 16]).unwrap(),
        id(90),
        id(2),
        rev(20),
        vec![PhysicalLineageNode {
            recovery: None,
            id: id(100),
            parent: None,
            trace: id(90),
            variant: id(2),
            created_revision: rev(2),
            packs: packs(0),
        }],
    )
    .unwrap();
    let state = TreatmentState::default();
    let context = FoundStockContext {
        family: CorrectedDepartureFamily::Normal,
        hub: graph.hub(),
        stock_revision: rev(20),
        catalog: &c,
        latest_recorded_at: at(150),
        target: &account,
        funding: &funding,
        lineage: &graph,
        treatment: &state,
        active_identifications: &[],
        uncertain_submissions: false,
    };
    let inputs: Vec<_> = (0..HUB_FOUND_ALLOCATIONS)
        .map(|n| found(raw.clone(), 1, &format!("independent-found-{n}")))
        .collect();
    let command = FoundStockCommand {
        target_operation: original.operation,
        target_group: original.group,
        expected_stock_revision: rev(20),
        expected_catalog_revision: c.revision(),
        allocations: inputs,
        basis: note(),
    };
    let plan = command.plan(&context, id(400), actor(), at(200)).unwrap();
    assert_eq!(plan.amendment.after, packs(36));
    assert_eq!(plan.after_treatment.initial_reviews().len(), 1);
    let ids: BTreeSet<_> = plan.allocations.iter().map(|a| a.group.id).collect();
    assert_eq!(ids.len(), HUB_FOUND_ALLOCATIONS);
    for allocation in &plan.allocations {
        assert_eq!(
            allocation.group.id,
            found_stock_group_id(id(400), allocation.credit.allocation).unwrap()
        );
        assert!(allocation.group.work.is_empty());
    }
    let mut too_many = command;
    too_many.allocations.push(found(raw, 1, "one-more-found"));
    assert_eq!(
        too_many.plan(&context, id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::Capacity)
    );
    assert_eq!(
        found_stock_group_id(id(400), 64),
        Err(PhysicalCorrectionError::Capacity)
    );
}
#[test]
fn repeated_correction_cannot_consume_return_or_original_operation_identity() {
    let fixture = Fixture::new();
    let command = fixture.command(vec![found(base(), 1, "new-found")]);
    assert_eq!(
        command.plan(
            &fixture.context(),
            reservation().operation,
            actor(),
            at(200)
        ),
        Err(PhysicalCorrectionError::Duplicate)
    );
    assert_eq!(
        command.plan(&fixture.context(), target().operation, actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidHistory)
    );
    let zero = fixture.command(vec![found(base(), 0, "zero-is-not-found-stock")]);
    assert_eq!(
        zero.plan(&fixture.context(), id(400), actor(), at(200)),
        Err(PhysicalCorrectionError::InvalidQuantity)
    );
}

#[test]
fn sibling_depletion_cannot_hide_growth_under_another_recovered_birth() {
    let fixture = Fixture::new();
    let plan = fixture
        .command(vec![
            found(base(), 2, "found-A"),
            found(extra(), 2, "found-B"),
        ])
        .plan(&fixture.context(), id(400), actor(), at(200))
        .unwrap();
    let a = plan.allocations[0].group.id;
    let b = plan.allocations[1].group.id;
    let mut nodes = plan.after_lineage.nodes().to_vec();
    nodes.iter_mut().find(|n| n.id == a).unwrap().packs = packs(1);
    nodes.iter_mut().find(|n| n.id == b).unwrap().packs = packs(1);
    nodes.push(PhysicalLineageNode {
        recovery: None,
        id: id(700),
        parent: Some(a),
        trace: id(90),
        variant: id(2),
        created_revision: rev(22),
        packs: packs(2),
    });
    let expanded = PhysicalLineage::new(plan.hub, id(90), id(2), rev(22), nodes).unwrap();
    // The union still has four: A1+descendant2+B1. A's own original allowance was2.
    assert_eq!(
        expanded
            .resolve(&plan.review_required.scope)
            .unwrap()
            .packs(),
        packs(4)
    );
    assert!(
        TreatmentState::restore_with_initial_reviews(
            &expanded,
            fixture.treatment.decisions().to_vec(),
            fixture.treatment.holds().to_vec(),
            vec![plan.review_required.clone()]
        )
        .is_err()
    );
    let target =
        DepartureQuantityAccount::restore(target(), vec![addition(), plan.amendment], packs(2))
            .unwrap();
    let funding = [
        PhysicalDebitFunding::restore(
            base(),
            vec![plan.allocations[0].credit],
            vec![reservation()],
            false,
        )
        .unwrap(),
        PhysicalDebitFunding::restore(extra(), vec![plan.allocations[1].credit], vec![], false)
            .unwrap(),
    ];
    let context = FoundStockContext {
        family: CorrectedDepartureFamily::Normal,
        hub: plan.hub,
        stock_revision: rev(22),
        catalog: &fixture.catalog,
        latest_recorded_at: at(200),
        target: &target,
        funding: &funding,
        lineage: &expanded,
        treatment: &plan.after_treatment,
        active_identifications: &[],
        uncertain_submissions: false,
    };
    let command = FoundStockCommand {
        expected_stock_revision: rev(22),
        ..fixture.command(vec![found(base(), 1, "third-found")])
    };
    assert_eq!(
        command.plan(&context, id(401), actor(), at(201)),
        Err(PhysicalCorrectionError::Treatment)
    );
}
