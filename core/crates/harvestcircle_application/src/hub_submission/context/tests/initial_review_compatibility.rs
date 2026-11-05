use super::*;
use harvestcircle_domain::{
    UnixTimestamp,
    hub::{
        actuals::{
            ActualNewDepartureCommand, ActualPhysicalBirth, ActualPhysicalBirthKind,
            ActualReportCommand, ActualReportRecord, ActualReportSources, ActualReviewFacts,
            HistoricalActualPosition, HistoricalActualSource,
        },
        disposition::{DispositionCommand, DispositionKind},
        movement::PhysicalStockGroup,
        portions::{PhysicalLineage, PhysicalLineageNode},
        reference::{LocationPurpose, SourceProvenance, WholePacks},
        stock::{Occurrence, ReceiptIdentities},
        treatment::{AcceptanceDecision, AcceptanceStatus, Hold, TreatmentStamp, TreatmentState},
    },
};
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Retained fictional native capture compatibility evidence").unwrap()
}
fn fixture() -> (
    PhysicalStockGroup,
    PhysicalLineage,
    TreatmentState,
    ActualReportRecord,
    HistoricalActualSource,
    ActualPhysicalBirth,
) {
    let source_operation = HubRecordId::from_hex("0000000000007000800000000000000b").unwrap();
    let ids = ReceiptIdentities::from_operation_bytes(source_operation).unwrap();
    let group = PhysicalStockGroup {
        id: ids.group,
        trace: ids.trace,
        variant: id(4),
        parent: None,
        identification: None,
        location: id(2),
        packs: WholePacks::from_value(20).unwrap(),
        revision: rev(1),
        causal_floor: Some(at(100)),
        work: vec![],
    };
    let graph = PhysicalLineage::new(
        hub().id(),
        ids.trace,
        id(4),
        rev(10),
        vec![PhysicalLineageNode {
            recovery: None,
            id: ids.group,
            parent: None,
            trace: ids.trace,
            variant: id(4),
            created_revision: rev(2),
            packs: group.packs,
        }],
    )
    .unwrap();
    let scope = |r| {
        graph
            .restore_scope(
                rev(r),
                graph.capture(&[group.id]).unwrap().anchors().to_vec(),
            )
            .unwrap()
    };
    let stamp = TreatmentStamp {
        id: id(20),
        stock_revision: rev(3),
        actor: owner(),
        basis: harvestcircle_domain::hub::reference::SampleHandlingBasis::WHOLE_PACK_V1,
        recorded_at: at(110),
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    let state = TreatmentState::restore(
        &graph,
        vec![AcceptanceDecision {
            stamp: stamp.clone(),
            scope: scope(2),
            status: AcceptanceStatus::Accepted,
        }],
        vec![Hold {
            stamp: TreatmentStamp {
                id: id(21),
                stock_revision: rev(5),
                ..stamp
            },
            scope: scope(4),
            release: None,
        }],
    )
    .unwrap();
    let report = ActualReportCommand {
        variant: Some(id(4)),
        recipient: Some(id(9)),
        packs: Some(WholePacks::from_value(2).unwrap()),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Known(at(120)),
        observation: note(),
    }
    .preserve(
        hub().id(),
        rev(5),
        rev(1),
        HubRecordId::from_hex("0000000000007000800000000000000c").unwrap(),
        owner(),
        at(150),
    )
    .unwrap();
    let source = HistoricalActualSource {
        operation: source_operation,
        actor: owner(),
        hub: hub().id(),
        stock_revision: rev(2),
        catalog_revision: rev(1),
        group: HistoricalActualPosition::from(&group),
        location: ReferenceRecord {
            id: id(2),
            revision: rev(1),
            value: ReferenceValue::Location {
                label: HubLabel::new("Retained original location").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        },
        occurred: Occurrence::Known(at(100)),
        recorded_at: at(100),
    };
    let birth = ActualPhysicalBirth {
        kind: ActualPhysicalBirthKind::Receipt,
        operation: source_operation,
        hub: hub().id(),
        trace: ids.trace,
        variant: id(4),
        group: ids.group,
        parent: None,
        stock_revision: rev(2),
        packs: group.packs,
        actor: owner(),
        occurred: Occurrence::Known(at(100)),
        recorded_at: at(100),
    };
    (group, graph, state, report, source, birth)
}
#[test]
fn legacy_disposition_and_actual_review_capture_bytes_stay_frozen() {
    let (group, graph, state, report, source, birth) = fixture();
    assert_eq!(
        serde_json::to_vec(&TreatmentState::default()).unwrap(),
        br#"{"decisions":[],"holds":[]}"#
    );
    let disposition = DispositionCommand {
        expected_stock_revision: rev(10),
        expected_catalog_revision: rev(1),
        group: group.id,
        expected_group_revision: group.revision,
        packs: group.packs,
        kind: DispositionKind::Destroyed,
        occurred: Occurrence::Known(at(160)),
        observation: note(),
    };
    let material = [HubSubmittedMaterial::Disposition(Box::new(
        HubSubmittedDisposition {
            id: group.id,
            group: group.clone(),
            lineage: graph.clone(),
            treatment: state.clone(),
            uncertain_submissions: true,
            unresolved_actuals: true,
        },
    ))];
    let context = HubSubmissionContext {
        hub: hub(),
        catalog_revision: Some(rev(1)),
        stock_revision: Some(rev(10)),
        material: &material,
    };
    let capture = HubSubmittedRequest::Disposition(&disposition)
        .capture(&operation(), scope(7), &context)
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(capture.as_bytes())),
        "638ebc07f29dd02fb088db787d6b7b836b214faa55f220cfe99bb01f07e1b91e"
    );
    assert_eq!(
        capture
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "3e4b726cfba4cac4e4fe812d4b077e9992181feb75e4c81bbe75816c9889aecd"
    );
    let command = ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report: report.id,
            expected_stock_revision: rev(10),
            expected_catalog_revision: rev(1),
            group: group.id,
            expected_group_revision: group.revision,
            packs: WholePacks::from_value(2).unwrap(),
            recipient: id(9),
            occurred: Occurrence::Known(at(120)),
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: source.operation,
        source_group: source.group.id,
        duplicate_review_basis: note(),
    };
    let material = [HubSubmittedMaterial::ActualReview(Box::new(
        HubSubmittedActualReview {
            id: report.id,
            report,
            group,
            lineage: graph,
            births: vec![birth],
            treatment: state,
            source,
            activity: vec![],
            known_departures: vec![],
            uncertain_submissions: true,
            interpretations: None,
        },
    ))];
    let context = HubSubmissionContext {
        material: &material,
        ..context
    };
    let capture = HubSubmittedRequest::ActualReview(&command)
        .capture(&operation(), scope(7), &context)
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(capture.as_bytes())),
        "64a69800fdace49e6698f219db99242d7e5f6787d06ce5151eb1fdd17b81811e"
    );
    assert_eq!(
        capture
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "cfdbc82ad9405013f7a38bcfdaeb27b1008d70821e04d4068eb62b2f77b3a9cc"
    );
}

