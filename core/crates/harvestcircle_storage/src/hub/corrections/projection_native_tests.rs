//! Seeded output-projection fixtures only. The departure and observed claim use
//! actual native APIs; synthetic correction roots deliberately have no fresh
//! in-memory grant, full decision seal, stock events or valid current stock state.
//! These tests qualify metadata readers, never found posting or funding effects.
use super::birth::{BirthData, FoundBirthProjection};
use super::*;
use crate::hub::stock::tests::{Fixture, id, operation, owner};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::{
    actuals::{ActualPhysicalBirth, ActualPhysicalBirthKind},
    corrections::{
        CorrectionObservedFacts, FoundStockCommand, FoundStockInput, PhysicalDebitKey,
        found_stock_group_id,
    },
    movement::{PhysicalIdentification, PhysicalStockGroup},
    portions::{PhysicalLineage, PhysicalLineageNode, PhysicalRecoveryOrigin},
    reference::{HubLabel, ReferenceKind, SampleHandlingBasis, SourceProvenance, WholePacks},
    stock::Occurrence,
    treatment::{InitialGroupReview, TreatmentStamp, treatment_record_id},
};
use sqlx::Connection;
pub(in crate::hub) mod found_review_tests;
mod quantity_tests;
mod return_current_tests;
pub(super) mod return_funding_tests;
mod target_funding_tests;

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note(text: &str) -> SourceProvenance {
    SourceProvenance::new(text).unwrap()
}

struct World {
    db: Database,
    fixture: Fixture,
    hub: HubId,
    op: OperationRecord,
    graph: PhysicalLineage,
    before: PhysicalLineage,
    projection: FoundBirthProjection,
    review: InitialGroupReview,
    extra_report: Option<HubRecordId>,
}

pub(in crate::hub) async fn actual_debit(
    db: &Database,
) -> (PhysicalStockGroup, DurableRequestId, u32) {
    actual_debit_with_early_return(db, false).await
}

async fn actual_debit_with_early_return(
    db: &Database,
    early_return: bool,
) -> (PhysicalStockGroup, DurableRequestId, u32) {
    actual_debit_configuration(db, early_return, None).await
}

async fn actual_debit_configuration(
    db: &Database,
    early_return: bool,
    hold_before_debit: Option<bool>,
) -> (PhysicalStockGroup, DurableRequestId, u32) {
    actual_debit_hold_configuration(db, early_return, hold_before_debit, false).await
}

pub(in crate::hub::corrections) async fn actual_debit_unknown_hold(
    db: &Database,
) -> (PhysicalStockGroup, DurableRequestId, u32) {
    actual_debit_hold_configuration(db, false, Some(true), true).await
}

