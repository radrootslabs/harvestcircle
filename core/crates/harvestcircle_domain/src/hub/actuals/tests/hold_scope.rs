use super::*;
use crate::hub::movement::HandlingIdentities;
use crate::hub::portions::PhysicalLineageNode;
use crate::hub::reference::SampleHandlingBasis;
use crate::hub::stock::ReceiptIdentities;

struct ScopeFixture {
    graph: PhysicalLineage,
    root: HubRecordId,
    child: HubRecordId,
    hold: Hold,
    births: Vec<ActualPhysicalBirth>,
}
impl ScopeFixture {
    fn new(direct_child: bool) -> Self {
        let operation = id(220);
        let root_ids = ReceiptIdentities::from_operation_bytes(operation).unwrap();
        let root = root_ids.group;
        let child_op = id(221);
        let child = HandlingIdentities::from_operation_bytes(child_op)
            .unwrap()
            .group;
        let hub = HubId::from_bytes([222; 16]).unwrap();
        let variant = id(223);
        let revision = |n| HubRevision::from_value(n).unwrap();
        let root_node = PhysicalLineageNode {
            id: root,
            parent: None,
            trace: root_ids.trace,
            variant,
            created_revision: revision(2),
            packs: packs(20),
        };
        let before =
            PhysicalLineage::new(hub, root_ids.trace, variant, revision(2), vec![root_node])
                .unwrap();
        let nodes = vec![
            PhysicalLineageNode {
                packs: packs(14),
                ..root_node
            },
            PhysicalLineageNode {
                id: child,
                parent: Some(root),
                trace: root_ids.trace,
                variant,
                created_revision: revision(4),
                packs: packs(6),
            },
        ];
        let after = PhysicalLineage::new(hub, root_ids.trace, variant, revision(7), nodes).unwrap();
        let scope = if direct_child {
            after
                .restore_scope(
                    revision(5),
                    vec![crate::hub::portions::PhysicalScopeAnchor {
                        group: child,
                        created_revision: revision(4),
                        captured_packs: packs(6),
                    }],
                )
                .unwrap()
        } else {
            before.capture(&[root]).unwrap()
        };
        let hold = Hold {
            stamp: TreatmentStamp {
                id: id(224),
                stock_revision: revision(if direct_child { 6 } else { 3 }),
                actor: owner(),
                basis: SampleHandlingBasis::WHOLE_PACK_V1,
                recorded_at: at(155),
                occurred: known(110),
                observation: note(
                    "Original physical scope and independently recorded effective time",
                ),
            },
            scope,
            release: None,
        };
        let births = vec![
            ActualPhysicalBirth {
                kind: ActualPhysicalBirthKind::Receipt,
                operation,
                hub,
                trace: root_ids.trace,
                variant,
                group: root,
                parent: None,
                stock_revision: revision(2),
                packs: packs(20),
                actor: owner(),
                occurred: known(50),
                recorded_at: at(100),
            },
            ActualPhysicalBirth {
                kind: ActualPhysicalBirthKind::ObservedPortion,
                operation: child_op,
                hub,
                trace: root_ids.trace,
                variant,
                group: child,
                parent: Some(root),
                stock_revision: revision(4),
                packs: packs(6),
                actor: owner(),
                occurred: known(115),
                recorded_at: at(150),
            },
        ];
        Self {
            graph: after,
            root,
            child,
            hold,
            births,
        }
    }
    fn verify(&self) -> Result<HistoricalHoldScopeAssessment, ActualReportError> {
        HistoricalHoldScopeContext {
            lineage: &self.graph,
            group: self.child,
            occurred: known(120),
            births: &self.births,
            now: at(200),
        }
        .verify(&self.hold)
    }
}
fn reason(result: HistoricalHoldScopeAssessment) -> HistoricalHoldScopeUncertainty {
    let HistoricalHoldScopeAssessment::Unresolved { reason, .. } = result else {
        panic!("expected unresolved historical scope")
    };
    reason
}

#[test]
fn earlier_parent_hold_applies_to_a_later_born_descendant_that_existed_before_departure() {
    let f = ScopeFixture::new(false);
    let original = f.hold.clone();
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("verified inherited scope")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::EstablishedBreach);
    assert_eq!(proof.group(), f.child);
    assert_eq!(proof.occurred(), known(120));
    assert_eq!(proof.lineage_revision(), f.graph.revision());
    assert_eq!(proof.births(), f.births.as_slice());
    assert_eq!(proof.original(), &original);
    assert_eq!(f.hold, original);
    assert!(f.hold.release.is_none());
}

