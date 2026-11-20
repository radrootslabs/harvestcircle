use super::*;
pub(in crate::hub) mod capacity;
pub(in crate::hub) mod faults;
mod funding;
mod maximum;
use crate::hub::{
    handling::trace::HubTraceSnapshotQuery,
    stock::tests::{Fixture, id, query},
};
use harvestcircle_domain::{
    SafeError,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
        corrections::{
            CorrectionObservationCommand, CorrectionObservedFacts, CountClassificationCommand,
        },
        movement::PhysicalIdentification,
        reference::{HubLabel, SourceAttribution, WholePacks},
        returns::ReturnCommand,
        stock::Occurrence,
    },
};

fn at(value: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(value).unwrap()
}
fn own(value: u32) -> HubRecordId {
    crate::hub::actuals::common::operation_id(&operation(value)).unwrap()
}
fn evidence() -> SourceProvenance {
    SourceProvenance::new(
        "Explicit independent fictional whole-intake source and quantity evidence",
    )
    .unwrap()
}

pub(in crate::hub) async fn classification(
    db: &Database,
    scope: HubSubmissionScope,
    explanation: HubRecordId,
    prior: (HubRecordId, Option<HubRecordId>),
    packs: u32,
    number: u32,
    time: i64,
) -> Result<Box<super::super::super::count_class_record::HubCountClassificationReceipt>, SafeError>
{
    let crate::HubMutationResult::CorrectionObserved(observation) = db
        .submit_hub_mutation(
            scope,
            operation(number),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: explanation,
                    corrected_packs: WholePacks::from_value(packs).unwrap(),
                    report: None,
                    basis: evidence(),
                },
            })),
            at(time),
        )
        .await?
    else {
        panic!("ordinary correction observation")
    };
    let page = db.hub_stock_page(owner(), query()).await?;
    let command = CountClassificationCommand {
        observation_operation: observation.original.operation,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_account_prior: Some(prior.0),
        expected_prior: prior.1,
    };
    if packs == 0 && prior.1.is_some() {
        assert_return_floor_on(db, command.clone(), scope, own(number + 1), at(time + 10)).await?;
    }
    let crate::HubMutationResult::CountClassified(receipt) = db
        .submit_hub_mutation(
            scope,
            operation(number + 1),
            HubMutation::CountClassification(Box::new(command)),
            at(time + 10),
        )
        .await?
    else {
        panic!("ordinary classification")
    };
    Ok(receipt)
}

// Keep the extra read-only oracle's construction out of the ordinary native
// command caller frame, including when this branch is not selected.
fn assert_return_floor_on<'a>(
    db: &'a Database,
    checked: CountClassificationCommand,
    scope: HubSubmissionScope,
    operation: HubRecordId,
    time: UnixTimestamp,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), SafeError>> + Send + 'a>> {
    Box::pin(async move {
        db.host().transaction(|tx| Box::pin(async move {
            let hub = super::super::super::reference::authorize_on(tx, owner()).await?.id();
            let mut reader = super::super::super::OriginalReader::new(tx);
            let context = super::super::super::count_classification::current_on(&mut reader, hub, &checked, None).await?;
            assert_eq!(checked.plan(&context.domain(), operation, scope.actor, time), Err(harvestcircle_domain::hub::corrections::QuantityEffectError::Accounting(harvestcircle_domain::hub::corrections::CorrectionError::ReturnConflict)));
            Ok::<_, SafeError>(())
        })).await.map_err(crate::db::map_transaction_error)
    })
}

async fn physical(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    crate::hub::custody::tests::material(fixture)
        .await
        .into_iter()
        .filter(|(table, _)| {
            !matches!(
                table.as_str(),
                "hub_submissions"
                    | "hub_stock_operations"
                    | "hub_stock_state"
                    | "hub_corrections"
                    | "hub_correction_scope_links"
                    | "hub_correction_captures"
                    | "hub_correction_count_explanations"
                    | "hub_correction_quantity_changes"
                    | "hub_count_interpretation_markers"
                    | "hub_actual_report_reviews"
                    | "hub_correction_observations"
                    | "hub_count_return_associations"
            )
        })
        .collect()
}

pub(in crate::hub) struct AssociationWorld {
    pub fixture: Fixture,
    pub db: Database,
    pub scope: HubSubmissionScope,
    pub returned: Box<crate::HubReturnReceipt>,
    pub explanation: Box<crate::HubCountExplanationReceipt>,
    pub associated: Box<crate::HubCountReturnAssociationReceipt>,
    pub command: CountReturnAssociationCommand,
    pub trace: HubRecordId,
    pub before: Vec<(String, Vec<String>)>,
}

