use super::*;
use harvestcircle_domain::hub::corrections::CountExplanationFacts;

#[test]
fn count_requests_preserve_source_and_observation_head_contracts_as_data_only() {
    let command = CountExplanationCommand {
        facts: CountExplanationFacts {
            report: id(2),
            count_reconciliation: id(3),
            expected_stock_revision: rev(4),
            expected_catalog_revision: rev(5),
            expected_prior: Some(id(6)),
            packs: packs(1),
            recipient: id(7),
            occurred: Occurrence::Unknown,
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: id(8),
        source_group: id(9),
    };
    let (archive, value) = parsed(&HubSubmittedRequest::CountExplanation(&command));
    assert_eq!(archive.kind(), "correction_count_explain");
    assert_eq!(
        value["input"]["source_operation"],
        serde_json::to_value(id(8)).unwrap()
    );
    assert_eq!(
        value["input"]["source_group"],
        serde_json::to_value(id(9)).unwrap()
    );
    assert_eq!(
        value["input"]["facts"]["count_reconciliation"],
        serde_json::to_value(id(3)).unwrap()
    );
    let changed = CountExplanationCommand {
        source_operation: id(10),
        ..command.clone()
    };
    assert_ne!(
        archive.digest(),
        HubSubmittedRequest::CountExplanation(&changed)
            .archive()
            .unwrap()
            .digest()
    );

    let command = CountClassificationCommand {
        observation_operation: id(11),
        expected_stock_revision: rev(12),
        expected_catalog_revision: rev(13),
        expected_account_prior: Some(id(14)),
        expected_prior: Some(id(15)),
    };
    let (archive, value) = parsed(&HubSubmittedRequest::CountClassification(&command));
    assert_eq!(archive.kind(), "correction_count_classify");
    assert_eq!(
        value["input"]["observation_operation"],
        serde_json::to_value(id(11)).unwrap()
    );
    assert_eq!(
        value["input"]["expected_account_prior"],
        serde_json::to_value(id(14)).unwrap()
    );
    assert_eq!(
        value["input"]["expected_prior"],
        serde_json::to_value(id(15)).unwrap()
    );
    assert!(value["input"].get("corrected_packs").is_none());
    let changed = CountClassificationCommand {
        expected_prior: None,
        ..command
    };
    assert_ne!(
        archive.digest(),
        HubSubmittedRequest::CountClassification(&changed)
            .archive()
            .unwrap()
            .digest()
    );
}
