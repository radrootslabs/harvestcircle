//! Real actor admission and credential/read-only recovery for the new custody kinds.
use super::*;
use harvestcircle_domain::hub::{
    disposition::{DispositionCommand, DispositionKind},
    returns::{ReturnCommand, ReturnDepartureClaim, ReturnLinkResolution},
};

#[tokio::test(flavor = "multi_thread")]
async fn runtime_return_and_disposition_use_durable_admission_preserve_unknowns_and_recover_originals()
 {
    let (actor, secrets, owner) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let return_op = DurableRequestId::new_v7();
    let input = ReturnCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(3),
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: packs(2),
        identification: PhysicalIdentification {
            label: HubLabel::new("Actually returned two crates").unwrap(),
            observation: note("Present custody with unknown old departure"),
        },
        planned_location: Some(id(4)),
        departure: Some(ReturnDepartureClaim {
            departure: id(99),
            group: None,
        }),
        occurred: Occurrence::Unknown,
        observation: note("Claimed old exit is not in these records"),
    };
    let result = actor
        .apply_hub_return(
            return_op.clone(),
            input.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(
        result.original.linkage.resolution,
        ReturnLinkResolution::Unrecorded
    );
    assert_eq!(
        result.original.acceptance,
        harvestcircle_domain::hub::treatment::AcceptanceStatus::ReviewRequired
    );
    assert_eq!(
        result
            .original
            .intake_work
            .as_ref()
            .unwrap()
            .quantity
            .remaining(),
        packs(2)
    );
    assert_eq!(
        actor
            .apply_hub_return(
                return_op.clone(),
                input.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        result
    );
    let group = actor
        .hub_group(result.original.identities.group)
        .await
        .unwrap();
    let removal = DispositionCommand {
        expected_stock_revision: group.revision,
        expected_catalog_revision: group.catalog_revision,
        group: group.group.id,
        expected_group_revision: group.group.revision,
        packs: packs(2),
        kind: DispositionKind::Destroyed,
        occurred: Occurrence::Unknown,
        observation: note(
            "Actually destroyed complete two crates, no buyer delivery or count loss",
        ),
    };
    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor.hub_return_operation(return_op.clone()).await.unwrap(),
        Some(result.clone())
    );
    let removal_op = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .apply_hub_disposition(
                removal_op.clone(),
                removal.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    let mut next_return = input;
    next_return.expected_stock_revision = group.revision;
    next_return.identification.label = HubLabel::new("Different returned crates").unwrap();
    let next_op = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .apply_hub_return(
                next_op.clone(),
                next_return.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    for op in [removal_op.clone(), next_op] {
        assert!(
            actor
                ._adapter
                .database()
                .hub_submission(owner, op)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        actor.hub_stock(query()).await.unwrap().rows[0]
            .position
            .packs,
        packs(2)
    );
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();
    let removed = actor
        .apply_hub_disposition(
            removal_op.clone(),
            removal.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(removed.original.work_changes[0].retired, packs(2));
    assert_eq!(
        actor
            .apply_hub_disposition(
                removal_op.clone(),
                removal.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        removed
    );
    assert!(actor.hub_stock(query()).await.unwrap().rows.is_empty());
    assert_eq!(
        actor
            .hub_disposition_operation(removal_op.clone())
            .await
            .unwrap(),
        Some(removed.clone())
    );
    assert_eq!(
        actor.hub_return_operation(return_op.clone()).await.unwrap(),
        Some(result)
    );
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_return_operation(return_op)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        actor
            .hub_disposition_operation(removal_op)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert!(
        actor
            .apply_hub_return(
                DurableRequestId::new_v7(),
                next_return,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .is_err()
    );
    assert!(
        actor
            .apply_hub_disposition(
                DurableRequestId::new_v7(),
                removal,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .is_err()
    );
    actor.close().await.unwrap();
}
