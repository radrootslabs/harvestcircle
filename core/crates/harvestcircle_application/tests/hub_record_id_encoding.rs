use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    HubRevision,
    reference::{HubLabel, HubRecordId, ReferenceCommand, ReferenceMutation, ReferenceValue},
};

#[test]
fn record_id_formatter_preserves_published_json_and_request_digest() {
    let id = HubRecordId::from_bytes([
        0x00, 0x0f, 0x10, 0x7f, 0x80, 0xff, 0x01, 0xab, 0xcd, 0xef, 0x09, 0x0a, 0x55, 0xaa, 0xfe,
        0xff,
    ])
    .unwrap();
    assert_eq!(
        serde_json::to_string(&id).unwrap(),
        "\"000f107f80ff01abcdef090a55aafeff\""
    );
    let command = ReferenceCommand {
        expected_catalog_revision: HubRevision::INITIAL,
        mutation: ReferenceMutation::Revise {
            id,
            expected_record_revision: HubRevision::INITIAL,
            value: ReferenceValue::Party {
                label: HubLabel::new("Archive party").unwrap(),
            },
        },
    };
    let archive = HubSubmittedRequest::Reference(&command).archive().unwrap();
    // Published v1 field ordering, lowercase ID and digest purpose are fixed.
    let expected = br#"{"schema":1,"kind":"reference","input":{"expected_catalog_revision":1,"mutation":{"revise":{"id":"000f107f80ff01abcdef090a55aafeff","expected_record_revision":1,"value":{"party":{"label":"Archive party"}}}}}}"#;
    assert_eq!(archive.as_bytes(), expected);
    let digest: String = archive
        .digest()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        digest,
        "cda0521b9e2bb19898609d3f730e01e66d8be12e070a2b2ec6c3df3565a2b5d8"
    );
}
