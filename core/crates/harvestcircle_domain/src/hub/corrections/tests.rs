use super::*;
fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn original() -> OriginalDepartureQuantity {
    OriginalDepartureQuantity {
        operation: id(1),
        group: id(2),
        trace: id(3),
        variant: id(4),
        stock_revision: rev(10),
        packs: packs(6),
    }
}
fn increase() -> DepartureQuantityAmendment {
    DepartureQuantityAmendment {
        operation: id(5),
        stock_revision: rev(11),
        before: packs(6),
        after: packs(8),
        delta: DepartureQuantityDelta::AdditionalDeparture(packs(2)),
    }
}
fn decrease() -> DepartureQuantityAmendment {
    DepartureQuantityAmendment {
        operation: id(6),
        stock_revision: rev(12),
        before: packs(8),
        after: packs(5),
        delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(3)),
    }
}
fn loss_original() -> OriginalCountLossQuantity {
    OriginalCountLossQuantity {
        reconciliation: id(20),
        operation: id(19),
        stock_revision: rev(30),
        trace: id(3),
        variant: id(4),
        packs: packs(4),
    }
}
fn explanation() -> CountLossExplanation {
    CountLossExplanation {
        operation: id(21),
        stock_revision: rev(31),
        packs: packs(2),
    }
}
fn classification_change(after: u32) -> CountExplanationAmendment {
    CountExplanationAmendment {
        operation: id(23),
        stock_revision: rev(32),
        explanation: id(21),
        before: packs(2),
        after: packs(after),
    }
}
#[test]
fn corrected_classification_returns_overstatement_to_unexplained_loss_without_stock() {
    let account = CountLossQuantityAccount::restore_corrected(
        loss_original(),
        vec![explanation()],
        vec![classification_change(1)],
        vec![],
    )
    .unwrap();
    assert_eq!(account.explanations(), &[explanation()]);
    assert_eq!(account.effective_explanations()[0].packs, packs(1));
    assert_eq!(account.explained(), packs(1));
    assert_eq!(account.unexplained(), packs(3));
    let trace = context(&[], &[], std::slice::from_ref(&account), 16)
        .balance()
        .unwrap();
    assert_eq!(trace.on_hand, packs(16));
    assert_eq!(trace.actual_departed, packs(1));
    assert_eq!(trace.counted_loss, packs(3));
}
#[test]
fn zero_effective_classification_keeps_the_original_known_event_visible() {
    let account = CountLossQuantityAccount::restore_corrected(
        loss_original(),
        vec![explanation()],
        vec![classification_change(0)],
        vec![],
    )
    .unwrap();
    assert_eq!(account.explanations()[0].packs, packs(2));
    assert_eq!(account.effective_explanations()[0].original, explanation());
    assert_eq!(account.effective_explanations()[0].packs, packs(0));
    assert_eq!(account.unexplained(), packs(4));
    assert_eq!(
        context(&[], &[], &[account], 16)
            .balance()
            .unwrap()
            .actual_departed,
        packs(0)
    );
}
#[test]
fn classification_and_new_explanations_interleave_in_recorded_order_and_share_allowance() {
    let next = CountLossExplanation {
        operation: id(24),
        stock_revision: rev(33),
        packs: packs(3),
    };
    let good = CountLossQuantityAccount::restore_corrected(
        loss_original(),
        vec![explanation(), next],
        vec![classification_change(1)],
        vec![],
    )
    .unwrap();
    assert_eq!(good.explained(), packs(4));
    assert_eq!(good.unexplained(), packs(0));
    let later = CountExplanationAmendment {
        operation: id(25),
        stock_revision: rev(34),
        before: packs(1),
        after: packs(2),
        ..classification_change(1)
    };
    assert_eq!(
        CountLossQuantityAccount::restore_corrected(
            loss_original(),
            vec![explanation(), next],
            vec![classification_change(1), later],
            vec![]
        ),
        Err(CorrectionError::InvalidQuantity)
    );
    assert_eq!(
        CountLossQuantityAccount::restore_corrected(
            loss_original(),
            vec![explanation(), next],
            vec![CountExplanationAmendment {
                stock_revision: rev(34),
                ..classification_change(1)
            }],
            vec![]
        ),
        Err(CorrectionError::InvalidQuantity)
    );
}
#[test]
fn classification_current_returns_bind_the_effective_event_not_just_its_original() {
    let returns = CountExplanationReturns {
        explanation: id(21),
        packs: packs(3),
    };
    let good = CountLossQuantityAccount::restore_corrected(
        loss_original(),
        vec![explanation()],
        vec![classification_change(3)],
        vec![returns],
    )
    .unwrap();
    assert_eq!(
        good.effective_explanations()[0].established_returns,
        packs(3)
    );
    assert_eq!(
        CountLossQuantityAccount::restore_corrected(
            loss_original(),
            vec![explanation()],
            vec![classification_change(1)],
            vec![returns]
        ),
        Err(CorrectionError::ReturnConflict)
    );
    assert_eq!(
        CountLossQuantityAccount::restore_corrected(
            loss_original(),
            vec![explanation()],
            vec![classification_change(3)],
            vec![returns, returns]
        ),
        Err(CorrectionError::Duplicate)
    );
}
#[test]
fn damaged_classification_heads_and_event_roles_fail_closed() {
    for bad in [
        CountExplanationAmendment {
            before: packs(1),
            ..classification_change(1)
        },
        CountExplanationAmendment {
            explanation: id(99),
            ..classification_change(1)
        },
        CountExplanationAmendment {
            operation: id(21),
            ..classification_change(1)
        },
        CountExplanationAmendment {
            stock_revision: rev(31),
            ..classification_change(1)
        },
        classification_change(2),
    ] {
        assert!(
            CountLossQuantityAccount::restore_corrected(
                loss_original(),
                vec![explanation()],
                vec![bad],
                vec![]
            )
            .is_err()
        );
    }
    let loss = CountLossQuantityAccount::restore_corrected(
        loss_original(),
        vec![explanation()],
        vec![CountExplanationAmendment {
            operation: original().operation,
            ..classification_change(1)
        }],
        vec![],
    )
    .unwrap();
    let departed = DepartureQuantityAccount::restore(original(), vec![], packs(0)).unwrap();
    assert!(context(&[departed], &[], &[loss], 10).balance().is_err());
}
fn context<'a>(
    normal: &'a [DepartureQuantityAccount],
    actual: &'a [DepartureQuantityAccount],
    loss: &'a [CountLossQuantityAccount],
    on_hand: u32,
) -> CorrectedTraceQuantityContext<'a> {
    CorrectedTraceQuantityContext {
        trace: id(3),
        variant: id(4),
        original: packs(20),
        on_hand: packs(on_hand),
        disposed: packs(0),
        departures: normal,
        actual_departures: actual,
        count_losses: loss,
    }
}
#[test]
fn mixed_deltas_preserve_original_and_bound_current_returns_without_undoing_history() {
    let original = original();
    let changes = vec![increase(), decrease()];
    let account = DepartureQuantityAccount::restore(original, changes.clone(), packs(5)).unwrap();
    assert_eq!(account.original(), original);
    assert_eq!(account.amendments(), changes);
    assert_eq!(account.effective(), packs(5));
    assert_eq!(account.additional_departure(), packs(2));
    assert_eq!(account.found_remainder(), packs(3));
    assert_eq!(account.established_returns(), packs(5));
    assert_eq!(
        DepartureQuantityAccount::restore(original, changes, packs(6)),
        Err(CorrectionError::ReturnConflict)
    );
    // Eight returned packs can follow an amended eight-pack departure even though
    // the frozen original was six. The current bound is not the original count.
    assert!(DepartureQuantityAccount::restore(original, vec![increase()], packs(8)).is_ok());
}
#[test]
fn quantity_history_rejects_stale_reordered_duplicate_and_forged_results() {
    for bad in [
        DepartureQuantityAmendment {
            before: packs(5),
            ..increase()
        },
        DepartureQuantityAmendment {
            after: packs(7),
            ..increase()
        },
        DepartureQuantityAmendment {
            stock_revision: rev(10),
            ..increase()
        },
    ] {
        assert_eq!(
            DepartureQuantityAccount::restore(original(), vec![bad], packs(0)),
            Err(CorrectionError::InvalidHistory)
        );
    }
    assert_eq!(
        DepartureQuantityAccount::restore(original(), vec![decrease(), increase()], packs(0)),
        Err(CorrectionError::InvalidHistory)
    );
    let bad = DepartureQuantityAmendment {
        operation: original().operation,
        ..increase()
    };
    assert_eq!(
        DepartureQuantityAccount::restore(original(), vec![bad], packs(0)),
        Err(CorrectionError::Duplicate)
    );
    let bad = DepartureQuantityAmendment {
        operation: increase().operation,
        ..decrease()
    };
    assert_eq!(
        DepartureQuantityAccount::restore(original(), vec![increase(), bad], packs(0)),
        Err(CorrectionError::Duplicate)
    );
}
#[test]
fn no_zero_overflow_or_unfunded_remainder_is_a_valid_delta() {
    for delta in [
        DepartureQuantityDelta::AdditionalDeparture(packs(0)),
        DepartureQuantityDelta::IdentifiedFoundRemainder(packs(0)),
        DepartureQuantityDelta::AdditionalDeparture(packs(1_000_000)),
        DepartureQuantityDelta::IdentifiedFoundRemainder(packs(7)),
    ] {
        let change = DepartureQuantityAmendment {
            delta,
            ..increase()
        };
        assert_eq!(
            DepartureQuantityAccount::restore(original(), vec![change], packs(0)),
            Err(CorrectionError::InvalidQuantity)
        );
    }
    assert_eq!(
        DepartureQuantityAccount::restore(
            OriginalDepartureQuantity {
                packs: packs(0),
                ..original()
            },
            vec![],
            packs(0)
        ),
        Err(CorrectionError::InvalidHistory)
    );
    assert_eq!(
        DepartureQuantityAccount::restore(
            OriginalDepartureQuantity {
                stock_revision: rev(1),
                ..original()
            },
            vec![],
            packs(0)
        ),
        Err(CorrectionError::InvalidHistory)
    );
    assert_eq!(
        DepartureQuantityAccount::restore(
            original(),
            vec![increase(); HUB_CORRECTION_ROWS + 1],
            packs(0)
        ),
        Err(CorrectionError::Capacity)
    );
}
#[test]
fn complete_overstatement_can_reach_zero_only_without_established_returns() {
    let change = DepartureQuantityAmendment {
        before: packs(6),
        after: packs(0),
        delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(6)),
        ..decrease()
    };
    assert_eq!(
        DepartureQuantityAccount::restore(original(), vec![change], packs(0))
            .unwrap()
            .effective(),
        packs(0)
    );
    assert_eq!(
        DepartureQuantityAccount::restore(original(), vec![change], packs(1)),
        Err(CorrectionError::ReturnConflict)
    );
}
#[test]
fn partial_loss_explanation_leaves_visible_unexplained_balance_and_no_new_debit() {
    let account = CountLossQuantityAccount::restore(loss_original(), vec![explanation()]).unwrap();
    assert_eq!(account.original(), loss_original());
    assert_eq!(account.explanations(), &[explanation()]);
    assert_eq!(account.explained(), packs(2));
    assert_eq!(account.unexplained(), packs(2));
    let before = [CountLossQuantityAccount::restore(loss_original(), vec![]).unwrap()];
    let after = [account];
    let old = context(&[], &[], &before, 16).balance().unwrap();
    let new = context(&[], &[], &after, 16).balance().unwrap();
    assert_eq!(new.on_hand, old.on_hand);
    assert_eq!(new.original, old.original);
    assert_eq!(new.actual_departed, packs(2));
    assert_eq!(new.counted_loss, packs(2));
    assert_eq!(
        new.actual_departed.checked_add(new.counted_loss).unwrap(),
        old.counted_loss
    );
}
#[test]
fn loss_explanation_rejects_overclaim_zero_duplicate_and_capacity() {
    let one = explanation();
    let different = CountLossExplanation {
        operation: id(23),
        stock_revision: rev(32),
        packs: packs(3),
    };
    assert_eq!(
        CountLossQuantityAccount::restore(loss_original(), vec![one, different]),
        Err(CorrectionError::InvalidQuantity)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(
            loss_original(),
            vec![CountLossExplanation {
                packs: packs(0),
                ..one
            }]
        ),
        Err(CorrectionError::InvalidQuantity)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(
            loss_original(),
            vec![
                one,
                CountLossExplanation {
                    operation: id(23),
                    ..one
                }
            ]
        ),
        Err(CorrectionError::InvalidHistory)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(
            loss_original(),
            vec![
                one,
                CountLossExplanation {
                    stock_revision: rev(32),
                    ..one
                }
            ]
        ),
        Err(CorrectionError::Duplicate)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(loss_original(), vec![one; HUB_CORRECTION_ROWS + 1]),
        Err(CorrectionError::Capacity)
    );
}
#[test]
fn trace_requires_balanced_effects_instead_of_minting_stock_from_arithmetic() {
    let accounts =
        [DepartureQuantityAccount::restore(original(), vec![increase()], packs(0)).unwrap()];
    assert_eq!(
        context(&accounts, &[], &[], 12).balance().unwrap().departed,
        packs(8)
    );
    assert_eq!(
        context(&accounts, &[], &[], 14).balance(),
        Err(CorrectionError::Accounting)
    );
    let accounts =
        [
            DepartureQuantityAccount::restore(original(), vec![increase(), decrease()], packs(0))
                .unwrap(),
        ];
    assert_eq!(
        context(&accounts, &[], &[], 15).balance().unwrap().departed,
        packs(5)
    );
    assert_eq!(
        context(&accounts, &[], &[], 12).balance(),
        Err(CorrectionError::Accounting)
    );
}
#[test]
fn mixed_actual_normal_loss_and_disposition_have_one_physical_equation() {
    let normal =
        [
            DepartureQuantityAccount::restore(original(), vec![increase(), decrease()], packs(0))
                .unwrap(),
        ];
    let actual = [DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: id(30),
            stock_revision: rev(20),
            group: id(31),
            packs: packs(3),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap()];
    let loss = [CountLossQuantityAccount::restore(loss_original(), vec![explanation()]).unwrap()];
    let mut c = context(&normal, &actual, &loss, 6);
    c.disposed = packs(2);
    let result = c.balance().unwrap();
    assert_eq!(result.departed, packs(5));
    assert_eq!(result.actual_departed, packs(5));
    assert_eq!(result.counted_loss, packs(2));
    c.on_hand = packs(5);
    assert_eq!(c.balance(), Err(CorrectionError::Accounting));
}
#[test]
fn a_known_event_cannot_be_debited_again_or_explain_two_losses() {
    let explanation = CountLossExplanation {
        operation: original().operation,
        ..explanation()
    };
    let normal = [DepartureQuantityAccount::restore(original(), vec![], packs(0)).unwrap()];
    let loss = [CountLossQuantityAccount::restore(loss_original(), vec![explanation]).unwrap()];
    assert_eq!(
        context(&normal, &[], &loss, 10).balance(),
        Err(CorrectionError::Duplicate)
    );
    assert_eq!(
        context(&[], &normal, &loss, 10).balance(),
        Err(CorrectionError::Duplicate)
    );
    let losses = [
        CountLossQuantityAccount::restore(loss_original(), vec![explanation]).unwrap(),
        CountLossQuantityAccount::restore(
            OriginalCountLossQuantity {
                reconciliation: id(25),
                operation: id(24),
                stock_revision: rev(29),
                ..loss_original()
            },
            vec![CountLossExplanation { ..explanation }],
        )
        .unwrap(),
    ];
    assert_eq!(
        context(&[], &[], &losses, 12).balance(),
        Err(CorrectionError::Duplicate)
    );
}
#[test]
fn trace_rejects_cross_population_variant_duplicate_lines_and_family_reuse() {
    let normal = DepartureQuantityAccount::restore(original(), vec![], packs(0)).unwrap();
    for bad in [
        OriginalDepartureQuantity {
            trace: id(99),
            ..original()
        },
        OriginalDepartureQuantity {
            variant: id(99),
            ..original()
        },
    ] {
        let rows = [DepartureQuantityAccount::restore(bad, vec![], packs(0)).unwrap()];
        assert_eq!(
            context(&rows, &[], &[], 14).balance(),
            Err(CorrectionError::InvalidHistory)
        );
    }
    let rows = [normal.clone(), normal.clone()];
    assert_eq!(
        context(&rows, &[], &[], 8).balance(),
        Err(CorrectionError::Duplicate)
    );
    let other_group = [DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            group: id(99),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap()];
    assert_eq!(
        context(&[normal], &other_group, &[], 8).balance(),
        Err(CorrectionError::Duplicate)
    );
    let losses = [CountLossQuantityAccount::restore(loss_original(), vec![]).unwrap(); 1];
    let mut c = context(&[], &[], &losses, 16);
    c.variant = id(99);
    assert_eq!(c.balance(), Err(CorrectionError::InvalidHistory));
}
#[test]
fn no_count_or_amendment_operation_can_hide_a_second_original_effect() {
    let normal = DepartureQuantityAccount::restore(original(), vec![increase()], packs(0)).unwrap();
    let other = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: increase().operation,
            stock_revision: rev(20),
            group: id(40),
            packs: packs(2),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[normal.clone(), other], &[], &[], 10).balance(),
        Err(CorrectionError::Duplicate)
    );
    let loss = [CountLossQuantityAccount::restore(
        loss_original(),
        vec![CountLossExplanation {
            operation: increase().operation,
            ..explanation()
        }],
    )
    .unwrap()];
    assert_eq!(
        context(&[normal], &[], &loss, 8).balance(),
        Err(CorrectionError::Duplicate)
    );
}
#[test]
fn aggregate_bounds_and_overflow_fail_closed() {
    let account = DepartureQuantityAccount::restore(original(), vec![], packs(0)).unwrap();
    let many = vec![account; HUB_CORRECTION_ROWS + 1];
    assert_eq!(
        context(&many, &[], &[], 0).balance(),
        Err(CorrectionError::Capacity)
    );
    let mut c = context(&[], &[], &[], 1_000_000);
    c.disposed = packs(1);
    assert_eq!(c.balance(), Err(CorrectionError::InvalidQuantity));
}