async fn actual_debit_hold_configuration(
    db: &Database,
    early_return: bool,
    hold_before_debit: Option<bool>,
    unknown_hold: bool,
) -> (PhysicalStockGroup, DurableRequestId, u32) {
    use harvestcircle_domain::hub::actuals::{
        ActualNewDepartureCommand, ActualReportCommand, ActualReportSources, ActualReviewFacts,
    };
    let catalog = crate::hub::stock::tests::setup(db).await;
    let mut receipt = crate::hub::stock::tests::command(&catalog);
    receipt.occurred = Occurrence::Known(at(100));
    let intake = db
        .apply_hub_receipt(owner(), operation(1), receipt, at(100))
        .await
        .unwrap();
    let group = intake.original.position.group;
    if early_return {
        return_funding_tests::record_return(db, None, 120).await;
    }
    let runtime = db.begin_hub_runtime(at(140)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(140),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    if hold_before_debit == Some(true) {
        if unknown_hold {
            hold_actual_group_occurrence(db, scope, group, Occurrence::Unknown, 145).await;
        } else {
            hold_actual_group(db, scope, group, 105, 145).await;
        }
    }
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: None,
                recipient: None,
                packs: Some(WholePacks::from_value(2).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note(
                    "Two packs already departed; independently reviewed before found observation",
                ),
            })),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("actual report")
    };
    let view = db.hub_group(owner(), group).await.unwrap();
    let crate::HubMutationResult::ActualReviewed(review) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(ActualNewDepartureCommand {
                facts: ActualReviewFacts {
                    report: report.original.id,
                    expected_stock_revision: view.revision,
                    expected_catalog_revision: view.catalog_revision,
                    group,
                    expected_group_revision: view.group.revision,
                    packs: WholePacks::from_value(2).unwrap(),
                    recipient: id(3),
                    occurred: Occurrence::Known(at(110)),
                    source_evidence: note("Original receiving cohort independently established"),
                    quantity_evidence: note("Two whole packs verified"),
                    recipient_evidence: note("Actual recipient confirmed"),
                    timing_evidence: note("Observed before native report"),
                },
                source_operation: crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
                source_group: group,
                duplicate_review_basis: note(
                    "Complete native history reviewed; no prior duplicate",
                ),
            })),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("actual review")
    };
    let crate::HubActualReviewOriginal::Departure(plan) = review.original else {
        panic!("physical original")
    };
    if hold_before_debit == Some(false) {
        hold_actual_group(db, scope, group, 165, 165).await;
    }
    (plan.reduction.before, review.operation, 2)
}

async fn hold_actual_group(
    db: &Database,
    scope: HubSubmissionScope,
    group: HubRecordId,
    occurred: i64,
    recorded: i64,
) {
    hold_actual_group_occurrence(db, scope, group, Occurrence::Known(at(occurred)), recorded).await;
}

async fn hold_actual_group_occurrence(
    db: &Database,
    scope: HubSubmissionScope,
    group: HubRecordId,
    occurred: Occurrence,
    recorded: i64,
) {
    use harvestcircle_domain::hub::{
        movement::HandlingCommand,
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let current = db.hub_group(owner(), group).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(90),
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
            },
            occurred,
            observation: note("Explicit native cohort hold at its original capture cut"),
        }))),
        at(recorded),
    )
    .await
    .unwrap();
}

async fn seed(omit_allocation: bool, omit_review: bool) -> World {
    seed_debit(omit_allocation, omit_review, false).await
}

async fn seed_debit(omit_allocation: bool, omit_review: bool, actual: bool) -> World {
    seed_debit_binding(omit_allocation, omit_review, actual, 0).await
}

async fn seed_debit_binding(
    omit_allocation: bool,
    omit_review: bool,
    actual: bool,
    fault: u8,
) -> World {
    seed_options(omit_allocation, omit_review, actual, fault, false).await
}

