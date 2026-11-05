use super::*;
use harvestcircle_application::hub_submission::{
    HubSubmissionContext, HubSubmittedMaterial, HubSubmittedRequest,
};

#[tokio::test]
async fn native_count_context_encodes_exact_read_evidence_without_admission_or_effect() {
    let (fixture, db, query) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let command = query.clone();
    let (hub, material) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                let material = Box::pin(crate::hub::corrections::capture_count_explanation_on(
                    tx,
                    hub.id(),
                    owner(),
                    &operation(5),
                    &command,
                ))
                .await?;
                Ok((hub, material))
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    assert_eq!(
        material.id,
        crate::hub::actuals::common::operation_id(&operation(5)).unwrap()
    );
    assert_eq!(material.account.unexplained(), packs(2));
    assert_eq!(material.count.before.packs, packs(20));
    assert_eq!(material.count.count.counted_packs, packs(18));
    assert_eq!(material.report.id, query.facts.report);
    assert_eq!(material.source.operation, query.source_operation);
    assert_eq!(material.source.group.id, query.source_group);
    assert!(!material.uncertain_submissions);
    let known = serde_json::to_value(&material.known).unwrap();
    assert_eq!(known["schema"], 2);
    assert_eq!(known["count_accounts"], serde_json::json!([]));
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let archived = HubSubmittedRequest::CountExplanation(&query)
        .capture(
            &operation(5),
            HubSubmissionScope {
                runtime: runtime.runtime,
                generation: runtime.generation,
                actor: runtime.actor.unwrap(),
            },
            &HubSubmissionContext {
                hub,
                catalog_revision: Some(query.facts.expected_catalog_revision),
                stock_revision: Some(query.facts.expected_stock_revision),
                material: &[HubSubmittedMaterial::CountExplanation(Box::new(material))],
            },
        )
        .unwrap();
    let description: serde_json::Value = serde_json::from_slice(archived.as_bytes()).unwrap();
    assert_eq!(description["kind"], "correction_count_explain");
    assert_eq!(
        description["material"][0]["count_explanation"]["known"]["schema"],
        2
    );
    assert!(
        db.hub_submission(owner(), operation(5))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_capture_separates_current_catalog_content_from_original_count_labels() {
    use harvestcircle_domain::hub::reference::{
        HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue,
    };
    let (fixture, db, mut query) = world().await;
    let input = query.clone();
    let original = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                Box::pin(crate::hub::corrections::capture_count_explanation_on(
                    tx,
                    hub,
                    owner(),
                    &operation(5),
                    &input,
                ))
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let mut expected = Vec::new();
    for (index, record) in original.references.iter().enumerate() {
        let label = HubLabel::new(format!("Current catalog description {index}")).unwrap();
        let value = match &record.value {
            ReferenceValue::Product { .. } => ReferenceValue::Product { label },
            ReferenceValue::Party { .. } => ReferenceValue::Party { label },
            // Pack definitions are immutable, including their original labels.
            ReferenceValue::Pack { .. } => {
                expected.push(record.clone());
                continue;
            }
            _ => panic!("selected catalog kinds"),
        };
        let crate::HubMutationResult::Reference(change) = db
            .submit_hub_mutation(
                scope,
                operation(25 + u32::try_from(index).unwrap()),
                HubMutation::Reference(Box::new(ReferenceCommand {
                    expected_catalog_revision: query.facts.expected_catalog_revision,
                    mutation: ReferenceMutation::Revise {
                        id: record.id,
                        expected_record_revision: record.revision,
                        value,
                    },
                })),
                at(145),
            )
            .await
            .unwrap()
        else {
            panic!("native reference mutation");
        };
        query.facts.expected_catalog_revision = change.change.catalog_revision;
        expected.push(change.change.current);
    }
    let before = crate::hub::custody::tests::material(&fixture).await;
    let captured = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                Box::pin(crate::hub::corrections::capture_count_explanation_on(
                    tx,
                    hub,
                    owner(),
                    &operation(5),
                    &query,
                ))
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    assert_eq!(captured.references, expected);
    assert_ne!(captured.references, original.references);
    assert_eq!(captured.count, original.count);
    assert_eq!(captured.report, original.report);
    assert!(
        captured.known.facts().catalog_revision.value()
            > original.known.facts().catalog_revision.value()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    assert!(
        db.hub_submission(owner(), operation(5))
            .await
            .unwrap()
            .is_none()
    );
    db.close().await.unwrap();
}