/// Every fixture record comes from the ordinary native product command path.
pub(in crate::hub) fn association_world()
-> std::pin::Pin<Box<dyn std::future::Future<Output = Box<AssociationWorld>> + Send>> {
    Box::pin(async move {
        let (fixture, db, template) =
            super::super::super::count_review::tests::world_with_recipient(5).await;
        let runtime = db.hub_runtime_session().await.unwrap().unwrap();
        let scope = HubSubmissionScope {
            runtime: runtime.runtime,
            generation: runtime.generation,
            actor: runtime.actor.unwrap(),
        };
        let crate::HubMutationResult::CountExplained(first) = db
            .submit_hub_mutation(
                scope,
                operation(801),
                HubMutation::CountExplanation(Box::new(template.clone())),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("first explanation")
        };
        classification(&db, scope, own(801), (own(801), None), 0, 802, 210)
            .await
            .unwrap();
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Returned(returned) = db
            .submit_hub_mutation(
                scope,
                operation(900),
                HubMutation::Return(Box::new(ReturnCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: page.catalog_revision,
                    variant: id(2),
                    location: id(6),
                    returned_by: None,
                    source: SourceAttribution::Unknown,
                    packs: WholePacks::from_value(1).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Separate identified restricted returned crate")
                            .unwrap(),
                        observation: evidence(),
                    },
                    planned_location: None,
                    departure: None,
                    occurred: Occurrence::Known(at(90)),
                    observation: evidence(),
                })),
                at(230),
            )
            .await
            .unwrap()
        else {
            panic!("ordinary native Return with empty association prefix")
        };
        let trace = first.original.known_event.trace;
        let crate::HubMutationResult::ActualReported(report) = db
            .submit_hub_mutation(
                scope,
                operation(904),
                HubMutation::ActualReport(Box::new(ActualReportCommand {
                    variant: Some(id(2)),
                    recipient: Some(template.facts.recipient),
                    packs: Some(WholePacks::from_value(2).unwrap()),
                    sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                        trace,
                        group: Some(template.source_group),
                        location: Some(id(6)),
                    }),
                    occurred: Occurrence::Known(at(80)),
                    observation: evidence(),
                })),
                at(240),
            )
            .await
            .unwrap()
        else {
            panic!("independent two-pack report")
        };
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let mut command = template;
        command.facts.report = report.original.id;
        command.facts.packs = WholePacks::from_value(2).unwrap();
        command.facts.occurred = Occurrence::Known(at(80));
        command.facts.expected_prior = Some(own(803));
        command.facts.expected_stock_revision = page.revision;
        command.facts.expected_catalog_revision = page.catalog_revision;
        let source_operation = command.source_operation;
        let source_group = command.source_group;
        let crate::HubMutationResult::CountExplained(explanation) = db
            .submit_hub_mutation(
                scope,
                operation(905),
                HubMutation::CountExplanation(Box::new(command)),
                at(250),
            )
            .await
            .unwrap()
        else {
            panic!("target explanation after Return")
        };
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let command = CountReturnAssociationCommand {
            return_operation: own(900),
            explanation_operation: own(905),
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            expected_account_prior: Some(own(905)),
            expected_returns_prior: None,
            source_operation,
            source_group,
            source_evidence: evidence(),
            quantity_evidence: evidence(),
            timing_evidence: evidence(),
        };
        let before = physical(&fixture).await;
        let crate::HubMutationResult::CountReturnAssociated(associated) = db
            .submit_hub_mutation(
                scope,
                operation(906),
                HubMutation::CountReturnAssociation(Box::new(command.clone())),
                at(260),
            )
            .await
            .unwrap()
        else {
            panic!("whole-intake association")
        };
        assert_eq!(associated.original.association.packs.value(), 1);
        assert_eq!(associated.original.association.return_operation, own(900));
        assert_eq!(
            associated.original.association.explanation_operation,
            own(905)
        );
        assert_eq!(physical(&fixture).await, before);
        Box::new(AssociationWorld {
            fixture,
            db,
            scope,
            returned,
            explanation,
            associated,
            command,
            trace,
            before,
        })
    })
}

