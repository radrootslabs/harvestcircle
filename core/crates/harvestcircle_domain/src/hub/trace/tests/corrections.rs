use super::*;
use crate::hub::corrections::{
    DepartureQuantityAccount, DepartureQuantityAmendment, DepartureQuantityDelta,
    OriginalDepartureQuantity,
};

fn corrected() -> TraceSnapshot {
    let mut snapshot = fixture();
    snapshot.stock_revision = rev(6);
    let mut found = snapshot.on_hand[0].clone();
    found.group = id(60);
    found.revision = HubRevision::INITIAL;
    found.packs = packs(2);
    found.treatment.group = found.group;
    found.treatment.packs = found.packs;
    found.identification = Some(PhysicalIdentification {
        label: label("New found physical crates"),
        observation: text("Independently observed outside the departed remainder"),
    });
    snapshot.on_hand.push(found);
    let exit = &snapshot.exits[0];
    let original = OriginalDepartureQuantity {
        operation: exit.departure.operation,
        group: exit.group,
        trace: snapshot.trace.id,
        variant: snapshot.trace.variant.id,
        stock_revision: exit.departure.stock_revision,
        packs: exit.packs,
    };
    snapshot.quantity_corrections.push(
        DepartureQuantityAccount::restore(
            original,
            vec![DepartureQuantityAmendment {
                operation: id(61),
                stock_revision: rev(6),
                before: packs(5),
                after: packs(3),
                delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(2)),
            }],
            packs(2),
        )
        .unwrap(),
    );
    snapshot
}

#[test]
fn current_found_balance_preserves_gross_original_and_does_not_subtract_returns_twice() {
    let snapshot = corrected();
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.departed.value()
        ),
        (20, 17, 3)
    );
    assert_eq!(snapshot.exits[0].packs.value(), 5);
    assert_eq!(
        snapshot.quantity_corrections[0]
            .established_returns()
            .value(),
        2
    );
}

#[test]
fn omitted_duplicate_unrelated_or_future_account_cannot_explain_current_found_mass() {
    let snapshot = corrected();
    let mut missing = snapshot.clone();
    missing.quantity_corrections.clear();
    assert_eq!(missing.balance(), Err(TraceError::Accounting));
    let mut duplicate = snapshot.clone();
    duplicate
        .quantity_corrections
        .push(duplicate.quantity_corrections[0].clone());
    assert_eq!(duplicate.balance(), Err(TraceError::InvalidSnapshot));
    for fault in 0..3 {
        let mut changed = snapshot.clone();
        let account = &snapshot.quantity_corrections[0];
        let mut original = account.original();
        let mut history = account.amendments().to_vec();
        match fault {
            0 => original.group = id(62),
            1 => original.variant = id(63),
            _ => history[0].stock_revision = rev(7),
        };
        changed.quantity_corrections[0] =
            DepartureQuantityAccount::restore(original, history, packs(2)).unwrap();
        assert_eq!(changed.balance(), Err(TraceError::InvalidSnapshot));
    }
}
