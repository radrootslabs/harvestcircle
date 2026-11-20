use super::*;

fn command() -> CountReturnAssociationCommand {
    CountReturnAssociationCommand {
        return_operation: id(1),
        explanation_operation: id(2),
        expected_stock_revision: rev(10),
        expected_catalog_revision: rev(8),
        expected_account_prior: Some(id(3)),
        expected_returns_prior: Some(id(4)),
        source_operation: id(5),
        source_group: id(6),
        source_evidence: note(),
        quantity_evidence: note(),
        timing_evidence: note(),
    }
}

#[test]
fn association_archive_binds_original_scope_current_heads_and_all_evidence() {
    let c = command();
    let (archive, value) = parsed(&HubSubmittedRequest::CountReturnAssociation(&c));
    assert_eq!(archive.kind(), "count_return_associate");
    assert!(known_hub_submission_kind(archive.kind()));
    assert_eq!(value["input"], serde_json::to_value(&c).unwrap());
    for field in 0..11 {
        let mut changed = c.clone();
        match field {
            0 => changed.return_operation = id(9),
            1 => changed.explanation_operation = id(9),
            2 => changed.expected_stock_revision = rev(11),
            3 => changed.expected_catalog_revision = rev(9),
            4 => changed.expected_account_prior = None,
            5 => changed.expected_returns_prior = None,
            6 => changed.source_operation = id(9),
            7 => changed.source_group = id(9),
            8 => {
                changed.source_evidence = SourceProvenance::new("Changed source evidence").unwrap()
            }
            9 => {
                changed.quantity_evidence =
                    SourceProvenance::new("Changed whole-intake evidence").unwrap()
            }
            _ => {
                changed.timing_evidence =
                    SourceProvenance::new("Changed occurrence evidence").unwrap()
            }
        }
        assert_ne!(
            archive.digest(),
            HubSubmittedRequest::CountReturnAssociation(&changed)
                .archive()
                .unwrap()
                .digest(),
            "{field}"
        );
    }
}

#[test]
fn descriptive_association_capture_inspects_without_decoding_execution_or_raising_input_limit() {
    let mut c = command();
    let maximum = SourceProvenance::new("\"".repeat(512)).unwrap();
    c.source_evidence = maximum.clone();
    c.quantity_evidence = maximum.clone();
    c.timing_evidence = maximum;
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let operation = crate::DurableRequestId::parse("01900000-0000-7000-8000-000000000001").unwrap();
    let scope = HubSubmissionScope {
        runtime: HubRuntimeIncarnation::from_bytes([7; 16]).unwrap(),
        generation: crate::SessionGeneration::from_value(1),
        actor,
    };
    let hub = HubOwnership::new(HubId::from_bytes([8; 16]).unwrap(), actor);
    // Encoding an empty descriptive material list is not native prefix proof.
    let context = HubSubmissionContext {
        hub,
        stock_revision: Some(c.expected_stock_revision),
        catalog_revision: Some(c.expected_catalog_revision),
        material: &[],
    };
    let request = HubSubmittedRequest::CountReturnAssociation(&c);
    let capture = request.capture(&operation, scope, &context).unwrap();
    let metadata = inspect_hub_submission(capture.as_bytes()).unwrap();
    assert_eq!(metadata.operation, operation);
    assert_eq!(metadata.kind, "count_return_associate");
    assert_eq!(metadata.input_digest, *request.archive().unwrap().digest());
    assert_eq!(metadata.capture_digest, *capture.digest());
    assert_eq!(HUB_SUBMITTED_REQUEST_MAX_BYTES, 64 * 1024);
    assert!(request.archive().unwrap().as_bytes().len() < HUB_SUBMITTED_REQUEST_MAX_BYTES);
}