async fn seed_options(
    omit_allocation: bool,
    omit_review: bool,
    actual: bool,
    fault: u8,
    extra_report: bool,
) -> World {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (original, debit_operation, debit_packs) = if actual {
        actual_debit(&db).await
    } else {
        let (_, departure) =
            crate::hub::handling::departure::tests::custody_departure_fixture(&db).await;
        let before = departure.record.consumed[0].before.clone();
        let packs = before.packs.value();
        (before, departure.operation, packs)
    };
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let target = crate::hub::actuals::common::operation_id(&debit_operation).unwrap();
    let input = FoundStockInput {
        debit: PhysicalDebitKey {
            operation: target,
            group: original.id,
        },
        packs: WholePacks::from_value(2).unwrap(),
        location: id(6),
        identification: PhysicalIdentification {
            label: HubLabel::new("Distinct found blue crate").unwrap(),
            observation: note("Observed separately outside the post-debit remainder"),
        },
        cohort_evidence: note("Independent physical identification and source investigation"),
    };
    let mut allocations = vec![input.clone()];
    if omit_allocation {
        allocations.push(input.clone());
    }
    let observation = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: target,
            target_group: original.id,
            allocations: allocations.clone(),
            basis: note("Original departure overstatement independently reviewed"),
        },
    };
    let runtime = db.begin_hub_runtime(at(170)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(170),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let crate::HubMutationResult::CorrectionObserved(saved) = db
        .submit_hub_mutation(
            scope,
            operation(130),
            HubMutation::CorrectionObservation(Box::new(observation.clone())),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("recorded observation")
    };
    let extra_report = if extra_report {
        use harvestcircle_domain::hub::actuals::{ActualReportCommand, ActualReportSources};
        let crate::HubMutationResult::ActualReported(report)=db.submit_hub_mutation(scope,operation(132),HubMutation::ActualReport(Box::new(ActualReportCommand {
            variant:None,recipient:None,packs:Some(WholePacks::from_value(6).unwrap()),sources:ActualReportSources::Unknown,occurred:Occurrence::Unknown,
            observation:note("Independent corrected line total; factual report only, no permission to debit"),
        })),at(185)).await.unwrap() else {panic!("raw quantity report")};
        Some(report.original.id)
    } else {
        None
    };
    let original_trace = original.trace;
    let debit_id = debit_operation.clone();
    let (before, catalog, debit) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let state = stock::read::state_on(tx, hub).await?;
                let catalog = reference::load_catalog_on(tx, hub).await?;
                let graph =
                    handling::lineage::load_on(tx, hub, original_trace, &catalog, state.revision)
                        .await?
                        .graph;
                let debit = stock::read::operation_on(tx, hub, &debit_id)
                    .await?
                    .unwrap();
                Ok::<_, SafeError>((graph, catalog, debit))
            })
        })
        .await
        .unwrap();
    let found_id = operation(200);
    let own = crate::hub::actuals::common::operation_id(&found_id).unwrap();
    let child = found_stock_group_id(own, 0).unwrap();
    let revision =
        HubRevision::from_value(before.revision().value().checked_add(1).unwrap()).unwrap();
    let location = catalog
        .require_kind(input.location, ReferenceKind::Location)
        .unwrap()
        .clone();
    let command = FoundStockCommand {
        target_operation: target,
        target_group: original.id,
        expected_stock_revision: before.revision(),
        expected_catalog_revision: catalog.revision(),
        allocations,
        basis: match &observation.facts {
            CorrectionObservedFacts::Found { basis, .. } => basis.clone(),
            _ => unreachable!(),
        },
    };
    let op = OperationRecord {
        id: found_id,
        kind: "correction_found".into(),
        actor: owner(),
        hash: command.fingerprint(hub, owner(), own).unwrap(),
        catalog_revision: catalog.revision(),
        revision,
        result: id(200),
        recorded_at: at(200),
    };
    let recovery = PhysicalRecoveryOrigin {
        debit_operation: target,
        debit_revision: debit.revision,
    };
    let group = PhysicalStockGroup {
        id: child,
        trace: original.trace,
        variant: original.variant,
        parent: Some(original.id),
        identification: Some(input.identification.clone()),
        location: input.location,
        packs: input.packs,
        revision: HubRevision::INITIAL,
        causal_floor: Some(op.recorded_at),
        work: Vec::new(),
    };
    let projection = FoundBirthProjection {
        group,
        location,
        birth: ActualPhysicalBirth {
            kind: ActualPhysicalBirthKind::FoundRemainder { allocation: 0 },
            operation: own,
            hub,
            trace: original.trace,
            variant: original.variant,
            group: child,
            parent: Some(original.id),
            stock_revision: revision,
            packs: input.packs,
            actor: owner(),
            occurred: Occurrence::Known(op.recorded_at),
            recorded_at: op.recorded_at,
        },
        recovery,
        basis: command.basis.clone(),
        cohort_evidence: input.cohort_evidence.clone(),
        observation_operation: saved.original.operation,
        target_operation: target,
        target_group: original.id,
        allocation: 0,
    };
    let mut nodes = before.nodes().to_vec();
    nodes.push(PhysicalLineageNode {
        id: child,
        parent: Some(original.id),
        trace: original.trace,
        variant: original.variant,
        created_revision: revision,
        packs: input.packs,
        recovery: Some(recovery),
    });
    let graph =
        PhysicalLineage::new(hub, original.trace, original.variant, revision, nodes).unwrap();
    let review = InitialGroupReview {
        stamp: TreatmentStamp {
            id: treatment_record_id(own).unwrap(),
            stock_revision: revision,
            actor: owner(),
            basis: SampleHandlingBasis::WHOLE_PACK_V1,
            recorded_at: op.recorded_at,
            occurred: Occurrence::Known(op.recorded_at),
            observation: command.basis.clone(),
        },
        scope: graph.capture(&[child]).unwrap(),
    };
    let mut raw = fixture.raw().await;
    let result_query = if actual {
        "SELECT result_hash FROM hub_actual_reviews WHERE operation_id=?"
    } else {
        "SELECT result_hash FROM hub_departures WHERE operation_id=?"
    };
    let debit_result: Vec<u8> = sqlx::query_scalar(result_query)
        .bind(debit.id.as_str())
        .fetch_one(&mut raw)
        .await
        .unwrap();
    let mut data = BirthData {
        schema: 1,
        location_revision: projection.location.revision.value(),
        debit_packs,
        debit_before_packs: original.packs.value(),
        debit_before_group_revision: original.revision.value(),
        debit_after_group_revision: original.revision.value().checked_add(1).unwrap(),
        debit_result_hash: debit_result.try_into().unwrap(),
        debit_kind: debit.kind,
        debit_actor: debit.actor.to_hex(),
        debit_input_hash: debit.hash,
        debit_catalog_revision: debit.catalog_revision.value(),
        debit_result: debit.result.to_hex(),
        debit_recorded_at: debit.recorded_at.as_seconds(),
    };
    // Re-seal the intentionally wrong captured fact: native correlation must
    // reject it even when the separate projection seal itself is consistent.
    match fault {
        0 => {}
        1 => data.debit_result = id(250).to_hex(),
        2 => data.debit_input_hash[0] ^= 1,
        3 => data.debit_result_hash[0] ^= 1,
        _ => panic!("unknown debit fault"),
    }
    let birth_seal = super::birth::seal(&op, hub, &data, &projection).unwrap();
    let review_seal =
        super::initial_review::seal(&op, hub, &review, &[(child, birth_seal)]).unwrap();
    // SQL fixtures cannot manufacture FreshAdmission: this body is deliberately
    // not a valid saved request, and no product writer consumes or decodes it.
    sqlx::query("INSERT INTO hub_submissions (operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) VALUES (?,?,?,'correction_found',?,?,?,?,1,?,?,200)")
        .bind(op.id.as_str()).bind(hub.as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(scope.runtime.as_bytes().as_slice()).bind(scope.generation.value().to_be_bytes().as_slice()).bind([1_u8;32].as_slice()).bind(op.hash.as_slice()).bind(b"{}".as_slice()).bind([1_u8;32].as_slice()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,'correction_found',?,?,?,?,200)")
        .bind(op.id.as_str()).bind(hub.as_bytes().as_slice()).bind(owner().as_bytes().as_slice()).bind(op.hash.as_slice()).bind(i64::try_from(op.catalog_revision.value()).unwrap()).bind(i64::try_from(revision.value()).unwrap()).bind(op.result.as_bytes().as_slice()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,observation_operation_id,payload_schema,payload,result_hash) VALUES (?,?,'found',?,'line',?,?,1,?,?)")
        .bind(op.result.as_bytes().as_slice()).bind(op.id.as_str()).bind(debit_operation.as_str()).bind(original.id.as_bytes().as_slice()).bind(operation(130).as_str()).bind(b"{}".as_slice()).bind([2_u8;32].as_slice()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(child.as_bytes().as_slice())
        .bind(original.trace.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,?,?,?,?)")
        .bind(child.as_bytes().as_slice()).bind(original.id.as_bytes().as_slice()).bind(input.identification.label.as_str()).bind(input.identification.observation.as_str()).bind(op.id.as_str()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,2,1)").bind(child.as_bytes().as_slice()).bind(input.location.as_bytes().as_slice()).bind(i64::try_from(projection.location.revision.value()).unwrap()).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_correction_found_allocations (operation_id,allocation,debit_operation_id,debit_group_id,debit_stock_revision,child_group_id,packs,birth_schema,birth_payload,projection_hash) VALUES (?,0,?,?,?,?,2,1,?,?)")
        .bind(op.id.as_str()).bind(debit_operation.as_str()).bind(original.id.as_bytes().as_slice()).bind(i64::try_from(debit.revision.value()).unwrap()).bind(child.as_bytes().as_slice()).bind(serde_json::to_vec(&data).unwrap()).bind(birth_seal.as_slice()).execute(&mut raw).await.unwrap();
    if !omit_review {
        sqlx::query("INSERT INTO hub_correction_initial_reviews (operation_id,record_id,trace_id,capture_stock_revision,anchor_count,payload_schema,payload,result_hash) VALUES (?,?,?,?,1,1,?,?)")
        .bind(op.id.as_str()).bind(review.stamp.id.as_bytes().as_slice()).bind(original.trace.as_bytes().as_slice()).bind(i64::try_from(revision.value()).unwrap()).bind(serde_json::to_vec(&review).unwrap()).bind(review_seal.as_slice()).execute(&mut raw).await.unwrap();
        sqlx::query("INSERT INTO hub_correction_initial_review_anchors (operation_id,group_id,created_revision,packs) VALUES (?,?,?,2)")
        .bind(op.id.as_str()).bind(child.as_bytes().as_slice()).bind(i64::try_from(revision.value()).unwrap()).execute(&mut raw).await.unwrap();
    }
    raw.close().await.unwrap();
    World {
        db,
        fixture,
        hub,
        op,
        graph,
        before,
        projection,
        review,
        extra_report,
    }
}

