//! Full native current-cut comparison and separate legal posting tests. No
//! synthetic correction root is accepted as a legal effect.
pub(in crate::hub) mod posting_tests;
use super::*;
use crate::{
    HubFoundCorrectionReviewBlocker as Blocker, HubFoundCorrectionReviewOutcome as Outcome,
    HubFoundCorrectionReviewQuery,
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        corrections::{CorrectionObservationCommand, PhysicalCorrectionError},
        treatment::AcceptanceStatus,
    },
};

#[derive(Clone, Copy)]
enum PriorReturn {
    None,
    Established,
    TwoEstablished,
    Unknown,
}

#[tokio::test]
async fn unresolved_actual_report_blocks_found_comparison_without_discarding_either_fact() {
    use harvestcircle_domain::hub::actuals::{ActualReportCommand, ActualReportSources};
    let world = world(2, PriorReturn::None).await;
    let crate::HubMutationResult::ActualReported(report) = world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1601),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: None,
                recipient: None,
                packs: None,
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note(
                    "Independent unresolved report remains relevant to every possible source",
                ),
            })),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("raw actual report")
    };
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let review = world
        .db
        .hub_found_correction_review(owner(), world.query, at(200))
        .await
        .unwrap();
    assert_eq!(review.observation, world.observation);
    assert_eq!(
        review.outcome,
        Outcome::Blocked(Blocker::Policy(
            PhysicalCorrectionError::UncertainSubmission
        ))
    );
    assert_eq!(
        world
            .db
            .hub_actual_report_operation(owner(), report.operation.clone())
            .await
            .unwrap(),
        Some(*report)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}
struct ReviewWorld {
    fixture: Fixture,
    db: Database,
    departure: crate::HubDepartureReceipt,
    observation: crate::HubCorrectionObservationReceipt,
    scope: HubSubmissionScope,
    query: HubFoundCorrectionReviewQuery,
    returned: Option<crate::HubReturnReceipt>,
}
async fn world(packs: u32, prior: PriorReturn) -> ReviewWorld {
    world_with_quantities(packs, prior, 20, 6).await
}
async fn world_with_quantities(
    packs: u32,
    prior: PriorReturn,
    total: u32,
    outbound: u32,
) -> ReviewWorld {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, departure) =
        crate::hub::handling::departure::tests::custody_departure_fixture_quantities(
            &db, None, total, outbound,
        )
        .await;
    let before = &departure.record.consumed[0].before;
    let returned = match prior {
        PriorReturn::None => None,
        PriorReturn::Established | PriorReturn::TwoEstablished => Some(
            super::return_funding_tests::record_return(
                &db,
                Some(harvestcircle_domain::hub::returns::ReturnDepartureClaim {
                    departure: departure.record.id,
                    group: Some(before.id),
                }),
                120,
            )
            .await,
        ),
        PriorReturn::Unknown => {
            Some(super::return_funding_tests::record_return(&db, None, 120).await)
        }
    };
    if matches!(prior, PriorReturn::TwoEstablished) {
        let mut command = super::return_funding_tests::return_command(
            &db,
            Some(harvestcircle_domain::hub::returns::ReturnDepartureClaim {
                departure: departure.record.id,
                group: Some(before.id),
            }),
            id(2),
        )
        .await;
        command.identification.label = HubLabel::new("Second separate active return").unwrap();
        db.apply_hub_return(owner(), operation(1650), command, at(130))
            .await
            .unwrap();
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
    let target = crate::hub::actuals::common::operation_id(&departure.operation).unwrap();
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: target,
            target_group: before.id,
            basis: note("Independently found current crates; original paperwork remains"),
            allocations: vec![FoundStockInput {
                debit: PhysicalDebitKey {
                    operation: target,
                    group: before.id,
                },
                packs: WholePacks::from_value(packs).unwrap(),
                location: before.location,
                identification: PhysicalIdentification {
                    label: HubLabel::new("Distinct observed found crates").unwrap(),
                    observation: note("Independent present physical identification"),
                },
                cohort_evidence: note("Observed old source and exact physical debit reviewed"),
            }],
        },
    };
    let crate::HubMutationResult::CorrectionObserved(observation) = db
        .submit_hub_mutation(
            scope,
            operation(1600),
            HubMutation::CorrectionObservation(Box::new(input)),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("recorded observation")
    };
    let query = HubFoundCorrectionReviewQuery {
        observation_operation: operation(1600),
        proposed_operation: operation(1700),
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    ReviewWorld {
        fixture,
        db,
        departure,
        observation: *observation,
        scope,
        query,
        returned,
    }
}