#[test]
fn nonempty_birth_review_is_included_in_native_submission_material() {
    use harvestcircle_domain::hub::treatment::InitialGroupReview;
    let (mut group, graph, old, _, _, _) = fixture();
    group.packs = WholePacks::from_value(19).unwrap();
    let child = id(30);
    let mut nodes = graph.nodes().to_vec();
    nodes[0].packs = group.packs;
    nodes.push(PhysicalLineageNode {
        recovery: None,
        id: child,
        parent: Some(group.id),
        trace: group.trace,
        variant: group.variant,
        created_revision: rev(11),
        packs: WholePacks::from_value(1).unwrap(),
    });
    let graph =
        PhysicalLineage::new(graph.hub(), graph.trace(), graph.variant(), rev(11), nodes).unwrap();
    let review = InitialGroupReview {
        stamp: TreatmentStamp {
            id: id(31),
            stock_revision: rev(11),
            actor: owner(),
            basis: old.decisions()[0].stamp.basis,
            recorded_at: at(200),
            occurred: Occurrence::Known(at(200)),
            observation: note(),
        },
        scope: graph.capture(&[child]).unwrap(),
    };
    let state = TreatmentState::restore_with_initial_reviews(
        &graph,
        old.decisions().to_vec(),
        old.holds().to_vec(),
        vec![review],
    )
    .unwrap();
    let command = DispositionCommand {
        expected_stock_revision: rev(11),
        expected_catalog_revision: rev(1),
        group: group.id,
        expected_group_revision: group.revision,
        packs: group.packs,
        kind: DispositionKind::Destroyed,
        occurred: Occurrence::Known(at(201)),
        observation: note(),
    };
    let material = HubSubmittedDisposition {
        id: group.id,
        group,
        lineage: graph,
        treatment: state,
        uncertain_submissions: true,
        unresolved_actuals: true,
    };
    let capture = |material: HubSubmittedDisposition| {
        let rows = [HubSubmittedMaterial::Disposition(Box::new(material))];
        HubSubmittedRequest::Disposition(&command)
            .capture(
                &operation(),
                scope(7),
                &HubSubmissionContext {
                    hub: hub(),
                    catalog_revision: Some(rev(1)),
                    stock_revision: Some(rev(11)),
                    material: &rows,
                },
            )
            .unwrap()
    };
    let encoded = capture(material.clone());
    let value: serde_json::Value = serde_json::from_slice(encoded.as_bytes()).unwrap();
    let reviews = &value["material"][0]["disposition"]["treatment"]["initial_reviews"];
    assert_eq!(reviews.as_array().unwrap().len(), 1);
    assert_eq!(reviews[0]["stamp"]["stock_revision"], 11);
    assert!(reviews[0].get("status").is_none());
    assert_eq!(reviews[0]["scope"]["anchors"][0]["group"], child.to_hex());
    let without = capture(HubSubmittedDisposition {
        treatment: old,
        ..material
    });
    assert_eq!(encoded.input_digest(), without.input_digest());
    assert_ne!(encoded.digest(), without.digest());
    assert_ne!(encoded.as_bytes(), without.as_bytes());
}