#[tokio::test]
async fn seeded_birth_and_initial_review_read_at_their_exact_cut_without_current_stock_authority() {
    let world = seed(false, false).await;
    let expected = world.projection.clone();
    let review = world.review.clone();
    let hub = world.hub;
    let operation = world.op.id.clone();
    let graph = world.graph.clone();
    let before = world.before.clone();
    let group = expected.group.id;
    let recovery = expected.recovery;
    let result = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .unwrap();
                let birth = super::birth::birth_on(tx, hub, &op, group).await?;
                let reviews = super::initial_review::through_on(tx, hub, &graph).await?;
                let historical =
                    handling::lineage::historical_on(tx, hub, graph.trace(), graph.revision())
                        .await?;
                assert_eq!(historical.node(group).unwrap().recovery, Some(recovery));
                assert!(
                    super::initial_review::through_on(tx, hub, &before)
                        .await?
                        .is_empty()
                );
                Ok::<_, SafeError>((birth, reviews))
            })
        })
        .await
        .unwrap();
    assert_eq!(result.0, expected);
    assert_eq!(result.1, vec![review]);
    assert_eq!(
        world
            .db
            .hub_stock_page(owner(), crate::hub::stock::tests::query())
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
}

#[tokio::test]
async fn seeded_projection_rejects_an_omitted_observed_allocation_without_mutating_rows() {
    let world = seed(true, false).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let hub = world.hub;
    let operation = world.op.id.clone();
    let group = world.projection.group.id;
    let result = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .unwrap();
                super::birth::birth_on(tx, hub, &op, group).await
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        result.unwrap_err().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
}