#[test]
fn direct_child_scope_cannot_be_held_before_its_original_physical_birth() {
    let f = ScopeFixture::new(true);
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime
    );
    assert_eq!(f.hold.stamp.occurred, known(110));
    assert_eq!(f.births[1].occurred, known(115));
}

#[test]
fn a_parent_already_detached_at_capture_cannot_lend_its_birth_to_an_invalid_child_anchor() {
    let mut f = ScopeFixture::new(true);
    f.hold.scope = f
        .graph
        .restore_scope(
            HubRevision::from_value(5).unwrap(),
            vec![
                crate::hub::portions::PhysicalScopeAnchor {
                    group: f.root,
                    created_revision: HubRevision::from_value(2).unwrap(),
                    captured_packs: packs(14),
                },
                crate::hub::portions::PhysicalScopeAnchor {
                    group: f.child,
                    created_revision: HubRevision::from_value(4).unwrap(),
                    captured_packs: packs(6),
                },
            ],
        )
        .unwrap();
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime
    );
}

#[test]
fn missing_unknown_or_overlapping_birth_clocks_remain_unresolved() {
    let mut f = ScopeFixture::new(false);
    f.births.remove(0);
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::MissingBirth
    );
    let mut f = ScopeFixture::new(false);
    f.births[0].occurred = Occurrence::Unknown;
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::UnknownBirthTime
    );
    let mut f = ScopeFixture::new(false);
    f.births[0].occurred = interval(50, 100);
    f.hold.stamp.occurred = known(90);
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::AnchorNotPresentAtEffectiveTime
    );
}

#[test]
fn a_group_born_after_the_reported_departure_cannot_establish_historical_scope() {
    let mut f = ScopeFixture::new(false);
    f.births[1].occurred = known(125);
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::GroupNotPresentAtDeparture
    );
}

#[test]
fn fully_depleted_original_groups_retain_historical_hold_membership() {
    let mut f = ScopeFixture::new(false);
    let nodes = f
        .graph
        .nodes()
        .iter()
        .map(|n| PhysicalLineageNode {
            packs: if n.id == f.child { packs(0) } else { n.packs },
            ..*n
        })
        .collect();
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        f.graph.revision(),
        nodes,
    )
    .unwrap();
    assert_eq!(f.graph.node(f.child).unwrap().packs, packs(0));
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("depleted historical group remains provable")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::EstablishedBreach);
    assert_eq!(proof.group(), f.child);
    assert_eq!(proof.original(), &f.hold);
}

#[test]
fn later_depletion_cannot_hide_an_enlarged_original_anchor_scope() {
    let mut f = ScopeFixture::new(false);
    let nodes = f
        .graph
        .nodes()
        .iter()
        .map(|n| PhysicalLineageNode {
            packs: if n.id == f.root { packs(4) } else { n.packs },
            ..*n
        })
        .collect();
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        f.graph.revision(),
        nodes,
    )
    .unwrap();
    f.hold.scope = f
        .graph
        .restore_scope(
            HubRevision::from_value(2).unwrap(),
            vec![crate::hub::portions::PhysicalScopeAnchor {
                group: f.root,
                created_revision: HubRevision::from_value(2).unwrap(),
                captured_packs: packs(30),
            }],
        )
        .unwrap();
    assert_eq!(f.births[0].packs, packs(20));
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::ExpandedPhysicalScope
    );
}

#[test]
fn reduced_or_previously_detached_anchor_cohorts_need_canonical_physical_cut_evidence() {
    for captured in [packs(14), packs(20)] {
        let mut f = ScopeFixture::new(false);
        let nodes = f
            .graph
            .nodes()
            .iter()
            .map(|n| PhysicalLineageNode {
                packs: if n.id == f.child { packs(0) } else { n.packs },
                ..*n
            })
            .collect();
        f.graph = PhysicalLineage::new(
            f.graph.hub(),
            f.graph.trace(),
            f.graph.variant(),
            f.graph.revision(),
            nodes,
        )
        .unwrap();
        f.hold.scope = f
            .graph
            .restore_scope(
                HubRevision::from_value(5).unwrap(),
                vec![crate::hub::portions::PhysicalScopeAnchor {
                    group: f.root,
                    created_revision: HubRevision::from_value(2).unwrap(),
                    captured_packs: captured,
                }],
            )
            .unwrap();
        f.hold.stamp.stock_revision = HubRevision::from_value(6).unwrap();
        let result = HistoricalHoldScopeContext {
            lineage: &f.graph,
            group: f.root,
            occurred: known(120),
            births: &f.births,
            now: at(200),
        }
        .verify(&f.hold)
        .unwrap();
        assert_eq!(
            reason(result),
            HistoricalHoldScopeUncertainty::AnchorCohortUnverified
        );
    }
}