#[tokio::test]
async fn exact_recorded_found_observation_compares_restricted_new_child_without_posting_or_revival()
{
    let world = world(2, PriorReturn::Established).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let review = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(review.observation, world.observation);
    let Outcome::Candidate(plan) = review.outcome else {
        panic!("qualified current comparison")
    };
    assert_eq!(plan.amendment.before.value(), 6);
    assert_eq!(plan.amendment.after.value(), 4);
    assert_eq!(plan.allocations[0].available_before.value(), 4);
    let child = &plan.allocations[0].group;
    assert_ne!(child.id, world.departure.record.consumed[0].before.id);
    assert_eq!(
        child.parent,
        Some(world.departure.record.consumed[0].before.id)
    );
    assert_eq!(child.packs.value(), 2);
    assert_eq!(child.revision, HubRevision::INITIAL);
    assert!(child.work.is_empty());
    assert_eq!(child.causal_floor, Some(at(200)));
    let treatment = plan
        .after_treatment
        .project(&plan.after_lineage, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(
        treatment
            .groups
            .iter()
            .find(|g| g.group == child.id)
            .unwrap()
            .acceptance,
        AcceptanceStatus::ReviewRequired
    );
    assert_eq!(treatment.summary.departure_eligible.value(), 14);
    assert_eq!(treatment.summary.review_required.value(), 2);
    assert_eq!(
        world
            .db
            .hub_departure_operation(owner(), world.departure.operation.clone())
            .await
            .unwrap()
            .unwrap(),
        world.departure
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn zero_overfunded_and_uncertain_facts_stay_recorded_when_comparison_is_blocked() {
    for (packs, prior, error) in [
        (
            0,
            PriorReturn::None,
            PhysicalCorrectionError::InvalidQuantity,
        ),
        (
            5,
            PriorReturn::Established,
            PhysicalCorrectionError::InvalidQuantity,
        ),
        (
            2,
            PriorReturn::Unknown,
            PhysicalCorrectionError::UnresolvedReturnScope,
        ),
    ] {
        let world = world(packs, prior).await;
        let before = crate::hub::custody::tests::material(&world.fixture).await;
        let review = world
            .db
            .hub_found_correction_review(owner(), world.query, at(200))
            .await
            .unwrap();
        assert_eq!(review.observation, world.observation);
        assert_eq!(review.outcome, Outcome::Blocked(Blocker::Policy(error)));
        assert_eq!(
            crate::hub::custody::tests::material(&world.fixture).await,
            before
        );
        world.db.close().await.unwrap();
    }
}

#[tokio::test]
async fn active_identification_in_another_trace_cannot_be_reused_by_found_comparison() {
    let world = world(2, PriorReturn::Established).await;
    let mut replacement = world.observation.original.observation.clone();
    replacement.previous = Some(world.observation.original.operation);
    let CorrectionObservedFacts::Found { allocations, .. } = &mut replacement.facts else {
        unreachable!()
    };
    allocations[0].identification = world
        .returned
        .as_ref()
        .unwrap()
        .original
        .group
        .identification
        .clone()
        .unwrap();
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1601),
            HubMutation::CorrectionObservation(Box::new(replacement)),
            at(160),
        )
        .await
        .unwrap();
    let mut input = world.query;
    input.observation_operation = operation(1601);
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), input, at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::Policy(PhysicalCorrectionError::IdentityConflict))
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn superseded_observation_and_unknown_named_target_remain_readable_without_new_effect() {
    let world = world(2, PriorReturn::None).await;
    let mut replacement = world.observation.original.observation.clone();
    replacement.previous = Some(world.observation.original.operation);
    let CorrectionObservedFacts::Found {
        target_operation,
        target_group,
        ..
    } = &mut replacement.facts
    else {
        unreachable!()
    };
    *target_operation = id(98);
    *target_group = id(99);
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1601),
            HubMutation::CorrectionObservation(Box::new(replacement)),
            at(160),
        )
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let old = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(old.observation, world.observation);
    assert_eq!(
        old.outcome,
        Outcome::Blocked(Blocker::ObservationNoLongerCurrent)
    );
    let mut input = world.query;
    input.observation_operation = operation(1601);
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), input, at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::TargetNotQualified)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    // A syntactically valid UUIDv7 with no original is equally unqualified.
    let mut next = world.observation.original.observation.clone();
    next.previous = Some(crate::hub::actuals::common::operation_id(&operation(1601)).unwrap());
    let CorrectionObservedFacts::Found {
        target_operation, ..
    } = &mut next.facts
    else {
        unreachable!()
    };
    *target_operation = crate::hub::actuals::common::operation_id(&operation(9999)).unwrap();
    world
        .db
        .submit_hub_mutation(
            world.scope,
            operation(1602),
            HubMutation::CorrectionObservation(Box::new(next)),
            at(170),
        )
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    let input = HubFoundCorrectionReviewQuery {
        observation_operation: operation(1602),
        proposed_operation: operation(1700),
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), input, at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::TargetNotQualified)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn other_raw_observation_is_not_excluded_and_original_review_survives_reopen() {
    let world = world(2, PriorReturn::None).await;
    let candidate = world
        .db
        .hub_found_correction_review(owner(), world.query.clone(), at(200))
        .await
        .unwrap();
    assert!(matches!(candidate.outcome, Outcome::Candidate(_)));
    world.db.close().await.unwrap();
    let db = world.fixture.open().await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        db.hub_found_correction_review(owner(), world.query.clone(), at(200))
            .await
            .unwrap(),
        candidate
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    let runtime = db.begin_hub_runtime(at(210)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(210),
        )
        .await
        .unwrap();
    let input = world.observation.original.observation.clone();
    db.submit_hub_mutation(
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        operation(1601),
        HubMutation::CorrectionObservation(Box::new(input)),
        at(220),
    )
    .await
    .unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        db.hub_found_correction_review(owner(), world.query, at(230))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::Policy(
            PhysicalCorrectionError::UncertainSubmission
        ))
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_native_actual_debit_compares_exact_found_amount_without_reducing_work_again() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, packs) = super::actual_debit(&db).await;
    assert_eq!(packs, 2);
    assert_eq!(before.packs.value(), 20);
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
    let target = crate::hub::actuals::common::operation_id(&target).unwrap();
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: target,
            target_group: before.id,
            basis: note("Actual debit comparison; no new known departure"),
            allocations: vec![FoundStockInput {
                debit: PhysicalDebitKey {
                    operation: target,
                    group: before.id,
                },
                packs: WholePacks::from_value(1).unwrap(),
                location: before.location,
                identification: PhysicalIdentification {
                    label: HubLabel::new("Found actual debit crate").unwrap(),
                    observation: note("Independent currently found crate"),
                },
                cohort_evidence: note("Exact old actual debit reviewed"),
            }],
        },
    };
    db.submit_hub_mutation(
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        operation(1600),
        HubMutation::CorrectionObservation(Box::new(input)),
        at(180),
    )
    .await
    .unwrap();
    let query = HubFoundCorrectionReviewQuery {
        observation_operation: operation(1600),
        proposed_operation: operation(1700),
        expected_stock_revision: None,
        expected_catalog_revision: None,
    };
    let material = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_found_correction_review(owner(), query.clone(), at(179))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::Policy(PhysicalCorrectionError::InvalidHistory))
    );
    let review = db
        .hub_found_correction_review(owner(), query, at(200))
        .await
        .unwrap();
    let Outcome::Candidate(plan) = review.outcome else {
        panic!("actual debit candidate")
    };
    assert_eq!(
        plan.family,
        harvestcircle_domain::hub::corrections::CorrectedDepartureFamily::ReviewedActual
    );
    assert_eq!(
        (plan.amendment.before.value(), plan.amendment.after.value()),
        (2, 1)
    );
    assert_eq!(plan.allocations[0].funding.before, before);
    assert!(plan.allocations[0].group.work.is_empty());
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn foreign_stale_or_already_used_proposals_never_acquire_a_review_as_execution() {
    let world = world(2, PriorReturn::None).await;
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        world
            .db
            .hub_found_correction_review(
                crate::hub::stock::tests::other(),
                world.query.clone(),
                at(200)
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = world.query.clone();
    stale.expected_stock_revision = Some(HubRevision::INITIAL);
    stale.expected_catalog_revision = Some(world.observation.original.catalog_revision);
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), stale, at(200))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    for proposal in [
        world.departure.operation.clone(),
        operation(101),
        world.query.observation_operation.clone(),
    ] {
        let mut query = world.query.clone();
        query.proposed_operation = proposal;
        assert_eq!(
            world
                .db
                .hub_found_correction_review(owner(), query, at(200))
                .await
                .unwrap()
                .outcome,
            Outcome::Blocked(Blocker::OperationAlreadyUsed)
        );
    }
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn pending_intent_cannot_be_evaded_by_selecting_another_proposed_operation() {
    let world = world(2, PriorReturn::None).await;
    let input = world.observation.original.observation.clone();
    assert_eq!(
        world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(1700),
                HubMutation::CorrectionObservation(Box::new(input)),
                at(100)
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let pending = world
        .db
        .hub_submission(owner(), operation(1700))
        .await
        .unwrap()
        .unwrap();
    let before = crate::hub::custody::tests::material(&world.fixture).await;
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), world.query.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::OperationAlreadyUsed)
    );
    let mut other = world.query;
    other.proposed_operation = operation(1701);
    assert_eq!(
        world
            .db
            .hub_found_correction_review(owner(), other, at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(Blocker::Policy(
            PhysicalCorrectionError::UncertainSubmission
        ))
    );
    assert_eq!(
        world
            .db
            .hub_submission(owner(), operation(1700))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        crate::hub::custody::tests::material(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}
