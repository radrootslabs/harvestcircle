//! Ordinary native fan-out, complementing the separate long E/C-chain cases.
use super::*;
use crate::hub::custody::tests::material_digest;

#[tokio::test]
async fn ordinary_native_mixed_association_fanout_precursor_reopens_cold() {
    qualify(4, false).await;
}

#[tokio::test]
#[ignore = "Explicit ordinary native mixed4096/4097 shared correction budget"]
async fn ordinary_native_mixed_association_4096_rows_rejects_growth_and_reopens_cold() {
    qualify(4094, true).await;
}

fn submit<'a>(
    db: &'a Database,
    scope: HubSubmissionScope,
    number: u32,
    mutation: HubMutation,
    time: i64,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<crate::HubMutationResult, SafeError>> + Send + 'a>,
> {
    Box::pin(db.submit_hub_mutation(scope, operation(number), mutation, at(time)))
}

fn qualify(
    associations: u32,
    full: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        // Leave one further whole Return affordable after all associations: overflow
        // must be the shared correction row cap, not the quantity floor.
        let quantity = associations + 2;
        let (fixture, db, template) =
            crate::hub::corrections::count_review::tests::world_with_shortage(
                5, quantity, quantity,
            )
            .await;
        let runtime = db.hub_runtime_session().await.unwrap().unwrap();
        let scope = HubSubmissionScope {
            runtime: runtime.runtime,
            generation: runtime.generation,
            actor: runtime.actor.unwrap(),
        };
        let source_operation = template.source_operation;
        let source_group = template.source_group;
        let crate::HubMutationResult::CountExplained(explanation) = submit(
            &db,
            scope,
            801,
            HubMutation::CountExplanation(Box::new(template)),
            200,
        )
        .await
        .unwrap() else {
            panic!("ordinary explanation")
        };
        let classified = classification(
            &db,
            scope,
            own(801),
            (own(801), None),
            quantity - 1,
            802,
            210,
        )
        .await
        .unwrap();
        let mut revision = classified.original.amendment.stock_revision;
        let mut catalog = classified.original.observation.catalog_revision;
        let mut returns_head = None;
        let mut first = None;
        let mut last = None;
        for pair in 0..associations {
            let number = 500000 + pair * 2;
            let time = 300 + i64::from(pair) * 4;
            let returned = receive_return(&db, scope, number, revision, catalog, time).await;
            let command = CountReturnAssociationCommand {
                return_operation: own(number),
                explanation_operation: own(801),
                expected_stock_revision: returned.original.stock_revision,
                expected_catalog_revision: returned.original.catalog_revision,
                expected_account_prior: Some(own(803)),
                expected_returns_prior: returns_head,
                source_operation,
                source_group,
                source_evidence: evidence(),
                quantity_evidence: evidence(),
                timing_evidence: evidence(),
            };
            let crate::HubMutationResult::CountReturnAssociated(associated) = submit(
                &db,
                scope,
                number + 1,
                HubMutation::CountReturnAssociation(Box::new(command.clone())),
                time + 2,
            )
            .await
            .unwrap() else {
                panic!("ordinary association {pair}")
            };
            assert_eq!(associated.original.association.packs.value(), 1);
            revision = associated.original.association.stock_revision;
            catalog = associated.original.association.catalog_revision;
            returns_head = Some(associated.original.association.operation);
            if first.is_none() {
                first = Some((returned.clone(), associated.clone()));
            }
            last = Some((returned, associated, command));
            if (pair + 2) % 16 == 0 {
                eprintln!(
                    "ordinary mixed native committed{} correction rows",
                    pair + 3
                );
            }
        }
        let (first_return, first_association) = first.unwrap();
        let (last_return, last_association, last_command) = last.unwrap();
        let stable = material_digest(&fixture).await;
        let crate::HubMutationResult::CountReturnAssociated(replay) = submit(
            &db,
            scope,
            500001 + (associations - 1) * 2,
            HubMutation::CountReturnAssociation(Box::new(last_command)),
            30000,
        )
        .await
        .unwrap() else {
            panic!("exact recorded association replay")
        };
        assert_eq!(replay, last_association);
        assert_eq!(material_digest(&fixture).await, stable);
        db.close().await.unwrap();
        let db = fixture.open().await;
        // The deepest association is the first cold original read.
        assert_eq!(
            db.hub_count_return_association_operation(owner(), last_association.operation.clone())
                .await
                .unwrap()
                .as_ref(),
            Some(last_association.as_ref())
        );
        assert_eq!(
            db.hub_count_explanation_operation(owner(), operation(801))
                .await
                .unwrap()
                .as_ref(),
            Some(explanation.as_ref())
        );
        assert_eq!(
            db.hub_count_classification_operation(owner(), operation(803))
                .await
                .unwrap()
                .as_ref(),
            Some(classified.as_ref())
        );
        for (returned, associated) in [
            (&first_return, &first_association),
            (&last_return, &last_association),
        ] {
            assert_eq!(
                db.hub_return_operation(owner(), returned.operation.clone())
                    .await
                    .unwrap()
                    .as_ref(),
                Some(returned.as_ref())
            );
            assert_eq!(
                db.hub_count_return_association_operation(owner(), associated.operation.clone())
                    .await
                    .unwrap()
                    .as_ref(),
                Some(associated.as_ref())
            );
        }
        let current = db
            .hub_trace_snapshot(
                owner(),
                HubTraceSnapshotQuery {
                    trace: explanation.original.known_event.trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            current.count_return_associations.len(),
            associations as usize
        );
        let balance = current.balance().unwrap();
        assert_eq!(
            (
                balance.original.value(),
                balance.on_hand.value(),
                balance.counted_loss.value(),
                balance.actual_departed.value()
            ),
            (quantity, 0, 1, quantity - 1)
        );
        assert_eq!(material_digest(&fixture).await, stable);
        if full {
            let number = 500000 + associations * 2;
            let returned = receive_return(&db, scope, number, revision, catalog, 30010).await;
            let retained = material_digest(&fixture).await;
            assert_ne!(retained, stable);
            let command = CountReturnAssociationCommand {
                return_operation: own(number),
                explanation_operation: own(801),
                expected_stock_revision: returned.original.stock_revision,
                expected_catalog_revision: returned.original.catalog_revision,
                expected_account_prior: Some(own(803)),
                expected_returns_prior: returns_head,
                source_operation,
                source_group,
                source_evidence: evidence(),
                quantity_evidence: evidence(),
                timing_evidence: evidence(),
            };
            assert_eq!(
                submit(
                    &db,
                    scope,
                    number + 1,
                    HubMutation::CountReturnAssociation(Box::new(command)),
                    30012
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubCapacity
            );
            assert!(
                db.hub_submission(owner(), operation(number + 1))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                db.hub_count_return_association_operation(owner(), operation(number + 1))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(material_digest(&fixture).await, retained);
            db.close().await.unwrap();
            let db = fixture.open().await;
            assert_eq!(
                db.hub_return_operation(owner(), operation(number))
                    .await
                    .unwrap()
                    .as_ref(),
                Some(returned.as_ref())
            );
            assert_eq!(
                db.hub_count_return_association_operation(
                    owner(),
                    last_association.operation.clone()
                )
                .await
                .unwrap()
                .as_ref(),
                Some(last_association.as_ref())
            );
            assert_eq!(material_digest(&fixture).await, retained);
            db.close().await.unwrap();
        } else {
            db.close().await.unwrap();
        }
    })
}

async fn receive_return(
    db: &Database,
    scope: HubSubmissionScope,
    number: u32,
    revision: harvestcircle_domain::hub::HubRevision,
    catalog: harvestcircle_domain::hub::HubRevision,
    time: i64,
) -> Box<crate::HubReturnReceipt> {
    let crate::HubMutationResult::Returned(returned) = submit(
        db,
        scope,
        number,
        HubMutation::Return(Box::new(ReturnCommand {
            expected_stock_revision: revision,
            expected_catalog_revision: catalog,
            variant: id(2),
            location: id(6),
            returned_by: None,
            source: SourceAttribution::Unknown,
            packs: WholePacks::from_value(1).unwrap(),
            identification: PhysicalIdentification {
                label: HubLabel::new(format!("Independent restricted Return {number}")).unwrap(),
                observation: evidence(),
            },
            planned_location: None,
            departure: None,
            occurred: Occurrence::Known(at(time)),
            observation: evidence(),
        })),
        time + 1,
    )
    .await
    .unwrap() else {
        panic!("ordinary Return {number}")
    };
    returned
}