#[test]
fn complete_existing_actual_and_count_capacity_is_not_truncated_by_correction_capacity() {
    fn distinct(n: u64) -> HubRecordId {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&n.to_be_bytes());
        HubRecordId::from_bytes(bytes).unwrap()
    }
    let actual: Vec<_> = (0..super::super::actuals::HUB_ACTUAL_REPORT_CAPACITY)
        .map(|n| {
            DepartureQuantityAccount::restore(
                OriginalDepartureQuantity {
                    operation: distinct(10_000 + u64::try_from(n).unwrap()),
                    stock_revision: rev(100 + u64::try_from(n).unwrap()),
                    packs: packs(1),
                    ..original()
                },
                vec![],
                packs(0),
            )
            .unwrap()
        })
        .collect();
    let losses = [
        CountLossQuantityAccount::restore(
            OriginalCountLossQuantity {
                packs: packs(1),
                ..loss_original()
            },
            vec![],
        )
        .unwrap(),
        CountLossQuantityAccount::restore(
            OriginalCountLossQuantity {
                reconciliation: id(25),
                operation: id(24),
                stock_revision: rev(29),
                packs: packs(1),
                ..loss_original()
            },
            vec![],
        )
        .unwrap(),
    ];
    let mut c = context(&[], &actual, &losses, 0);
    c.original = packs(4_098);
    let b = c.balance().unwrap();
    assert_eq!(b.actual_departed, packs(4_096));
    assert_eq!(b.counted_loss, packs(2));
}
#[test]
fn aggregate_correction_capacity_counts_all_amendments_and_explanations() {
    fn distinct(n: u64) -> HubRecordId {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&n.to_be_bytes());
        HubRecordId::from_bytes(bytes).unwrap()
    }
    let changes: Vec<_> = (0..HUB_CORRECTION_ROWS)
        .map(|n| {
            let n = u64::try_from(n).unwrap();
            DepartureQuantityAmendment {
                operation: distinct(10_000 + n),
                stock_revision: rev(11 + n),
                before: packs(if n % 2 == 0 { 6 } else { 7 }),
                after: packs(if n % 2 == 0 { 7 } else { 6 }),
                delta: if n % 2 == 0 {
                    DepartureQuantityDelta::AdditionalDeparture(packs(1))
                } else {
                    DepartureQuantityDelta::IdentifiedFoundRemainder(packs(1))
                },
            }
        })
        .collect();
    let normal = [DepartureQuantityAccount::restore(original(), changes, packs(0)).unwrap()];
    let losses = [CountLossQuantityAccount::restore(loss_original(), vec![explanation()]).unwrap()];
    assert_eq!(
        context(&normal, &[], &losses, 10).balance(),
        Err(CorrectionError::Capacity)
    );
    assert!(context(&normal, &[], &[], 14).balance().is_ok());
}

