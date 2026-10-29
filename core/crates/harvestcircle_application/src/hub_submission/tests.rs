use super::*;
use harvestcircle_domain::hub::counts::{CountObserveCommand, CountPostCommand};
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::dispatch::{DispatchAction, DispatchLineInput, DispatchSelection};
use harvestcircle_domain::hub::movement::{
    DiscrepancyAction, DiscrepancyCommand, MovementCommand, NewWorkCommand, PhysicalIdentification,
    ReplanCommand,
};
use harvestcircle_domain::hub::portions::SplitCommand;
use harvestcircle_domain::hub::reference::{
    HubLabel, HubRecordId, ReferenceMutation, ReferenceValue, SourceAttribution, SourceProvenance,
    WholePacks,
};
use harvestcircle_domain::hub::stock::{Occurrence, OpeningHistory};
use harvestcircle_domain::hub::treatment::{
    AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection,
};
use harvestcircle_domain::hub::{HubId, HubRevision};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use serde_json::Value;
use std::io::Write;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Private operational note: exact original \\\"value\\\"").unwrap()
}
fn occurred() -> Occurrence {
    Occurrence::Known(UnixTimestamp::from_seconds(100).unwrap())
}
fn identification() -> PhysicalIdentification {
    PhysicalIdentification {
        label: HubLabel::new("Distinct blue crates").unwrap(),
        observation: note(),
    }
}
fn parsed(request: &HubSubmittedRequest<'_>) -> (ArchivedHubRequest, Value) {
    let archive = request.archive().unwrap();
    let value: Value = serde_json::from_slice(archive.as_bytes()).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["kind"], archive.kind());
    (archive, value)
}

#[test]
fn archived_reference_retains_every_input_and_canonical_identifier_without_debug_content() {
    let command = ReferenceCommand {
        expected_catalog_revision: rev(9),
        mutation: ReferenceMutation::Create {
            id: id(2),
            value: ReferenceValue::Product {
                label: HubLabel::new("Fictional quoted \"apples\"").unwrap(),
            },
        },
    };
    let (archive, value) = parsed(&HubSubmittedRequest::Reference(&command));
    assert_eq!(value["input"]["expected_catalog_revision"], 9);
    assert_eq!(value["input"]["mutation"]["create"]["id"], id(2).to_hex());
    assert_eq!(
        value["input"]["mutation"]["create"]["value"]["product"]["label"],
        "Fictional quoted \"apples\""
    );
    assert_eq!(
        archive,
        HubSubmittedRequest::Reference(&command).archive().unwrap()
    );
    assert!(!format!("{archive:?}").contains("apples"));
    let mut changed = command;
    changed.expected_catalog_revision = rev(10);
    assert_ne!(
        archive.digest(),
        HubSubmittedRequest::Reference(&changed)
            .archive()
            .unwrap()
            .digest()
    );
}

#[test]
fn count_archive_preserves_cutoff_observation_and_review_without_any_safety_claim() {
    let mut command = CountObserveCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: rev(8),
        group: id(3),
        expected_group_revision: rev(1),
        counted_packs: packs(18),
        cutoff: occurred(),
        observation: note(),
    };
    let (archive, value) = parsed(&HubSubmittedRequest::CountObservation(&command));
    assert_eq!(value["input"]["counted_packs"], 18);
    assert_eq!(value["input"]["cutoff"]["known"], 100);
    assert_eq!(value["input"]["observation"], note().as_str());
    assert!(value["input"].get("unresolved_actual").is_none());
    assert!(value["input"].get("uncertain_intent").is_none());
    assert!(!format!("{archive:?}").contains("Private operational"));
    command.cutoff = Occurrence::Unknown;
    assert_ne!(
        archive.digest(),
        HubSubmittedRequest::CountObservation(&command)
            .archive()
            .unwrap()
            .digest()
    );
    let post = CountPostCommand {
        expected_stock_revision: rev(3),
        expected_catalog_revision: rev(8),
        count: id(4),
        reviewed_activity: true,
        surplus_identification: Some(identification()),
        reason: note(),
    };
    let (_, value) = parsed(&HubSubmittedRequest::CountPosting(&post));
    assert_eq!(value["input"]["reviewed_activity"], true);
    assert_eq!(
        value["input"]["surplus_identification"]["observation"],
        note().as_str()
    );
}