#[test]
fn current_expansion_within_the_birth_maximum_still_exceeds_a_smaller_captured_scope() {
    let mut f = ScopeFixture::new(false);
    f.hold.scope = f
        .graph
        .restore_scope(
            HubRevision::from_value(5).unwrap(),
            vec![crate::hub::portions::PhysicalScopeAnchor {
                group: f.root,
                created_revision: HubRevision::from_value(2).unwrap(),
                captured_packs: packs(10),
            }],
        )
        .unwrap();
    f.hold.stamp.stock_revision = HubRevision::from_value(6).unwrap();
    let result = HistoricalHoldScopeContext {
        lineage: &f.graph,
        group: f.root,
        occurred: known(120),
        births: &f.births,
        now: at(200),
    }
    .verify(&f.hold)
    .unwrap();
    assert_eq!(
        reason(result),
        HistoricalHoldScopeUncertainty::ExpandedPhysicalScope
    );
}

#[test]
fn arrival_operation_identity_and_foreign_or_duplicate_births_fail_native_context_checks() {
    let mut f = ScopeFixture::new(false);
    f.births[0].operation = id(230);
    assert_eq!(f.verify(), Err(ActualReportError::InvalidNativeContext));
    let mut f = ScopeFixture::new(false);
    f.births[1].hub = HubId::from_bytes([231; 16]).unwrap();
    assert_eq!(f.verify(), Err(ActualReportError::InvalidNativeContext));
    let mut f = ScopeFixture::new(false);
    f.births.push(f.births[0].clone());
    assert_eq!(f.verify(), Err(ActualReportError::InvalidNativeContext));
}

#[test]
fn verified_physical_scope_does_not_remove_effective_or_release_time_uncertainty() {
    let mut f = ScopeFixture::new(true);
    f.hold.stamp.occurred = Occurrence::Unknown;
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("known physical scope")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::TimingUnknown);
    let mut f = ScopeFixture::new(false);
    f.hold.release = Some(TreatmentStamp {
        id: id(225),
        stock_revision: HubRevision::from_value(7).unwrap(),
        recorded_at: at(180),
        occurred: Occurrence::Unknown,
        ..f.hold.stamp.clone()
    });
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("known physical scope")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::TimingUnknown);
    assert!(proof.original().release.is_some());
}

#[test]
fn a_later_hold_or_earlier_release_is_distinct_from_current_scope_authority() {
    let mut f = ScopeFixture::new(true);
    f.hold.stamp.occurred = known(130);
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("verified before hold")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::BeforeHold);
    let mut f = ScopeFixture::new(false);
    f.hold.release = Some(TreatmentStamp {
        id: id(225),
        stock_revision: HubRevision::from_value(7).unwrap(),
        recorded_at: at(180),
        occurred: known(118),
        ..f.hold.stamp.clone()
    });
    let HistoricalHoldScopeAssessment::Verified(proof) = f.verify().unwrap() else {
        panic!("verified after release")
    };
    assert_eq!(proof.timing(), HistoricalHoldTiming::AfterRelease);
    assert_eq!(f.hold.release, proof.original().release);
}

#[test]
fn a_late_recorded_split_before_the_hold_does_not_gain_physical_membership_from_native_order() {
    let mut f = ScopeFixture::new(false);
    f.births[1].occurred = known(100);
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry
    );
    assert_eq!(f.hold.stamp.occurred, known(110));
    assert_eq!(
        f.births[1].stock_revision,
        HubRevision::from_value(4).unwrap()
    );
    assert_eq!(
        f.hold.scope.captured_revision(),
        HubRevision::from_value(2).unwrap()
    );
}