#[test]
fn a_departure_operation_has_one_original_revision_across_all_lines() {
    let first = DepartureQuantityAccount::restore(original(), vec![], packs(0)).unwrap();
    let second = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            group: id(90),
            stock_revision: rev(11),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[first.clone(), second], &[], &[], 8).balance(),
        Err(CorrectionError::Duplicate)
    );
    let second = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            group: id(90),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[first, second], &[], &[], 8)
            .balance()
            .unwrap()
            .departed,
        packs(12)
    );
}
#[test]
fn original_count_operation_cannot_be_reused_in_any_other_event_role() {
    let count = CountLossQuantityAccount::restore(loss_original(), vec![]).unwrap();
    let out = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: loss_original().operation,
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[out], &[], std::slice::from_ref(&count), 10).balance(),
        Err(CorrectionError::Duplicate)
    );
    let amended = DepartureQuantityAccount::restore(
        original(),
        vec![DepartureQuantityAmendment {
            operation: loss_original().operation,
            ..increase()
        }],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[amended], &[], std::slice::from_ref(&count), 8).balance(),
        Err(CorrectionError::Duplicate)
    );
    let other = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            operation: id(24),
            stock_revision: rev(29),
            reconciliation: id(25),
            ..loss_original()
        },
        vec![CountLossExplanation {
            operation: loss_original().operation,
            ..explanation()
        }],
    )
    .unwrap();
    assert_eq!(
        context(&[], &[], &[count.clone(), other.clone()], 12).balance(),
        Err(CorrectionError::Duplicate)
    );
    assert_eq!(
        context(&[], &[], &[other, count], 12).balance(),
        Err(CorrectionError::Duplicate)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(
            loss_original(),
            vec![CountLossExplanation {
                operation: loss_original().operation,
                ..explanation()
            }]
        ),
        Err(CorrectionError::Duplicate)
    );
}
#[test]
fn count_explanations_follow_original_and_each_other_in_native_record_order() {
    assert_eq!(
        CountLossQuantityAccount::restore(
            loss_original(),
            vec![CountLossExplanation {
                stock_revision: rev(10),
                ..explanation()
            }]
        ),
        Err(CorrectionError::InvalidHistory)
    );
    assert_eq!(
        CountLossQuantityAccount::restore(
            OriginalCountLossQuantity {
                stock_revision: rev(1),
                ..loss_original()
            },
            vec![]
        ),
        Err(CorrectionError::InvalidHistory)
    );
    let next = CountLossExplanation {
        operation: id(23),
        stock_revision: rev(32),
        ..explanation()
    };
    let a = CountLossQuantityAccount::restore(loss_original(), vec![explanation(), next]).unwrap();
    assert_eq!(a.unexplained(), packs(0));
    assert_eq!(
        CountLossQuantityAccount::restore(loss_original(), vec![next, explanation()]),
        Err(CorrectionError::InvalidHistory)
    );
}

#[test]
fn one_native_revision_cannot_name_two_original_or_correction_operations() {
    let first = DepartureQuantityAccount::restore(original(), vec![increase()], packs(0)).unwrap();
    let second = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: id(80),
            group: id(81),
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[first.clone(), second], &[], &[], 6).balance(),
        Err(CorrectionError::InvalidHistory)
    );
    let count = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            stock_revision: increase().stock_revision,
            ..loss_original()
        },
        vec![],
    )
    .unwrap();
    assert_eq!(
        context(&[first], &[], &[count], 8).balance(),
        Err(CorrectionError::InvalidHistory)
    );
    let count = CountLossQuantityAccount::restore(loss_original(), vec![explanation()]).unwrap();
    let out = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            stock_revision: explanation().stock_revision,
            ..original()
        },
        vec![],
        packs(0),
    )
    .unwrap();
    assert_eq!(
        context(&[out], &[], &[count], 10).balance(),
        Err(CorrectionError::InvalidHistory)
    );
}