#[test]
fn received_and_opening_stock_keep_source_history_and_physical_input_distinct() {
    let receipt = ReceiptCommand {
        expected_stock_revision: rev(1),
        expected_catalog_revision: rev(8),
        variant: id(2),
        supplier: Some(id(4)),
        producer: None,
        source: SourceAttribution::Unknown,
        packs: packs(20),
        receiving: id(5),
        planned_location: Some(id(6)),
        occurred: occurred(),
        observation: note(),
    };
    let (_, value) = parsed(&HubSubmittedRequest::Receive(&receipt));
    assert_eq!(value["input"]["supplier"], id(4).to_hex());
    assert_eq!(value["input"]["producer"], Value::Null);
    assert_eq!(value["input"]["source"], "unknown");
    let opening = OpeningCommand {
        expected_stock_revision: rev(1),
        expected_catalog_revision: rev(8),
        variant: id(2),
        location: id(5),
        packs: packs(8),
        identification: identification(),
        source: SourceAttribution::Unknown,
        history: OpeningHistory::ExternalReference(note()),
        counted: occurred(),
        observation: note(),
    };
    let (_, value) = parsed(&HubSubmittedRequest::Opening(&opening));
    assert_eq!(
        value["input"]["history"]["external_reference"],
        note().as_str()
    );
    assert!(value["input"].get("supplier").is_none());
    assert_eq!(
        value["input"]["identification"]["label"],
        "Distinct blue crates"
    );
}

#[test]
fn every_existing_handling_and_document_action_has_its_own_archived_kind_and_complete_input() {
    let mut requests = Vec::new();
    let movement = MovementCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: rev(8),
        group: id(3),
        expected_group_revision: rev(1),
        to: id(5),
        packs: packs(8),
        separated_portion: Some(identification()),
        occurred: occurred(),
        observation: note(),
    };
    requests.push(("move", HandlingCommand::Move(movement)));
    requests.push((
        "split",
        HandlingCommand::Split(SplitCommand {
            expected_stock_revision: rev(2),
            expected_catalog_revision: rev(8),
            group: id(3),
            expected_group_revision: rev(1),
            packs: packs(8),
            identification: identification(),
            occurred: occurred(),
            observation: note(),
        }),
    ));
    for (kind, action) in [
        (
            "acceptance",
            TreatmentAction::SetAcceptance {
                selection: TreatmentSelection::new(vec![id(3)]).unwrap(),
                status: AcceptanceStatus::Accepted,
            },
        ),
        (
            "hold_place",
            TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![id(3)]).unwrap(),
            },
        ),
        (
            "hold_release",
            TreatmentAction::ReleaseHold {
                hold: id(7),
                expected_revision: rev(1),
            },
        ),
    ] {
        requests.push((
            kind,
            HandlingCommand::Treatment(TreatmentCommand {
                expected_stock_revision: rev(2),
                expected_catalog_revision: rev(8),
                action,
                occurred: occurred(),
                observation: note(),
            }),
        ));
    }
    requests.push((
        "work_replan",
        HandlingCommand::Replan(ReplanCommand {
            expected_stock_revision: rev(2),
            expected_catalog_revision: rev(8),
            requirement: id(9),
            expected_work_revision: rev(1),
            target: id(5),
            observation: note(),
        }),
    ));
    requests.push((
        "work_new",
        HandlingCommand::NewWork(NewWorkCommand {
            expected_stock_revision: rev(2),
            expected_catalog_revision: rev(8),
            groups: vec![id(3)],
            target: id(5),
            observation: note(),
        }),
    ));
    for (kind, action) in [
        (
            "discrepancy_open",
            DiscrepancyAction::Open {
                trace: id(10),
                group: Some(id(3)),
                observation: note(),
            },
        ),
        (
            "discrepancy_resolve",
            DiscrepancyAction::Resolve {
                discrepancy: id(11),
                expected_revision: rev(1),
                resolution: note(),
            },
        ),
    ] {
        requests.push((
            kind,
            HandlingCommand::Discrepancy(DiscrepancyCommand {
                expected_stock_revision: rev(2),
                expected_catalog_revision: rev(8),
                action,
            }),
        ));
    }
    for (kind, command) in requests {
        let (_, value) = parsed(&HubSubmittedRequest::Handling(&command));
        assert_eq!(value["kind"], kind);
        assert!(!value["input"].as_object().unwrap().is_empty());
    }
    let lines = DispatchSelection::new(vec![DispatchLineInput {
        group: id(3),
        expected_group_revision: rev(1),
        packs: packs(20),
    }])
    .unwrap();
    for (kind, action) in [
        (
            "dispatch_prepare",
            DispatchAction::Prepare {
                recipient: id(5),
                lines: lines.clone(),
            },
        ),
        (
            "dispatch_revise",
            DispatchAction::Revise {
                preparation: id(12),
                expected_revision: rev(1),
                recipient: id(5),
                lines: lines.clone(),
            },
        ),
        (
            "dispatch_issue",
            DispatchAction::Issue {
                preparation: id(12),
                expected_revision: rev(1),
            },
        ),
        (
            "dispatch_replace",
            DispatchAction::Replace {
                preparation: id(12),
                expected_revision: rev(1),
                recipient: id(5),
                lines,
            },
        ),
        (
            "dispatch_unused",
            DispatchAction::MarkUnused {
                preparation: id(12),
                expected_revision: rev(1),
            },
        ),
    ] {
        let command = DispatchCommand {
            expected_stock_revision: rev(2),
            expected_catalog_revision: rev(8),
            action,
            observation: note(),
        };
        let (_, value) = parsed(&HubSubmittedRequest::Dispatch(&command));
        assert_eq!(value["kind"], kind);
        assert_eq!(value["input"]["observation"], note().as_str());
    }
    let departure = DepartureCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: rev(8),
        preparation: id(12),
        expected_preparation_revision: rev(1),
        identification: id(13),
        occurred: occurred(),
        observation: note(),
    };
    let (_, value) = parsed(&HubSubmittedRequest::Departure(&departure));
    assert_eq!(value["kind"], "departure");
    assert_eq!(value["input"]["identification"], id(13).to_hex());
}

