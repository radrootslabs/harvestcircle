//! Persistent recovery-origin composition through actual native commands.
use super::*;

#[tokio::test]
async fn second_found_credit_reduces_exact_funding_preserves_first_original_and_refuses_exhaustion()
{
    let mut world = world(2, PriorReturn::None).await;
    let first = post_world(&world).await;
    for (obs, post, packs, time) in [(1601, 1701, 4, 210), (1602, 1702, 1, 240)] {
        let mut input = world.observation.original.observation.clone();
        input.previous = None;
        let CorrectionObservedFacts::Found { allocations, .. } = &mut input.facts else {
            unreachable!()
        };
        allocations[0].packs = WholePacks::from_value(packs).unwrap();
        allocations[0].identification.label =
            HubLabel::new(format!("Independent subsequent found crates {obs}")).unwrap();
        let crate::HubMutationResult::CorrectionObserved(observation) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(obs),
                HubMutation::CorrectionObservation(Box::new(input)),
                at(time),
            )
            .await
            .unwrap()
        else {
            panic!("raw subsequent fact")
        };
        world.observation = *observation;
        world.query.observation_operation = operation(obs);
        world.query.proposed_operation = operation(post);
        let funding = world
            .db
            .hub_correction_target_funding(
                owner(),
                crate::HubCorrectionFundingQuery {
                    target_operation: world.departure.operation.clone(),
                    original_group: world.departure.record.consumed[0].before.id,
                    expected_stock_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            funding.debits[0].available().unwrap().value(),
            if packs == 4 { 4 } else { 0 }
        );
        if packs == 4 {
            let second = post_world_at(&world, time + 10).await;
            assert_eq!(
                (
                    second.original.amendment.before.value(),
                    second.original.amendment.after.value()
                ),
                (4, 0)
            );
            assert_ne!(
                first.original.allocations[0].group.id,
                second.original.allocations[0].group.id
            );
            assert_eq!(
                world
                    .db
                    .hub_found_correction_original(owner(), first.operation.clone())
                    .await
                    .unwrap(),
                Some(*first.clone())
            );
            assert_eq!(
                world
                    .db
                    .hub_found_correction_original(owner(), second.operation.clone())
                    .await
                    .unwrap(),
                Some(*second)
            );
        } else {
            let before = crate::hub::custody::tests::material(&world.fixture).await;
            let review = world
                .db
                .hub_found_correction_review(owner(), world.query.clone(), at(time + 10))
                .await
                .unwrap();
            assert_eq!(
                review.outcome,
                Outcome::Blocked(Blocker::Policy(PhysicalCorrectionError::InvalidQuantity))
            );
            let command = FoundCorrectionCommand {
                observation_operation: review.observation.original.operation,
                expected_stock_revision: review.revision,
                expected_catalog_revision: review.catalog_revision,
            };
            assert_eq!(
                world
                    .db
                    .submit_hub_mutation(
                        world.scope,
                        operation(post),
                        HubMutation::FoundCorrection(Box::new(command)),
                        at(time + 10)
                    )
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::HubInvalidInput
            );
            assert_eq!(
                world
                    .db
                    .hub_correction_observation_original(owner(), operation(obs))
                    .await
                    .unwrap(),
                world.observation
            );
            assert_eq!(
                crate::hub::custody::tests::material(&world.fixture).await,
                before
            );
        }
    }
    world.db.close().await.unwrap();
    let reopened = world.fixture.open().await;
    assert_eq!(
        reopened
            .hub_found_correction_original(owner(), first.operation.clone())
            .await
            .unwrap(),
        Some(*first)
    );
    let second = reopened
        .hub_found_correction_original(owner(), operation(1701))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.original.amendment.after.value(), 0);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn found_actual_child_inherits_only_holds_captured_before_its_original_debit() {
    for pre_debit in [true, false] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let (before, target, _) =
            super::super::super::actual_debit_configuration(&db, false, Some(pre_debit)).await;
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
        let target_id = crate::hub::actuals::common::operation_id(&target).unwrap();
        let input = CorrectionObservationCommand {
            previous: None,
            facts: CorrectionObservedFacts::Found {
                target_operation: target_id,
                target_group: before.id,
                basis: note(
                    "Original debit and current independently found held-cohort crates compared",
                ),
                allocations: vec![FoundStockInput {
                    debit: PhysicalDebitKey {
                        operation: target_id,
                        group: before.id,
                    },
                    packs: WholePacks::from_value(2).unwrap(),
                    location: before.location,
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Independent found held-cohort two").unwrap(),
                        observation: note("Present identity separate from old parent"),
                    },
                    cohort_evidence: note(
                        "Exact historical actual physical debit and hold capture compared",
                    ),
                }],
            },
        };
        let crate::HubMutationResult::CorrectionObserved(observation) = db
            .submit_hub_mutation(
                scope,
                operation(1600),
                HubMutation::CorrectionObservation(Box::new(input)),
                at(180),
            )
            .await
            .unwrap()
        else {
            panic!("observed")
        };
        let query = HubFoundCorrectionReviewQuery {
            observation_operation: operation(1600),
            proposed_operation: operation(1700),
            expected_stock_revision: None,
            expected_catalog_revision: None,
        };
        let review = db
            .hub_found_correction_review(owner(), query, at(200))
            .await
            .unwrap();
        assert!(matches!(review.outcome, Outcome::Candidate(_)));
        let command = FoundCorrectionCommand {
            observation_operation: observation.original.operation,
            expected_stock_revision: review.revision,
            expected_catalog_revision: review.catalog_revision,
        };
        let work = work_rows(crate::hub::custody::tests::material(&fixture).await);
        let crate::HubMutationResult::FoundCorrected(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1700),
                HubMutation::FoundCorrection(Box::new(command)),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("held found posted")
        };
        let child = &receipt.original.allocations[0].group;
        let treatment = db
            .hub_treatment(
                owner(),
                crate::HubTreatmentScope {
                    trace: before.trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        let selected = treatment
            .projection
            .groups
            .iter()
            .find(|g| g.group == child.id)
            .unwrap();
        assert_eq!(selected.acceptance, AcceptanceStatus::ReviewRequired);
        assert_eq!(selected.active_holds.len(), usize::from(pre_debit));
        assert_eq!(
            treatment
                .projection
                .groups
                .iter()
                .find(|g| g.group == before.id)
                .unwrap()
                .active_holds
                .len(),
            1
        );
        assert!(child.work.is_empty());
        assert_eq!(
            work_rows(crate::hub::custody::tests::material(&fixture).await),
            work
        );
        db.close().await.unwrap();
        let reopened = fixture.open().await;
        assert_eq!(
            reopened
                .hub_found_correction_original(owner(), receipt.operation.clone())
                .await
                .unwrap(),
            Some(*receipt)
        );
        reopened.close().await.unwrap();
    }
}
