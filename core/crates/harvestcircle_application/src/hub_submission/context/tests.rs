use super::*;
use harvestcircle_domain::hub::{
    HubId,
    reference::{HubLabel, ReferenceMutation, ReferenceValue},
};
use serde_json::Value;

fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn id(n: u128) -> HubRecordId {
    HubRecordId::from_bytes(n.to_be_bytes()).unwrap()
}
fn hub() -> HubOwnership {
    HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), owner())
}
fn scope(generation: u64) -> HubSubmissionScope {
    HubSubmissionScope {
        runtime: HubRuntimeIncarnation::from_bytes([0x12; 16]).unwrap(),
        generation: SessionGeneration::from_value(generation),
        actor: owner(),
    }
}
fn operation() -> DurableRequestId {
    DurableRequestId::parse("00000000-0000-7000-8000-000000000001").unwrap()
}
fn reference(n: u128, label: &str) -> HubSubmittedMaterial {
    HubSubmittedMaterial::Reference(Box::new(ReferenceRecord {
        id: id(n),
        revision: HubRevision::INITIAL,
        value: ReferenceValue::Product {
            label: HubLabel::new(label).unwrap(),
        },
    }))
}
fn context(material: &[HubSubmittedMaterial]) -> HubSubmissionContext<'_> {
    HubSubmissionContext {
        hub: hub(),
        catalog_revision: Some(HubRevision::INITIAL),
        stock_revision: None,
        material,
    }
}

#[test]
fn capture_retains_original_labels_exact_scope_and_operation_without_debug_contents() {
    let input = ReferenceCommand {
        expected_catalog_revision: HubRevision::INITIAL,
        mutation: ReferenceMutation::Create {
            id: id(3),
            value: ReferenceValue::Pack {
                product: id(2),
                label: HubLabel::new("Fictional whole crate").unwrap(),
                nominal_grams: None,
            },
        },
    };
    let request = HubSubmittedRequest::Reference(&input);
    let material = [reference(2, "Original \"orchard apples\"")];
    let saved = request
        .capture(&operation(), scope(u64::MAX), &context(&material))
        .unwrap();
    let value: Value = serde_json::from_slice(saved.as_bytes()).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["scope"]["generation"].as_u64(), Some(u64::MAX));
    assert_eq!(
        value["scope"]["runtime"],
        "12121212121212121212121212121212"
    );
    assert_eq!(value["scope"]["actor"], owner().to_hex());
    assert_eq!(value["operation"], operation().as_str());
    assert_eq!(
        value["input"]["mutation"]["create"]["value"]["pack"]["product"],
        id(2).to_hex()
    );
    assert_eq!(
        value["material"][0]["reference"]["value"]["product"]["label"],
        "Original \"orchard apples\""
    );
    assert_eq!(saved.input_digest(), request.archive().unwrap().digest());
    assert!(!format!("{saved:?}").contains("orchard"));
    let renamed = [reference(2, "Later display name")];
    let later = request
        .capture(&operation(), scope(u64::MAX), &context(&renamed))
        .unwrap();
    assert_eq!(saved.input_digest(), later.input_digest());
    assert_ne!(saved.digest(), later.digest());
    for changed in [
        scope(0),
        HubSubmissionScope {
            runtime: HubRuntimeIncarnation::from_bytes([3; 16]).unwrap(),
            ..scope(u64::MAX)
        },
    ] {
        assert_ne!(
            saved.digest(),
            request
                .capture(&operation(), changed, &context(&material))
                .unwrap()
                .digest()
        );
    }
    let other = DurableRequestId::parse("00000000-0000-7000-8000-000000000002").unwrap();
    assert_ne!(
        saved.digest(),
        request
            .capture(&other, scope(u64::MAX), &context(&material))
            .unwrap()
            .digest()
    );
}

#[test]
fn canonical_captures_reject_duplicate_material_and_mismatched_owner() {
    let ownership = hub();
    let request = HubSubmittedRequest::Initialize(&ownership);
    let a = [reference(2, "Apples"), reference(3, "Pears")];
    let b = [a[1].clone(), a[0].clone()];
    assert_eq!(
        request.capture(&operation(), scope(0), &context(&a)),
        request.capture(&operation(), scope(0), &context(&b))
    );
    let duplicate = [a[0].clone(), a[0].clone()];
    assert_eq!(
        request.capture(&operation(), scope(0), &context(&duplicate)),
        Err(HubRequestArchiveError::InvalidContext)
    );
    let mut foreign = scope(0);
    foreign.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_eq!(
        request.capture(&operation(), foreign, &context(&a)),
        Err(HubRequestArchiveError::InvalidContext)
    );
    assert_eq!(
        HubRuntimeIncarnation::from_bytes([0; 16]),
        Err(HubRequestArchiveError::InvalidContext)
    );
}

