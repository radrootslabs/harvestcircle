use super::*;
use crate::hub::reference::{SourceAttribution, SourceProvenance, WholePacks};
use crate::hub::stock::Occurrence;
fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn hub() -> HubId {
    HubId::from_bytes([1; 16]).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent original-linked factual evidence").unwrap()
}

#[test]
fn attribution_intents_bind_context_unknowns_heads_and_manual_work_separately() {
    let source = SourceAttributionCommand {
        trace: id(2),
        expected_stock_revision: rev(5),
        expected_catalog_revision: rev(3),
        expected_prior: None,
        source: SourceAttribution::Unknown,
        basis: note(),
    };
    let recipient = RecipientAttributionCommand {
        departure_operation: id(2),
        expected_stock_revision: rev(5),
        expected_catalog_revision: rev(3),
        expected_prior: None,
        recipient: None,
        basis: note(),
    };
    let first = source.fingerprint(hub(), actor(), id(20));
    assert_eq!(source.fingerprint(hub(), actor(), id(20)), first);
    assert_ne!(first, recipient.fingerprint(hub(), actor(), id(20)));
    let mut changed = source.clone();
    changed.expected_prior = Some(id(19));
    assert_ne!(first, changed.fingerprint(hub(), actor(), id(20)));
    changed = source.clone();
    changed.expected_stock_revision = rev(6);
    assert_ne!(first, changed.fingerprint(hub(), actor(), id(20)));
    changed = source.clone();
    changed.expected_catalog_revision = rev(4);
    assert_ne!(first, changed.fingerprint(hub(), actor(), id(20)));
    assert_ne!(first, source.fingerprint(hub(), actor(), id(21)));
    assert_ne!(
        first,
        source.fingerprint(HubId::from_bytes([2; 16]).unwrap(), actor(), id(20))
    );
    let r = recipient.fingerprint(hub(), actor(), id(20));
    let mut changed = recipient.clone();
    changed.recipient = Some(id(4));
    assert_ne!(r, changed.fingerprint(hub(), actor(), id(20)));
    let follow = CorrectionFollowUpCommand {
        correction: id(8),
        target: CorrectionImpact::Recipient(id(4)),
        expected_stock_revision: rev(5),
        expected_catalog_revision: rev(3),
        expected_prior: None,
        state: CorrectionManualWork::Outstanding,
        basis: note(),
    };
    let before = follow.fingerprint(hub(), actor(), id(20));
    let mut work = follow.clone();
    work.state = CorrectionManualWork::WorkRecorded;
    assert_ne!(before, work.fingerprint(hub(), actor(), id(20)));
    let mut unknown = follow.clone();
    unknown.target = CorrectionImpact::UnresolvedRecipient(id(4));
    assert_ne!(before, unknown.fingerprint(hub(), actor(), id(20)));
    let mut doc = follow.clone();
    doc.target = CorrectionImpact::IssuedVersion(id(4));
    assert_ne!(before, doc.fingerprint(hub(), actor(), id(20)));
    let mut head = follow;
    head.expected_prior = Some(id(7));
    assert_ne!(before, head.fingerprint(hub(), actor(), id(20)));
}

#[test]
fn additional_intent_hashes_the_factual_total_and_exact_independent_balancing_source() {
    let command = AdditionalQuantityCommand {
        facts: CorrectedLineFacts {
            report: id(1),
            target_operation: id(2),
            target_group: id(3),
            expected_stock_revision: rev(5),
            expected_catalog_revision: rev(3),
            expected_prior: None,
            corrected_line_packs: WholePacks::from_value(8).unwrap(),
            recipient: None,
            occurred: Occurrence::Unknown,
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: id(4),
        source_group: id(5),
        balancing_group: id(8),
        expected_balancing_revision: rev(2),
        duplicate_review_basis: note(),
    };
    let original = command.fingerprint(hub(), actor(), id(20));
    for field in 0..9 {
        let mut changed = command.clone();
        match field {
            0 => changed.facts.corrected_line_packs = WholePacks::from_value(2).unwrap(),
            1 => changed.source_operation = id(6),
            2 => changed.source_group = id(6),
            3 => changed.expected_balancing_revision = rev(3),
            4 => changed.facts.expected_prior = Some(id(9)),
            5 => changed.facts.recipient = Some(id(9)),
            6 => changed.facts.target_group = id(9),
            7 => {
                changed.duplicate_review_basis =
                    SourceProvenance::new("Other duplicate review evidence").unwrap()
            }
            8 => changed.balancing_group = id(9),
            _ => unreachable!(),
        }
        assert_ne!(original, changed.fingerprint(hub(), actor(), id(20)));
    }
}

#[test]
fn count_explanation_intent_keeps_shared_account_head_separate_from_raw_report() {
    let command = CountExplanationFacts {
        report: id(1),
        count_reconciliation: id(2),
        expected_stock_revision: rev(5),
        expected_catalog_revision: rev(3),
        expected_prior: Some(id(8)),
        packs: WholePacks::from_value(2).unwrap(),
        recipient: id(3),
        occurred: Occurrence::Unknown,
        source_evidence: note(),
        quantity_evidence: note(),
        recipient_evidence: note(),
        timing_evidence: note(),
    };
    let original = command.fingerprint(hub(), actor(), id(20));
    for field in 0..5 {
        let mut changed = command.clone();
        match field {
            0 => changed.expected_prior = None,
            1 => changed.report = id(9),
            2 => changed.count_reconciliation = id(9),
            3 => changed.packs = WholePacks::from_value(1).unwrap(),
            4 => changed.recipient = id(9),
            _ => unreachable!(),
        }
        assert_ne!(original, changed.fingerprint(hub(), actor(), id(20)));
    }
}

#[test]
fn found_effect_intent_is_bounded_and_distinct_from_saved_observation() {
    use crate::hub::{movement::PhysicalIdentification, reference::HubLabel};
    let allocation = FoundStockInput {
        debit: PhysicalDebitKey {
            operation: id(2),
            group: id(3),
        },
        packs: WholePacks::from_value(1).unwrap(),
        location: id(4),
        identification: PhysicalIdentification {
            label: HubLabel::new("Separately identified crates").unwrap(),
            observation: note(),
        },
        cohort_evidence: note(),
    };
    let mut command = FoundStockCommand {
        target_operation: id(5),
        target_group: id(6),
        expected_stock_revision: rev(10),
        expected_catalog_revision: rev(3),
        allocations: vec![allocation; 64],
        basis: note(),
    };
    let observed = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: command.target_operation,
            target_group: command.target_group,
            allocations: command.allocations.clone(),
            basis: command.basis.clone(),
        },
    };
    let effect = command.fingerprint(hub(), actor(), id(20)).unwrap();
    assert_ne!(
        effect,
        observed.fingerprint(hub(), actor(), id(20)).unwrap()
    );
    command.expected_stock_revision = rev(11);
    assert_ne!(effect, command.fingerprint(hub(), actor(), id(20)).unwrap());
    command.allocations.push(command.allocations[0].clone());
    assert_eq!(
        command.fingerprint(hub(), actor(), id(20)),
        Err(CorrectionObservationError::Capacity)
    );
    command.allocations.clear();
    assert_eq!(
        command.fingerprint(hub(), actor(), id(20)),
        Err(CorrectionObservationError::Capacity)
    );
}