#[test]
fn each_intake_origin_requires_its_own_native_birth_namespace() {
    for kind in [
        ActualPhysicalBirthKind::Receipt,
        ActualPhysicalBirthKind::CountedOpening,
        ActualPhysicalBirthKind::Returned,
        ActualPhysicalBirthKind::CountedSurplus,
    ] {
        let operation = id(235);
        let (group, trace) = match kind {
            ActualPhysicalBirthKind::Receipt => {
                let ids = ReceiptIdentities::from_operation_bytes(operation).unwrap();
                (ids.group, ids.trace)
            }
            ActualPhysicalBirthKind::CountedOpening => {
                let ids =
                    crate::hub::stock::OpeningIdentities::from_operation_bytes(operation).unwrap();
                (ids.group, ids.population)
            }
            ActualPhysicalBirthKind::Returned => {
                let ids =
                    crate::hub::returns::ReturnIdentities::from_operation_bytes(operation).unwrap();
                (ids.group, ids.population)
            }
            ActualPhysicalBirthKind::CountedSurplus => {
                let ids =
                    crate::hub::counts::CountSurplusIdentities::from_operation(operation).unwrap();
                (ids.group, ids.population)
            }
            _ => unreachable!(),
        };
        let hub = HubId::from_bytes([236; 16]).unwrap();
        let variant = id(237);
        let revision = HubRevision::from_value(2).unwrap();
        let graph = PhysicalLineage::new(
            hub,
            trace,
            variant,
            revision,
            vec![PhysicalLineageNode {
                id: group,
                parent: None,
                trace,
                variant,
                created_revision: revision,
                packs: packs(20),
            }],
        )
        .unwrap();
        let birth = ActualPhysicalBirth {
            kind,
            operation,
            hub,
            trace,
            variant,
            group,
            parent: None,
            stock_revision: revision,
            packs: packs(20),
            actor: owner(),
            occurred: known(50),
            recorded_at: at(100),
        };
        birth.validate(&graph, at(200)).unwrap();
        let mut wrong = birth;
        wrong.kind = ActualPhysicalBirthKind::ObservedPortion;
        assert_eq!(
            wrong.validate(&graph, at(200)),
            Err(ActualReportError::InvalidNativeContext)
        );
    }
}

#[test]
fn complete_native_birth_inventory_accepts_the_lineage_limit_and_rejects_one_more() {
    let hub = HubId::from_bytes([240; 16]).unwrap();
    let variant = id(241);
    let operation = id(242);
    let ids = ReceiptIdentities::from_operation_bytes(operation).unwrap();
    let revision = |v| HubRevision::from_value(v).unwrap();
    let root_node = PhysicalLineageNode {
        id: ids.group,
        parent: None,
        trace: ids.trace,
        variant,
        created_revision: revision(2),
        packs: packs(256),
    };
    let initial =
        PhysicalLineage::new(hub, ids.trace, variant, revision(2), vec![root_node]).unwrap();
    let mut nodes = vec![PhysicalLineageNode {
        packs: packs(1),
        ..root_node
    }];
    let mut births = vec![ActualPhysicalBirth {
        kind: ActualPhysicalBirthKind::Receipt,
        operation,
        hub,
        trace: ids.trace,
        variant,
        group: ids.group,
        parent: None,
        stock_revision: revision(2),
        packs: packs(256),
        actor: owner(),
        occurred: known(1),
        recorded_at: at(2),
    }];
    for n in 1_u16..=255 {
        let mut bytes = [0; 16];
        bytes[14..].copy_from_slice(&n.to_be_bytes());
        let operation = HubRecordId::from_bytes(bytes).unwrap();
        let group = HandlingIdentities::from_operation_bytes(operation)
            .unwrap()
            .group;
        let stock_revision = revision(u64::from(n) + 4);
        nodes.push(PhysicalLineageNode {
            id: group,
            parent: Some(ids.group),
            trace: ids.trace,
            variant,
            created_revision: stock_revision,
            packs: packs(1),
        });
        births.push(ActualPhysicalBirth {
            kind: ActualPhysicalBirthKind::ObservedPortion,
            operation,
            hub,
            trace: ids.trace,
            variant,
            group,
            parent: Some(ids.group),
            stock_revision,
            packs: packs(1),
            actor: owner(),
            occurred: known(i64::from(n) + 20),
            recorded_at: at(400),
        });
    }
    let group = births.last().unwrap().group;
    let graph = PhysicalLineage::new(hub, ids.trace, variant, revision(260), nodes).unwrap();
    let hold = Hold {
        stamp: TreatmentStamp {
            id: id(243),
            stock_revision: revision(3),
            actor: owner(),
            basis: SampleHandlingBasis::WHOLE_PACK_V1,
            recorded_at: at(10),
            occurred: known(5),
            observation: note("Whole original parent scope before native descendant births"),
        },
        scope: initial.capture(&[ids.group]).unwrap(),
        release: None,
    };
    assert_eq!(births.len(), crate::hub::portions::HUB_LINEAGE_GROUPS);
    let result = HistoricalHoldScopeContext {
        lineage: &graph,
        group,
        occurred: known(300),
        births: &births,
        now: at(500),
    }
    .verify(&hold)
    .unwrap();
    let HistoricalHoldScopeAssessment::Verified(proof) = result else {
        panic!("complete bounded native proof")
    };
    assert_eq!(proof.births().len(), 256);
    assert_eq!(proof.timing(), HistoricalHoldTiming::EstablishedBreach);
    births.push(births[0].clone());
    assert_eq!(
        HistoricalHoldScopeContext {
            lineage: &graph,
            group,
            occurred: known(300),
            births: &births,
            now: at(500)
        }
        .verify(&hold),
        Err(ActualReportError::Capacity)
    );
}

