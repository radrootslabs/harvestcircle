//! Descriptive encoding tests. A supplied digest/set size never authenticates
//! a native prefix, supplies current allowance or creates execution permission.
use super::*;

#[test]
fn explicit_empty_return_scope_is_distinct_from_legacy_absence_without_changing_raw_input() {
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let operation = crate::DurableRequestId::parse("01900000-0000-7000-8000-000000000001").unwrap();
    let runtime = HubRuntimeIncarnation::from_bytes([7; 16]).unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation: crate::SessionGeneration::from_value(1),
        actor,
    };
    let hub = HubOwnership::new(HubId::from_bytes([8; 16]).unwrap(), actor);
    let command = ReturnCommand {
        expected_stock_revision: rev(10),
        expected_catalog_revision: rev(8),
        variant: id(1),
        location: id(2),
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: packs(1),
        identification: identification(),
        planned_location: None,
        departure: None,
        occurred: occurred(),
        observation: note(),
    };
    let request = HubSubmittedRequest::Return(&command);
    let old = HubSubmittedMaterial::Return(Box::new(HubSubmittedReturn {
        id: id(90),
        departure: None,
        established_return_totals: Vec::new(),
        current_allowances: None,
    }));
    let encode = |material: &[HubSubmittedMaterial]| {
        request
            .capture(
                &operation,
                scope,
                &HubSubmissionContext {
                    hub,
                    catalog_revision: Some(rev(8)),
                    stock_revision: Some(rev(10)),
                    material,
                },
            )
            .unwrap()
    };
    let legacy = encode(std::slice::from_ref(&old));
    let evidence = HubSubmittedReturnScope {
        id: id(90),
        hub: hub.id(),
        stock_revision: rev(10),
        epoch_stock_revision: 3,
        epoch_catalog_revision: 4,
        associations: 0,
        evidence_digest: [11; 32],
    };
    let current = encode(&[
        old.clone(),
        HubSubmittedMaterial::ReturnScope(Box::new(evidence.clone())),
    ]);
    let before = inspect_hub_submission(legacy.as_bytes()).unwrap();
    let after = inspect_hub_submission(current.as_bytes()).unwrap();
    assert_eq!(before.input_digest, after.input_digest);
    assert_ne!(before.capture_digest, after.capture_digest);
    let before: Value = serde_json::from_slice(legacy.as_bytes()).unwrap();
    let after: Value = serde_json::from_slice(current.as_bytes()).unwrap();
    assert_eq!(before["material"][0], after["material"][0]); // The old tag8 data stays exact.
    assert_eq!(after["material"][1]["return_scope"]["associations"], 0);
    for fault in 0..5 {
        let mut changed = evidence.clone();
        match fault {
            0 => changed.hub = HubId::from_bytes([9; 16]).unwrap(),
            1 => changed.stock_revision = rev(11),
            2 => changed.epoch_stock_revision = 4,
            3 => changed.associations = 1,
            4 => changed.evidence_digest[0] ^= 1,
            _ => unreachable!(),
        }
        assert_ne!(
            encode(&[
                old.clone(),
                HubSubmittedMaterial::ReturnScope(Box::new(changed))
            ])
            .digest(),
            current.digest()
        );
    }
    let duplicate = vec![
        old,
        HubSubmittedMaterial::ReturnScope(Box::new(evidence.clone())),
        HubSubmittedMaterial::ReturnScope(Box::new(evidence)),
    ];
    assert!(
        request
            .capture(
                &operation,
                scope,
                &HubSubmissionContext {
                    hub,
                    catalog_revision: Some(rev(8)),
                    stock_revision: Some(rev(10)),
                    material: &duplicate,
                }
            )
            .is_err()
    );
}