#[test]
fn material_allowance_never_bypasses_the_smaller_original_input_bound() {
    use harvestcircle_domain::hub::movement::NewWorkCommand;
    use harvestcircle_domain::hub::reference::SourceProvenance;
    let input = HandlingCommand::NewWork(NewWorkCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: HubRevision::INITIAL,
        groups: vec![id(4); HUB_SUBMITTED_REQUEST_MAX_BYTES / 32],
        target: id(5),
        observation: SourceProvenance::new("Fictional oversized intent").unwrap(),
    });
    assert_eq!(
        HubSubmittedRequest::Handling(&input).capture(&operation(), scope(0), &context(&[])),
        Err(HubRequestArchiveError::Capacity)
    );
    let ownership = hub();
    let material = vec![reference(2, "Fictional"); HUB_SUBMISSION_MATERIAL_CAPACITY + 1];
    assert_eq!(
        HubSubmittedRequest::Initialize(&ownership).capture(
            &operation(),
            scope(0),
            &context(&material)
        ),
        Err(HubRequestArchiveError::Capacity)
    );
}

#[test]
fn owned_requests_move_through_admission_without_decoding_or_changing_intent() {
    let original = hub();
    let raw = HubSubmittedRequest::Initialize(&original)
        .archive()
        .unwrap();
    let owned = HubMutation::Initialize(Box::new(original));
    assert_eq!(owned.submitted_request().archive().unwrap(), raw);
    let archive = owned
        .submitted_request()
        .capture(&operation(), scope(0), &context(&[]))
        .unwrap();
    assert_eq!(archive.kind(), "initialize");
    assert_eq!(archive.input_digest(), raw.digest());
}

#[test]
fn maximum_native_work_and_reference_shapes_fit_without_truncating_history() {
    use harvestcircle_domain::hub::{
        movement::{HUB_WORK_CAPACITY, WorkRequirement},
        reference::{HUB_REFERENCE_CAPACITY, LocationPurpose, WholePacks},
        stock::QuantityWork,
    };
    let target = ReferenceRecord {
        id: id(2),
        revision: HubRevision::from_value(HubRevision::MAXIMUM).unwrap(),
        value: ReferenceValue::Location {
            label: HubLabel::new("\"".repeat(120)).unwrap(),
            purpose: LocationPurpose::Storage,
        },
    };
    let mut material = Vec::new();
    for n in 1..=HUB_WORK_CAPACITY {
        let requirement = WorkRequirement {
            id: id(n as u128),
            trace: id(1),
            variant: id(2),
            target: Some(target.clone()),
            quantity: QuantityWork::pending(WholePacks::from_value(1_000_000).unwrap()),
            revision: target.revision,
        };
        requirement.validate().unwrap();
        material.push(HubSubmittedMaterial::Work(Box::new(requirement)));
    }
    material.extend((1..=HUB_REFERENCE_CAPACITY).map(|n| reference(n as u128, &"\"".repeat(120))));
    let ownership = hub();
    let saved = HubSubmittedRequest::Initialize(&ownership)
        .capture(&operation(), scope(u64::MAX), &context(&material))
        .unwrap();
    assert!(saved.as_bytes().len() < HUB_SUBMISSION_ARCHIVE_MAX_BYTES);
    let value: Value = serde_json::from_slice(saved.as_bytes()).unwrap();
    assert_eq!(
        value["material"].as_array().unwrap().len(),
        HUB_WORK_CAPACITY + HUB_REFERENCE_CAPACITY
    );
    assert_eq!(
        value["material"].as_array().unwrap().last().unwrap()["work"]["quantity"]["remaining"],
        1_000_000
    );
}

#[test]
fn inspection_checks_read_only_metadata_and_preserves_raw_input_digest() {
    let ownership = hub();
    let request = HubSubmittedRequest::Initialize(&ownership);
    let material = [reference(2, "Fictional original")];
    let saved = request
        .capture(&operation(), scope(u64::MAX), &context(&material))
        .unwrap();
    let inspected = inspect_hub_submission(saved.as_bytes()).unwrap();
    assert_eq!(inspected.operation, operation());
    assert_eq!(inspected.scope, scope(u64::MAX));
    assert_eq!(inspected.hub, ownership);
    assert_eq!(inspected.kind, saved.kind());
    assert_eq!(&inspected.input_digest, saved.input_digest());
    assert_eq!(&inspected.capture_digest, saved.digest());
    // Parsing the input is deliberately absent: an object that is not a domain
    // command is still only archival data, with a different consistency digest.
    let text = std::str::from_utf8(saved.as_bytes()).unwrap();
    let changed = text.replacen("Fictional original", "Changed original", 1);
    assert_ne!(
        &inspect_hub_submission(changed.as_bytes())
            .unwrap()
            .capture_digest,
        saved.digest()
    );
    for altered in [
        text.replacen("\"schema\":1", "\"schema\":99", 1),
        text.replacen("\"kind\":\"initialize\"", "\"kind\":\"unknown_action\"", 1),
        text.replacen(
            "\"generation\":18446744073709551615",
            "\"generation\":-1",
            1,
        ),
        text.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1),
    ] {
        assert!(inspect_hub_submission(altered.as_bytes()).is_err());
    }
}