fn sibling_fixture(
    root_packs: WholePacks,
    original: WholePacks,
    current: WholePacks,
    occurred: Occurrence,
) -> (ScopeFixture, HubRecordId) {
    let mut f = ScopeFixture::new(false);
    let operation = id(226);
    let sibling = HandlingIdentities::from_operation_bytes(operation)
        .unwrap()
        .group;
    let nodes = f
        .graph
        .nodes()
        .iter()
        .map(|n| PhysicalLineageNode {
            packs: if n.id == f.root { root_packs } else { packs(2) },
            created_revision: if n.id == f.child {
                HubRevision::from_value(5).unwrap()
            } else {
                n.created_revision
            },
            ..*n
        })
        .chain(std::iter::once(PhysicalLineageNode {
            id: sibling,
            parent: Some(f.root),
            trace: f.graph.trace(),
            variant: f.graph.variant(),
            created_revision: HubRevision::from_value(4).unwrap(),
            packs: current,
        }))
        .collect();
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        f.graph.revision(),
        nodes,
    )
    .unwrap();
    f.births[1].stock_revision = HubRevision::from_value(5).unwrap();
    f.births[1].packs = packs(2);
    f.births[1].recorded_at = at(170);
    f.births.push(ActualPhysicalBirth {
        kind: ActualPhysicalBirthKind::ObservedPortion,
        operation,
        hub: f.graph.hub(),
        trace: f.graph.trace(),
        variant: f.graph.variant(),
        group: sibling,
        parent: Some(f.root),
        stock_revision: HubRevision::from_value(4).unwrap(),
        packs: original,
        actor: owner(),
        occurred,
        recorded_at: at(160),
    });
    (f, sibling)
}

#[test]
fn a_late_recorded_sibling_detachment_is_not_hidden_by_the_selected_branch_or_zero_balance() {
    for current in [packs(6), packs(0)] {
        let (mut f, _) = sibling_fixture(packs(12), packs(6), current, known(100));
        assert_eq!(
            reason(f.verify().unwrap()),
            HistoricalHoldScopeUncertainty::AnchorCohortUnverified
        );
        f.births.pop();
        assert_eq!(
            reason(f.verify().unwrap()),
            HistoricalHoldScopeUncertainty::MissingBirth
        );
    }
}

#[test]
fn a_sibling_subtree_cannot_expand_its_birth_while_the_whole_parent_total_remains_balanced() {
    let (f, _) = sibling_fixture(packs(13), packs(4), packs(5), known(115));
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::ExpandedPhysicalScope
    );
}

#[test]
fn contradictory_birth_clocks_on_a_nonselected_branch_do_not_become_verified_cohort_evidence() {
    let (mut f, sibling) = sibling_fixture(packs(12), packs(6), packs(5), known(130));
    let operation = id(227);
    let group = HandlingIdentities::from_operation_bytes(operation)
        .unwrap()
        .group;
    let mut nodes = f.graph.nodes().to_vec();
    nodes.push(PhysicalLineageNode {
        id: group,
        parent: Some(sibling),
        trace: f.graph.trace(),
        variant: f.graph.variant(),
        created_revision: HubRevision::from_value(6).unwrap(),
        packs: packs(1),
    });
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        f.graph.revision(),
        nodes,
    )
    .unwrap();
    f.births.push(ActualPhysicalBirth {
        kind: ActualPhysicalBirthKind::ObservedPortion,
        operation,
        hub: f.graph.hub(),
        trace: f.graph.trace(),
        variant: f.graph.variant(),
        group,
        parent: Some(sibling),
        stock_revision: HubRevision::from_value(6).unwrap(),
        packs: packs(1),
        actor: owner(),
        occurred: known(120),
        recorded_at: at(180),
    });
    assert_eq!(
        reason(f.verify().unwrap()),
        HistoricalHoldScopeUncertainty::UncertainPhysicalAncestry
    );
}
