//! Pure output-seal fault cases only. These fixtures supply no native admission,
//! physical observation acceptance or found-posting qualification.
use super::birth::{BirthData, FoundBirthProjection};
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualPhysicalBirth, ActualPhysicalBirthKind},
    corrections::found_stock_group_id,
    movement::{PhysicalIdentification, PhysicalStockGroup},
    portions::{PhysicalLineage, PhysicalLineageNode, PhysicalRecoveryOrigin},
    reference::{
        HubLabel, LocationPurpose, ReferenceRecord, ReferenceValue, SampleHandlingBasis,
        SourceProvenance, WholePacks,
    },
    stock::Occurrence,
    treatment::{InitialGroupReview, TreatmentStamp, treatment_record_id},
};
fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn note(text: &str) -> SourceProvenance {
    SourceProvenance::new(text).unwrap()
}
fn fixture() -> (
    OperationRecord,
    HubId,
    BirthData,
    FoundBirthProjection,
    PhysicalLineage,
) {
    let hub = HubId::from_bytes([1; 16]).unwrap();
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let operation = crate::hub::stock::tests::operation(4);
    let own = crate::hub::actuals::common::operation_id(&operation).unwrap();
    let debit =
        crate::hub::actuals::common::operation_id(&crate::hub::stock::tests::operation(2)).unwrap();
    let observed =
        crate::hub::actuals::common::operation_id(&crate::hub::stock::tests::operation(3)).unwrap();
    let now = UnixTimestamp::from_seconds(200).unwrap();
    let child = found_stock_group_id(own, 0).unwrap();
    let recovery = PhysicalRecoveryOrigin {
        debit_operation: debit,
        debit_revision: rev(8),
    };
    let group = PhysicalStockGroup {
        id: child,
        trace: id(20),
        variant: id(21),
        parent: Some(id(10)),
        identification: Some(PhysicalIdentification {
            label: HubLabel::new("Separately observed crate").unwrap(),
            observation: note("Independent identification"),
        }),
        location: id(22),
        packs: WholePacks::from_value(2).unwrap(),
        revision: HubRevision::INITIAL,
        causal_floor: Some(now),
        work: Vec::new(),
    };
    let graph = PhysicalLineage::new(
        hub,
        id(20),
        id(21),
        rev(10),
        vec![
            PhysicalLineageNode {
                id: id(10),
                parent: None,
                trace: id(20),
                variant: id(21),
                created_revision: rev(2),
                packs: WholePacks::from_value(14).unwrap(),
                recovery: None,
            },
            PhysicalLineageNode {
                id: child,
                parent: Some(id(10)),
                trace: id(20),
                variant: id(21),
                created_revision: rev(10),
                packs: group.packs,
                recovery: Some(recovery),
            },
        ],
    )
    .unwrap();
    let projection = FoundBirthProjection {
        group,
        location: ReferenceRecord {
            id: id(22),
            revision: rev(1),
            value: ReferenceValue::Location {
                label: HubLabel::new("Inspected place").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        },
        birth: ActualPhysicalBirth {
            kind: ActualPhysicalBirthKind::FoundRemainder { allocation: 0 },
            operation: own,
            hub,
            trace: id(20),
            variant: id(21),
            group: child,
            parent: Some(id(10)),
            stock_revision: rev(10),
            packs: WholePacks::from_value(2).unwrap(),
            actor,
            occurred: Occurrence::Known(now),
            recorded_at: now,
        },
        recovery,
        basis: note("Independent recorded factual review"),
        cohort_evidence: note("Identified outside the post-debit remainder"),
        observation_operation: observed,
        target_operation: debit,
        target_group: id(10),
        allocation: 0,
    };
    let data = BirthData {
        schema: 1,
        location_revision: 1,
        debit_packs: 6,
        debit_before_packs: 6,
        debit_before_group_revision: 1,
        debit_after_group_revision: 2,
        debit_result_hash: [7; 32],
        debit_kind: "departure".into(),
        debit_actor: actor.to_hex(),
        debit_input_hash: [8; 32],
        debit_catalog_revision: 3,
        debit_result: id(30).to_hex(),
        debit_recorded_at: 100,
    };
    let op = OperationRecord {
        id: operation,
        kind: "correction_found".into(),
        actor,
        hash: [9; 32],
        catalog_revision: rev(3),
        revision: rev(10),
        result: id(40),
        recorded_at: now,
    };
    (op, hub, data, projection, graph)
}

#[test]
fn birth_seal_binds_complete_output_observation_and_old_debit_evidence() {
    let (op, hub, data, birth, graph) = fixture();
    birth.birth.validate(&graph, op.recorded_at).unwrap();
    let original = super::birth::seal(&op, hub, &data, &birth).unwrap();
    for field in 0..8 {
        let mut changed = birth.clone();
        match field {
            0 => changed.observation_operation = id(51),
            1 => changed.target_operation = id(52),
            2 => changed.target_group = id(53),
            3 => changed.basis = note("Different factual basis"),
            4 => changed.cohort_evidence = note("Different cohort claim"),
            5 => {
                changed.location.value = ReferenceValue::Location {
                    label: HubLabel::new("Different historical label").unwrap(),
                    purpose: LocationPurpose::Storage,
                }
            }
            6 => {
                changed.group.packs = WholePacks::from_value(3).unwrap();
                changed.birth.packs = changed.group.packs;
            }
            7 => changed.recovery.debit_operation = id(54),
            _ => unreachable!(),
        }
        assert_ne!(
            super::birth::seal(&op, hub, &data, &changed).unwrap(),
            original
        );
    }
    let mut changed = data.clone();
    changed.debit_result_hash = [6; 32];
    assert_ne!(
        super::birth::seal(&op, hub, &changed, &birth).unwrap(),
        original
    );
    changed = data.clone();
    changed.debit_input_hash = [5; 32];
    assert_ne!(
        super::birth::seal(&op, hub, &changed, &birth).unwrap(),
        original
    );
    changed = data;
    changed.debit_recorded_at = 101;
    assert_ne!(
        super::birth::seal(&op, hub, &changed, &birth).unwrap(),
        original
    );
}

#[test]
fn birth_projection_rejects_inconsistent_newborns_before_serializing_them() {
    let (op, hub, data, birth, _) = fixture();
    for field in 0..6 {
        let mut changed = birth.clone();
        match field {
            0 => changed.group.revision = rev(2),
            1 => changed.group.parent = None,
            2 => changed.group.causal_floor = None,
            3 => changed.group.packs = WholePacks::from_value(0).unwrap(),
            4 => changed.allocation = 64,
            5 => changed.birth.kind = ActualPhysicalBirthKind::Receipt,
            _ => unreachable!(),
        }
        assert!(super::birth::seal(&op, hub, &data, &changed).is_err());
    }
    let mut changed = data.clone();
    changed.debit_actor = "a".repeat(64 * 1024);
    assert!(super::birth::seal(&op, hub, &changed, &birth).is_err());
    changed = data;
    changed.debit_before_packs = 1;
    assert!(super::birth::seal(&op, hub, &changed, &birth).is_err());
}

#[test]
fn initial_review_seal_binds_exact_newborn_scope_and_separate_birth_seals() {
    let (op, hub, data, birth, graph) = fixture();
    let own = crate::hub::actuals::common::operation_id(&op.id).unwrap();
    let review = InitialGroupReview {
        stamp: TreatmentStamp {
            id: treatment_record_id(own).unwrap(),
            stock_revision: op.revision,
            actor: op.actor,
            basis: SampleHandlingBasis::WHOLE_PACK_V1,
            recorded_at: op.recorded_at,
            occurred: Occurrence::Known(op.recorded_at),
            observation: birth.basis.clone(),
        },
        scope: graph.capture(&[birth.group.id]).unwrap(),
    };
    let birth_seal = super::birth::seal(&op, hub, &data, &birth).unwrap();
    let expected =
        super::initial_review::seal(&op, hub, &review, &[(birth.group.id, birth_seal)]).unwrap();
    assert_ne!(expected, birth_seal);
    assert_ne!(
        super::initial_review::seal(&op, hub, &review, &[(birth.group.id, [1; 32])]).unwrap(),
        expected
    );
    assert!(super::initial_review::seal(&op, hub, &review, &[]).is_err());
    assert!(super::initial_review::seal(&op, hub, &review, &[(id(99), birth_seal)]).is_err());
    let mut changed = review;
    changed.stamp.occurred = Occurrence::Unknown;
    assert!(
        super::initial_review::seal(&op, hub, &changed, &[(birth.group.id, birth_seal)]).is_err()
    );
}
