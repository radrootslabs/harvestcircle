use super::*;

fn capture(f: &Fixture, effective: u32, physical: u32) -> ReturnAllowanceCapture {
    ReturnAllowanceCapture {
        hub: f.departure.hub,
        departure: f.departure.id,
        operation: f.departure.operation,
        stock_revision: f.command.expected_stock_revision,
        lines: f
            .departure
            .paper
            .versions
            .last()
            .unwrap()
            .material
            .lines
            .iter()
            .enumerate()
            .map(|(n, line)| ReturnLineAllowance {
                original_group: line.group,
                effective_departure: packs(if n == 0 {
                    effective
                } else {
                    line.packs.value()
                }),
                original_debit_return_bound: packs(if n == 0 {
                    physical
                } else {
                    line.packs.value()
                }),
                quantity_head: Some(ReturnQuantityHead {
                    operation: id(110 + u8::try_from(n).unwrap()),
                    stock_revision: f.command.expected_stock_revision,
                    projection_hash: [7; 32],
                }),
                funding_hash: [8; 32],
                unresolved_returns: false,
            })
            .collect(),
    }
}
fn fixture(lines: &[u32]) -> Fixture {
    let mut f = Fixture::new(lines);
    f.command.expected_stock_revision =
        HubRevision::from_value(f.departure.stock_revision.value() + 1).unwrap();
    f
}
fn plan(f: &Fixture, allowance: &ReturnAllowanceCapture) -> Result<ReturnPlan, ReturnError> {
    f.command.plan(
        &ReturnContext {
            current_allowances: Some(allowance),
            ..f.context()
        },
        f.ids,
        owner(),
        at(150),
    )
}

#[test]
fn effective_four_after_original_six_keeps_new_return_two_unresolved_after_prior_three() {
    let mut f = fixture(&[6]);
    f.totals[0].packs = packs(3);
    let original = f.departure.clone();
    let allowance = capture(&f, 4, 4);
    assert_eq!(
        f.plan().linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    let result = plan(&f, &allowance).unwrap();
    assert_eq!(
        result.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    assert_eq!(result.intake.packs, packs(2));
    assert_eq!(result.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(
        result
            .linkage
            .claimed_source
            .as_ref()
            .unwrap()
            .original
            .packs,
        packs(6)
    );
    assert_eq!(result.current_allowances, Some(allowance));
    assert_eq!(f.departure, original);
}

#[test]
fn additional_b_stock_never_expands_the_original_a_physical_return_bound() {
    let mut f = fixture(&[6]);
    f.totals[0].packs = packs(2);
    let allowance = capture(&f, 5, 3);
    assert_eq!(allowance.lines[0].linkable_quantity(), packs(3));
    assert_eq!(
        plan(&f, &allowance).unwrap().linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    let growth = capture(&f, 8, 6);
    assert_eq!(growth.lines[0].linkable_quantity(), packs(6));
    assert_eq!(
        plan(&f, &growth).unwrap().linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
}

#[test]
fn unresolved_prior_returns_preserve_custody_without_inventing_a_deduction() {
    let f = fixture(&[6]);
    let mut allowance = capture(&f, 6, 6);
    allowance.lines[0].unresolved_returns = true;
    let result = plan(&f, &allowance).unwrap();
    assert_eq!(result.intake.packs, packs(2));
    assert_eq!(result.group.packs, packs(2));
    assert_eq!(allowance.lines[0].linkable_quantity(), packs(6));
    assert_eq!(
        result.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    assert_eq!(f.totals[0].packs, packs(0));
}

#[test]
fn zero_current_event_or_original_debit_scope_does_not_discard_new_returned_custody() {
    let f = fixture(&[6]);
    for effective in [0, 2] {
        let result = plan(&f, &capture(&f, effective, 0)).unwrap();
        assert_eq!(result.intake.packs, packs(2));
        assert_eq!(result.group.revision, HubRevision::INITIAL);
        assert_eq!(
            result.linkage.resolution,
            ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
        );
    }
}

#[test]
fn allowance_capture_requires_complete_original_scope_and_exact_native_cut() {
    let f = fixture(&[6, 4]);
    let original = capture(&f, 6, 6);
    for fault in 0..10 {
        let mut value = original.clone();
        match fault {
            0 => {
                value.lines.pop();
            }
            1 => value.lines[1] = value.lines[0].clone(),
            2 => value.stock_revision = f.departure.stock_revision,
            3 => value.lines[0].original_debit_return_bound = packs(7),
            4 => value.departure = id(200),
            5 => value.lines[0].funding_hash = [0; 32],
            6 => {
                value.lines[0]
                    .quantity_head
                    .as_mut()
                    .unwrap()
                    .stock_revision =
                    HubRevision::from_value(value.stock_revision.value() + 1).unwrap()
            }
            7 => value.lines.clear(),
            8 => {
                value.lines[0].quantity_head = None;
                value.lines[0].effective_departure = packs(5);
            }
            9 => {
                value.lines[0].quantity_head = None;
                value.lines[0].original_debit_return_bound = packs(5);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            plan(&f, &value),
            Err(ReturnError::InvalidNativeContext),
            "fault {fault}"
        );
    }
}

#[test]
fn allowance_seal_binds_current_bound_head_and_funding_even_when_linkage_is_unchanged() {
    let f = fixture(&[6]);
    let original = capture(&f, 8, 6);
    let digest = |value: &ReturnAllowanceCapture| {
        let context = ReturnContext {
            current_allowances: Some(value),
            ..f.context()
        };
        let result = f.command.plan(&context, f.ids, owner(), at(150)).unwrap();
        return_result_digest(
            &f.command.fingerprint(context.hub, owner(), f.ids),
            &f.command,
            &context,
            &result,
        )
        .unwrap()
    };
    let first = digest(&original);
    for fault in 0..4 {
        let mut value = original.clone();
        match fault {
            0 => value.lines[0].effective_departure = packs(9),
            1 => value.lines[0].original_debit_return_bound = packs(5),
            2 => value.lines[0].quantity_head.as_mut().unwrap().operation = id(120),
            3 => value.lines[0].funding_hash = [9; 32],
            _ => unreachable!(),
        }
        assert_eq!(
            plan(&f, &value).unwrap().linkage.resolution,
            ReturnLinkResolution::EstablishedScope
        );
        assert_ne!(digest(&value), first);
    }
    let context = f.context();
    let old = f.plan();
    assert_ne!(
        return_result_digest(
            &f.command.fingerprint(context.hub, owner(), f.ids),
            &f.command,
            &context,
            &old
        )
        .unwrap(),
        first
    );
}