#[tokio::test]
async fn fully_explained_count_output_retains_only_the_original_record_uncertainty() {
    let world = association_world().await;
    let before = crate::hub::custody::tests::material_digest(&world.fixture).await;
    let current = world
        .db
        .hub_trace_snapshot(
            owner(),
            HubTraceSnapshotQuery {
                trace: world.trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = current.balance().unwrap();
    assert_eq!(
        (
            balance.gross_counted_loss.value(),
            balance.counted_loss.value(),
            balance.actual_departed.value()
        ),
        (2, 0, 2)
    );
    let output = harvestcircle_application::hub_output::trace_output(&current).unwrap();
    assert!(
        output
            .html
            .contains("Current explained count shortage:</strong> 2")
    );
    assert!(
        output
            .html
            .contains("Unexplained count shortage:</strong> 0")
    );
    assert!(
        output
            .html
            .contains("As originally recorded: loss time and cause unknown.")
    );
    assert!(output.html.contains(
        "Later explanations are separate evidence and do not rewrite the original count."
    ));
    assert_eq!(
        crate::hub::custody::tests::material_digest(&world.fixture).await,
        before
    );
    world.db.close().await.unwrap();
}

#[tokio::test]
async fn ordinary_return_before_target_explanation_associates_whole_intake_and_keeps_originals_after_classification_and_reopen()
 {
    let AssociationWorld {
        fixture,
        db,
        scope,
        returned,
        explanation,
        associated,
        command,
        trace,
        before,
    } = *association_world().await;
    let classified = classification(&db, scope, own(905), (own(905), None), 1, 907, 270)
        .await
        .unwrap();
    assert_eq!(physical(&fixture).await, before);
    let rejected = classification(
        &db,
        scope,
        own(905),
        (own(908), Some(own(908))),
        0,
        909,
        310,
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.code(), SafeErrorCode::HubConflict);
    assert!(
        db.hub_submission(owner(), operation(910))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.hub_count_classification_operation(owner(), operation(910))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(physical(&fixture).await, before);
    let raw_observation = db
        .hub_correction_observation_original(owner(), operation(909))
        .await
        .unwrap();
    assert_eq!(
        raw_observation.original.observation,
        CorrectionObservationCommand {
            previous: None,
            facts: CorrectionObservedFacts::CountClassification {
                explanation_operation: own(905),
                corrected_packs: WholePacks::from_value(0).unwrap(),
                report: None,
                basis: evidence()
            },
        }
    );
    let crate::HubMutationResult::CountReturnAssociated(replay) = db
        .submit_hub_mutation(
            scope,
            operation(906),
            HubMutation::CountReturnAssociation(Box::new(command)),
            at(330),
        )
        .await
        .unwrap()
    else {
        panic!("exact association replay")
    };
    assert_eq!(replay, associated);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_correction_observation_original(owner(), operation(909))
            .await
            .unwrap(),
        raw_observation
    );
    assert_eq!(
        db.hub_return_operation(owner(), operation(900))
            .await
            .unwrap()
            .as_ref(),
        Some(returned.as_ref())
    );
    assert_eq!(
        db.hub_count_return_association_operation(owner(), operation(906))
            .await
            .unwrap()
            .as_ref(),
        Some(associated.as_ref())
    );
    assert_eq!(
        db.hub_count_explanation_operation(owner(), operation(905))
            .await
            .unwrap()
            .as_ref(),
        Some(explanation.as_ref())
    );
    assert_eq!(
        db.hub_count_classification_operation(owner(), operation(908))
            .await
            .unwrap()
            .as_ref(),
        Some(classified.as_ref())
    );
    for population in [trace, returned.original.identities.population] {
        let current = db
            .hub_trace_snapshot(
                owner(),
                HubTraceSnapshotQuery {
                    trace: population,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            current.count_return_associations,
            vec![associated.original.association.clone()]
        );
        if population == trace {
            let balance = current.balance().unwrap();
            assert_eq!(
                (
                    balance.on_hand.value(),
                    balance.counted_loss.value(),
                    balance.actual_departed.value()
                ),
                (18, 1, 1)
            );
        } else {
            let balance = current.balance().unwrap();
            assert_eq!(
                (
                    balance.original.value(),
                    balance.on_hand.value(),
                    balance.counted_loss.value(),
                    balance.actual_departed.value()
                ),
                (1, 1, 0, 0)
            );
        }
    }
    assert_eq!(physical(&fixture).await, before);
    db.close().await.unwrap();
}