#[tokio::test]
async fn seeded_birth_original_survives_depleted_current_positions() {
    let world = seed(false, false).await;
    let mut raw = world.fixture.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=0 WHERE group_id=?")
        .bind(world.projection.group.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_stock_groups SET revision=2 WHERE group_id=?")
        .bind(world.projection.group.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let hub = world.hub;
    let operation = world.op.id.clone();
    let group = world.projection.group.id;
    let birth = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .unwrap();
                super::birth::birth_on(tx, hub, &op, group).await
            })
        })
        .await
        .unwrap();
    assert_eq!(birth, world.projection);
    assert_eq!(birth.group.packs.value(), 2);
    assert_eq!(birth.group.revision, HubRevision::INITIAL);
}

#[tokio::test]
async fn seeded_found_birth_cannot_inherit_prior_treatment_when_initial_review_is_missing() {
    let world = seed(false, true).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let hub = world.hub;
    let graph = world.graph.clone();
    let old = world.before.clone();
    let result = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                assert!(
                    super::initial_review::through_on(tx, hub, &old)
                        .await?
                        .is_empty()
                );
                super::initial_review::through_on(tx, hub, &graph).await
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        result.unwrap_err().code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
}

#[tokio::test]
async fn seeded_found_source_uses_the_sealed_birth_instead_of_current_positions() {
    let world = seed(false, false).await;
    let mut raw = world.fixture.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=0,revision=2 WHERE group_id=?")
        .bind(world.projection.group.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_stock_groups SET revision=2 WHERE group_id=?")
        .bind(world.projection.group.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let hub = world.hub;
    let operation = world.op.id.clone();
    let group = world.projection.group.id;
    let trace = world.graph.trace();
    let source = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let population = handling::read::trace_on(tx, hub, trace).await?;
                handling::actuals::source_original_on(tx, hub, &population, &operation, group).await
            })
        })
        .await
        .unwrap();
    assert_eq!(source.operation, world.projection.birth.operation);
    assert_eq!(source.stock_revision, world.projection.birth.stock_revision);
    assert_eq!(source.catalog_revision, world.op.catalog_revision);
    assert_eq!(source.group.id, group);
    assert_eq!(source.group.parent, world.projection.group.parent);
    assert_eq!(source.group.packs, world.projection.group.packs);
    assert_eq!(source.group.revision, HubRevision::INITIAL);
    assert_eq!(source.location, world.projection.location);
    assert_eq!(source.occurred, Occurrence::Known(world.op.recorded_at));
    assert_eq!(source.recorded_at, world.op.recorded_at);
}