#[test]
fn initial_ownership_and_integer_limits_are_exact_public_data() {
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let owner = HubOwnership::new(HubId::from_bytes([1; 16]).unwrap(), actor);
    let (_, value) = parsed(&HubSubmittedRequest::Initialize(&owner));
    assert_eq!(value["input"]["owner"], actor.to_hex());
    assert_eq!(value["input"]["id"], id(1).to_hex());
    let reference = ReferenceCommand {
        expected_catalog_revision: rev(HubRevision::MAXIMUM),
        mutation: ReferenceMutation::Create {
            id: id(2),
            value: ReferenceValue::Product {
                label: HubLabel::new("Exact integer").unwrap(),
            },
        },
    };
    let (_, value) = parsed(&HubSubmittedRequest::Reference(&reference));
    assert_eq!(
        value["input"]["expected_catalog_revision"].as_u64(),
        Some(HubRevision::MAXIMUM)
    );
}

#[test]
fn byte_capacity_is_enforced_before_an_oversized_archive_can_escape() {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit: 4,
    };
    writer.write_all("éé".as_bytes()).unwrap();
    assert!(writer.write_all("é".as_bytes()).is_err());
    assert_eq!(writer.bytes.len(), 4);
    let command = HandlingCommand::NewWork(NewWorkCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: rev(8),
        groups: vec![id(3); HUB_SUBMITTED_REQUEST_MAX_BYTES / 32],
        target: id(5),
        observation: note(),
    });
    assert_eq!(
        HubSubmittedRequest::Handling(&command).archive(),
        Err(HubRequestArchiveError::Capacity)
    );
}

#[test]
fn maximum_valid_group_and_document_inputs_fit_the_archive_without_truncation() {
    use harvestcircle_domain::hub::{
        dispatch::HUB_DISPATCH_LINES, movement::HUB_REQUIREMENT_GROUPS,
        treatment::HUB_TREATMENT_SELECTION_GROUPS,
    };
    let ids = |count: usize| {
        (1..=count)
            .map(|n| HubRecordId::from_bytes((n as u128).to_be_bytes()).unwrap())
            .collect::<Vec<_>>()
    };
    let large_note = SourceProvenance::new("\"".repeat(512)).unwrap();
    let work = NewWorkCommand {
        expected_stock_revision: rev(HubRevision::MAXIMUM),
        expected_catalog_revision: rev(HubRevision::MAXIMUM),
        groups: ids(HUB_REQUIREMENT_GROUPS),
        target: id(5),
        observation: large_note.clone(),
    };
    work.validate_shape().unwrap();
    let (_, value) = parsed(&HubSubmittedRequest::Handling(&HandlingCommand::NewWork(
        work,
    )));
    assert_eq!(
        value["input"]["new_work"]["groups"]
            .as_array()
            .unwrap()
            .len(),
        HUB_REQUIREMENT_GROUPS
    );
    let treatment = HandlingCommand::Treatment(TreatmentCommand {
        expected_stock_revision: rev(HubRevision::MAXIMUM),
        expected_catalog_revision: rev(HubRevision::MAXIMUM),
        action: TreatmentAction::PlaceHold {
            selection: TreatmentSelection::new(ids(HUB_TREATMENT_SELECTION_GROUPS)).unwrap(),
        },
        occurred: occurred(),
        observation: large_note.clone(),
    });
    let (archive, _) = parsed(&HubSubmittedRequest::Handling(&treatment));
    assert!(archive.as_bytes().len() < HUB_SUBMITTED_REQUEST_MAX_BYTES);
    let selection = DispatchSelection::new(
        ids(HUB_DISPATCH_LINES)
            .into_iter()
            .map(|group| DispatchLineInput {
                group,
                expected_group_revision: rev(HubRevision::MAXIMUM),
                packs: packs(1_000_000),
            })
            .collect(),
    )
    .unwrap();
    let dispatch = DispatchCommand {
        expected_stock_revision: rev(HubRevision::MAXIMUM),
        expected_catalog_revision: rev(HubRevision::MAXIMUM),
        action: DispatchAction::Prepare {
            recipient: id(5),
            lines: selection,
        },
        observation: large_note,
    };
    let (_, value) = parsed(&HubSubmittedRequest::Dispatch(&dispatch));
    assert_eq!(
        value["input"]["action"]["prepare"]["lines"]
            .as_array()
            .unwrap()
            .len(),
        HUB_DISPATCH_LINES
    );
}
