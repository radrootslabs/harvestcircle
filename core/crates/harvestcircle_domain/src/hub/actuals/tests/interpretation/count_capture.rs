use super::*;
use std::sync::Arc;

fn facts(f: &Fixture, records: Vec<ActualKnownInterpretation>) -> ActualKnownInterpretations {
    ActualKnownInterpretations {
        hub: f.graph.hub(),
        stock_revision: f.graph.revision(),
        catalog_revision: f.catalog.revision(),
        latest_recorded_at: at(160),
        records,
    }
}
#[test]
fn count_capture_interns_equal_shared_accounts_and_retains_selected_zero_meaning() {
    let (mut f, _) = fixture();
    let records = count_records(&mut f);
    assert!(!Arc::ptr_eq(
        records[0].count_explanation.as_ref().unwrap(),
        records[1].count_explanation.as_ref().unwrap()
    ));
    let inventory = ActualKnownCountInventory::new(facts(&f, records)).unwrap();
    let records = &inventory.facts().records;
    assert!(Arc::ptr_eq(
        records[0].count_explanation.as_ref().unwrap(),
        records[1].count_explanation.as_ref().unwrap()
    ));
    assert_eq!(records[0].view().unwrap().effective_packs, packs(0));
    assert_eq!(records[1].view().unwrap().effective_packs, packs(1));
    assert_eq!(
        inventory.clone().digest().unwrap(),
        inventory.digest().unwrap()
    );
}
#[test]
fn shared_capture_rejects_conflicts_duplicate_events_missing_accounts_and_wrong_cuts() {
    let (mut f, _) = fixture();
    let records = count_records(&mut f);
    for fault in 0..6 {
        let mut bad = records.clone();
        match fault {
            0 => {
                Arc::make_mut(bad[1].count_explanation.as_mut().unwrap())
                    .count
                    .reason = note("Conflicting canonical shortage original")
            }
            1 => bad.push(bad[0].clone()),
            2 => bad[0].count_explanation = None,
            3 => bad[0].stock_revision = rev(21),
            4 => bad[0].latest_recorded_at = at(170),
            _ => {
                bad.remove(0);
            }
        }
        assert!(
            ActualKnownCountInventory::new(facts(&f, bad)).is_err(),
            "fault {fault}"
        );
    }
}
#[test]
fn shared_capture_digest_binds_empty_cut_selected_event_recipients_and_account_history() {
    let (mut f, _) = fixture();
    let records = count_records(&mut f);
    let base = ActualKnownCountInventory::new(facts(&f, records.clone()))
        .unwrap()
        .digest()
        .unwrap();
    let empty = ActualKnownCountInventory::new(facts(&f, vec![])).unwrap();
    assert_ne!(base, empty.digest().unwrap());
    let mut changed = facts(&f, vec![]);
    changed.latest_recorded_at = at(161);
    assert_ne!(
        empty.digest().unwrap(),
        ActualKnownCountInventory::new(changed)
            .unwrap()
            .digest()
            .unwrap()
    );
    for fault in 0..3 {
        let mut changed = records.clone();
        match fault {
            0 => {
                changed[1].original.recipient = f.catalog.get(id(151)).unwrap().clone();
                changed[1].recipient_original.recipient = changed[1].original.recipient.clone();
            }
            1 => {
                let mut shared = changed[0]
                    .count_explanation
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .clone();
                shared.count.reason = note("Another retained original count basis");
                let shared = Arc::new(shared);
                for record in &mut changed {
                    record.count_explanation = Some(Arc::clone(&shared));
                }
            }
            _ => {
                recipient(&mut f, &mut changed[0], None, 93);
                changed[1].stock_revision = changed[0].stock_revision;
                changed[1].latest_recorded_at = changed[0].latest_recorded_at;
                let mut set = facts(&f, changed);
                set.latest_recorded_at = set.records[0].latest_recorded_at;
                assert_ne!(
                    base,
                    ActualKnownCountInventory::new(set)
                        .unwrap()
                        .digest()
                        .unwrap()
                );
                continue;
            }
        }
        assert_ne!(
            base,
            ActualKnownCountInventory::new(facts(&f, changed))
                .unwrap()
                .digest()
                .unwrap()
        );
    }
}
#[test]
fn complete_correction_capacity_is_unique_history_not_repeated_shared_snapshot_rows() {
    use crate::hub::corrections::{
        CountExplanationAmendment, CountLossExplanation, CountLossQuantityAccount,
    };
    fn operation(n: u64) -> HubRecordId {
        let mut bytes = [0xA5; 16];
        bytes[8..].copy_from_slice(&n.to_be_bytes());
        HubRecordId::from_bytes(bytes).unwrap()
    }
    let (mut f, _) = fixture();
    let sample = count_records(&mut f).remove(0);
    let value = sample.count_explanation.as_ref().unwrap();
    let count = crate::hub::corrections::HUB_CORRECTION_ROWS / 2;
    let explanations: Vec<_> = (0..count)
        .map(|i| CountLossExplanation {
            operation: operation((2 * i + 1) as u64),
            stock_revision: rev((2 * i + 6) as u64),
            packs: packs(1),
        })
        .collect();
    let amendments: Vec<_> = explanations
        .iter()
        .enumerate()
        .map(|(i, row)| CountExplanationAmendment {
            operation: operation((2 * i + 2) as u64),
            stock_revision: rev(row.stock_revision.value() + 1),
            explanation: row.operation,
            before: packs(1),
            after: packs(0),
        })
        .collect();
    let account = CountLossQuantityAccount::restore_corrected(
        value.account.original(),
        explanations.clone(),
        amendments,
        vec![],
    )
    .unwrap();
    let shared = Arc::new(ActualKnownCountExplanation {
        count: value.count.clone(),
        account,
    });
    advance(&mut f, rev(10_000));
    let records = explanations
        .iter()
        .map(|row| {
            let mut record = sample.clone();
            record.stock_revision = f.graph.revision();
            record.original.operation = row.operation;
            record.original.stock_revision = row.stock_revision;
            record.recipient_original.operation = row.operation;
            record.recipient_original.stock_revision = row.stock_revision;
            record.count_explanation = Some(Arc::clone(&shared));
            record
        })
        .collect();
    let inventory = ActualKnownCountInventory::new(facts(&f, records)).unwrap();
    assert_eq!(inventory.facts().records.len(), count);
    assert!(
        inventory
            .facts()
            .records
            .iter()
            .all(|row| Arc::ptr_eq(row.count_explanation.as_ref().unwrap(), &shared))
    );
    assert_eq!(
        inventory.facts().records[0].view().unwrap().effective_packs,
        packs(0)
    );
    assert_ne!(inventory.digest().unwrap(), [0; 32]);
}