#[tokio::test]
async fn seeded_found_projection_correlates_a_real_reviewed_actual_debit() {
    let world = seed_debit(false, false, true).await;
    let hub = world.hub;
    let operation = world.op.id.clone();
    let graph = world.graph.clone();
    let group = world.projection.group.id;
    let (birth, reviews) = world
        .db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let op = stock::read::operation_on(tx, hub, &operation)
                    .await?
                    .unwrap();
                let birth = super::birth::birth_on(tx, hub, &op, group).await?;
                let reviews = super::initial_review::through_on(tx, hub, &graph).await?;
                Ok::<_, SafeError>((birth, reviews))
            })
        })
        .await
        .unwrap();
    assert_eq!(birth, world.projection);
    assert_eq!(reviews, vec![world.review]);
}

#[tokio::test]
async fn resealed_wrong_actual_debit_facts_never_become_birth_evidence() {
    for fault in 1..=3 {
        let world = seed_debit_binding(false, false, true, fault).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        let hub = world.hub;
        let operation = world.op.id.clone();
        let group = world.projection.group.id;
        let result = world
            .db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let op = stock::read::operation_on(tx, hub, &operation)
                        .await?
                        .unwrap();
                    super::birth::birth_on(tx, hub, &op, group).await
                })
            })
            .await
            .map_err(map_transaction_error);
        assert_eq!(
            result.unwrap_err().code(),
            harvestcircle_domain::SafeErrorCode::StorageCorrupt,
            "fault {fault}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
    }
}
