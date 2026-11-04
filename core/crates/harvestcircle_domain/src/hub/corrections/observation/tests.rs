use super::*;
use crate::hub::corrections::PhysicalDebitKey;
use crate::hub::movement::PhysicalIdentification;
use crate::hub::reference::HubLabel;

fn id(n: u128) -> HubRecordId {
    HubRecordId::from_bytes(n.to_be_bytes()).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn hub() -> HubId {
    HubId::from_bytes(1_u128.to_be_bytes()).unwrap()
}
fn basis() -> SourceProvenance {
    SourceProvenance::new("Observed independently; factual review pending").unwrap()
}
fn allocation() -> FoundStockInput {
    FoundStockInput {
        debit: PhysicalDebitKey {
            operation: id(2),
            group: id(3),
        },
        packs: WholePacks::from_value(0).unwrap(),
        location: id(4),
        identification: PhysicalIdentification {
            label: HubLabel::new("Observed remainder").unwrap(),
            observation: basis(),
        },
        cohort_evidence: basis(),
    }
}
fn found(count: usize) -> CorrectionObservationCommand {
    CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: id(5),
            target_group: id(6),
            allocations: vec![allocation(); count],
            basis: basis(),
        },
    }
}
fn classification() -> CorrectionObservationCommand {
    CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: id(7),
            corrected_packs: WholePacks::from_value(0).unwrap(),
            report: None,
            basis: basis(),
        },
    }
}
fn preserve(command: &CorrectionObservationCommand) -> CorrectionObservationRecord {
    command
        .preserve(
            hub(),
            HubRevision::from_value(12).unwrap(),
            HubRevision::from_value(3).unwrap(),
            id(20),
            actor(),
            UnixTimestamp::from_seconds(100).unwrap(),
        )
        .unwrap()
}
#[test]
fn preserves_zero_and_duplicate_found_claims_without_factual_acceptance() {
    let command = found(64);
    let record = preserve(&command);
    assert_eq!(record.observation, command);
    assert_eq!(record.stock_revision.value(), 13);
    assert_eq!(record.catalog_revision.value(), 3);
    assert_eq!(record.observation.facts.group(), Some(id(6)));
    assert_eq!(record.observation.facts.family(), "found");
    assert_eq!(record.observation.facts.target(), id(5));
    assert_eq!(
        found(0).validate(),
        Err(CorrectionObservationError::Capacity)
    );
    assert_eq!(
        found(65).validate(),
        Err(CorrectionObservationError::Capacity)
    );
}
#[test]
fn zero_classification_requires_no_invented_departure_report() {
    let command = classification();
    let record = preserve(&command);
    assert_eq!(record.observation, command);
    assert_eq!(record.observation.facts.family(), "count_classification");
    assert_eq!(record.observation.facts.target(), id(7));
    assert_eq!(record.observation.facts.group(), None);
    let fingerprint = command.fingerprint(hub(), actor(), id(20)).unwrap();
    assert!(correction_observation_result_digest(&fingerprint, &record).is_ok());
}
#[test]
fn explicit_predecessor_changes_intent_and_cannot_be_self() {
    let command = classification();
    let mut changed = command.clone();
    changed.previous = Some(id(19));
    assert_ne!(
        command.fingerprint(hub(), actor(), id(20)).unwrap(),
        changed.fingerprint(hub(), actor(), id(20)).unwrap()
    );
    assert_eq!(preserve(&changed).observation.previous, Some(id(19)));
    changed.previous = Some(id(20));
    assert_eq!(
        changed.preserve(
            hub(),
            HubRevision::INITIAL,
            HubRevision::INITIAL,
            id(20),
            actor(),
            UnixTimestamp::from_seconds(100).unwrap()
        ),
        Err(CorrectionObservationError::InvalidIdentity)
    );
}
#[test]
fn fingerprints_bind_each_found_fact_and_native_identity() {
    let command = found(1);
    let fingerprint = command.fingerprint(hub(), actor(), id(20)).unwrap();
    let mut variants = Vec::new();
    for field in 0..9 {
        let mut changed = command.clone();
        let CorrectionObservedFacts::Found {
            target_operation,
            target_group,
            allocations,
            ..
        } = &mut changed.facts
        else {
            unreachable!()
        };
        match field {
            0 => *target_operation = id(50),
            1 => *target_group = id(60),
            2 => allocations[0].debit.operation = id(22),
            3 => allocations[0].debit.group = id(33),
            4 => allocations[0].packs = WholePacks::from_value(1).unwrap(),
            5 => allocations[0].location = id(44),
            6 => {
                allocations[0].identification.label =
                    HubLabel::new("Other observed remainder").unwrap()
            }
            7 => {
                allocations[0].identification.observation =
                    SourceProvenance::new("Other observation").unwrap()
            }
            8 => {
                allocations[0].cohort_evidence =
                    SourceProvenance::new("Other independent cohort evidence").unwrap()
            }
            _ => unreachable!(),
        }
        variants.push(changed.fingerprint(hub(), actor(), id(20)).unwrap());
        let CorrectionObservedFacts::Found { basis: reason, .. } = &mut changed.facts else {
            unreachable!()
        };
        *reason = SourceProvenance::new("Other basis").unwrap();
        variants.push(changed.fingerprint(hub(), actor(), id(20)).unwrap());
    }
    variants.push(command.fingerprint(hub(), actor(), id(21)).unwrap());
    variants.push(
        command
            .fingerprint(
                HubId::from_bytes(2_u128.to_be_bytes()).unwrap(),
                actor(),
                id(20),
            )
            .unwrap(),
    );
    assert!(variants.iter().all(|value| *value != fingerprint));
}
#[test]
fn result_seal_binds_native_cut_and_rejects_corrupt_identity_or_body() {
    let command = classification();
    let fingerprint = command.fingerprint(hub(), actor(), id(20)).unwrap();
    let record = preserve(&command);
    let digest = correction_observation_result_digest(&fingerprint, &record).unwrap();
    for field in 0..3 {
        let mut changed = record.clone();
        match field {
            0 => changed.stock_revision = HubRevision::from_value(14).unwrap(),
            1 => changed.catalog_revision = HubRevision::from_value(4).unwrap(),
            2 => changed.recorded_at = UnixTimestamp::from_seconds(101).unwrap(),
            _ => unreachable!(),
        }
        assert_ne!(
            correction_observation_result_digest(&fingerprint, &changed).unwrap(),
            digest
        );
    }
    let mut corrupt = record.clone();
    corrupt.id = id(99);
    assert_eq!(
        correction_observation_result_digest(&fingerprint, &corrupt),
        Err(CorrectionObservationError::InvalidHistory)
    );
    corrupt = record.clone();
    corrupt.stock_revision = HubRevision::INITIAL;
    assert_eq!(
        correction_observation_result_digest(&fingerprint, &corrupt),
        Err(CorrectionObservationError::InvalidHistory)
    );
    corrupt = record;
    let CorrectionObservedFacts::CountClassification {
        corrected_packs, ..
    } = &mut corrupt.observation.facts
    else {
        unreachable!()
    };
    *corrected_packs = WholePacks::from_value(1).unwrap();
    assert_eq!(
        correction_observation_result_digest(&fingerprint, &corrupt),
        Err(CorrectionObservationError::InvalidHistory)
    );
}