#[test]
fn reused_account_proof_still_checks_each_selected_original_and_recipient() {
    let (mut f, _) = fixture();
    let records = count_records(&mut f);
    for fault in 0..8 {
        let mut bad = records.clone();
        match fault {
            0 => bad[1].original.lines[0].group = id(201),
            1 => bad[1].original.lines[0].packs = packs(2),
            2 => bad[1].recipient_original.family = CorrectedDepartureFamily::ReviewedActual,
            3 => bad[1].original.trace = id(202),
            4 => bad[1].original.variant.id = id(203),
            5 => {
                bad[1].original.recorded_at = at(109);
                bad[1].recipient_original.recorded_at = at(109);
            }
            6 => bad[1].recipient_original.recipient = f.catalog.get(id(151)).unwrap().clone(),
            _ => bad[1].quantities = accounts(&bad[1].original),
        }
        assert!(bad[1].view().is_err(), "single view fault {fault}");
        assert!(
            ActualKnownCountInventory::new(facts(&f, bad)).is_err(),
            "shared proof fault {fault}"
        );
    }
}
#[test]
fn reused_account_proof_keeps_whole_recipient_history_and_dependency_collision_checks() {
    for collision in [false, true] {
        let (mut f, _) = fixture();
        let mut records = count_records(&mut f);
        recipient(
            &mut f,
            &mut records[1],
            None,
            if collision { 20 } else { 93 },
        );
        records[0].stock_revision = records[1].stock_revision;
        records[0].latest_recorded_at = records[1].latest_recorded_at;
        let mut set = facts(&f, records.clone());
        set.latest_recorded_at = records[1].latest_recorded_at;
        if collision {
            assert_eq!(records[1].view(), Err(ActualReportError::InvalidHistory));
            assert_eq!(
                ActualKnownCountInventory::new(set),
                Err(ActualReportError::InvalidHistory)
            );
        } else {
            let inventory = ActualKnownCountInventory::new(set).unwrap();
            let view = inventory.facts().records[1].view().unwrap();
            assert!(view.recipients.has_unknown());
            assert_eq!(view.effective_packs, packs(1));
            assert!(view.recipients.could_match(Some(id(152))));
        }
    }
}
