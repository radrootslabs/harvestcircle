use super::super::tests::{OfflineNostr, dependencies};
use super::*;
use harvestcircle_application::InMemorySecretStore;
use harvestcircle_domain::hub::{
    HubRevision,
    movement::PhysicalIdentification,
    reference::{
        HubLabel, LocationPurpose, ReferenceCommand, ReferenceMutation, ReferenceValue,
        SourceAttribution, SourceProvenance, WholePacks,
    },
    stock::{Occurrence, OpeningCommand, OpeningHistory, ReceiptCommand},
};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use harvestcircle_storage::HubStockQuery;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn now() -> UnixTimestamp {
    UnixTimestamp::from_seconds(50).unwrap()
}
fn query() -> HubStockQuery {
    HubStockQuery {
        expected_catalog_revision: None,
        expected_revision: None,
        offset: 0,
        limit: 64,
        location: None,
        trace: None,
        text: String::new(),
    }
}
async fn fixture() -> (RuntimeActorHandle, Arc<InMemorySecretStore>, PublicKey) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let actor = RuntimeActorHandle::in_memory(
        RelayConfiguration::default(),
        dependencies(secrets.clone(), Arc::new(OfflineNostr)).with_demo_profile(),
        NonZeroUsize::new(8).unwrap(),
        &Handle::current(),
    )
    .await
    .unwrap();
    let generated = actor
        .generate_identity(
            DurableRequestId::new_v7(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let owner = generated.identity().public_key();
    drop(generated);
    actor.activate_identity(owner).await.unwrap();
    actor
        .initialize_hub(
            DurableRequestId::new_v7(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    for (n, value) in [
        (
            1,
            ReferenceValue::Product {
                label: HubLabel::new("Fictional apples").unwrap(),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                label: HubLabel::new("Whole crate").unwrap(),
                product: id(1),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Location {
                label: HubLabel::new("Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            4,
            ReferenceValue::Location {
                label: HubLabel::new("A3").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        let catalog = actor.hub_references().await.unwrap();
        actor
            .apply_hub_reference(
                DurableRequestId::new_v7(),
                ReferenceCommand {
                    expected_catalog_revision: catalog.revision(),
                    mutation: ReferenceMutation::Create { id: id(n), value },
                },
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT,
            )
            .await
            .unwrap();
    }
    (actor, secrets, owner)
}
#[tokio::test(flavor = "multi_thread")]
async fn count_runtime_preserves_twenty_until_reviewed_post_and_recovers_originals_under_current_authority()
 {
    let (actor, _, _) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let receipt_operation = DurableRequestId::new_v7();
    let receipt = actor
        .receive_hub_stock(
            receipt_operation.clone(),
            ReceiptCommand {
                expected_stock_revision: HubRevision::INITIAL,
                expected_catalog_revision: catalog.revision(),
                variant: id(2),
                supplier: None,
                producer: None,
                source: SourceAttribution::Unknown,
                packs: packs(20),
                receiving: id(3),
                planned_location: Some(id(4)),
                occurred: Occurrence::Unknown,
                observation: note("Fictional twenty counted crates"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let page = actor.hub_stock(query()).await.unwrap();
    let observation_operation = DurableRequestId::new_v7();
    let observed = actor
        .observe_hub_count(
            observation_operation.clone(),
            CountObserveCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: catalog.revision(),
                group: receipt.original.position.group,
                expected_group_revision: page.rows[0].position.revision,
                counted_packs: packs(18),
                cutoff: Occurrence::Known(now()),
                observation: note("Fictional count of eighteen"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let saved = actor.hub_stock(query()).await.unwrap();
    assert_eq!(saved.rows[0].position.packs, packs(20));
    assert_eq!(
        saved.rows[0]
            .put_away
            .as_ref()
            .unwrap()
            .quantity
            .remaining(),
        packs(20)
    );
    let reviewed = actor.hub_count_review(observed.original.id).await.unwrap();
    assert!(reviewed.assessment.ready());
    let input = CountPostCommand {
        expected_stock_revision: reviewed.stock_revision,
        expected_catalog_revision: reviewed.catalog_revision,
        count: observed.original.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note("Reviewed the count and intervening native activity"),
    };
    let operation = DurableRequestId::new_v7();
    let posted = actor
        .post_hub_count(
            operation.clone(),
            input.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let page = actor.hub_stock(query()).await.unwrap();
    assert_eq!(page.rows[0].position.packs, packs(18));
    let work = &page.rows[0].put_away.as_ref().unwrap().quantity;
    assert_eq!(
        (
            work.completed(),
            work.no_longer_required(),
            work.remaining()
        ),
        (packs(0), packs(2), packs(18))
    );
    assert_eq!(
        actor
            .hub_receipt_operation(receipt_operation)
            .await
            .unwrap(),
        Some(receipt)
    );
    assert_eq!(
        actor
            .hub_count_observation_operation(observation_operation.clone())
            .await
            .unwrap(),
        Some(observed.clone())
    );
    assert_eq!(
        actor
            .post_hub_count(
                operation.clone(),
                input.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        posted
    );
    assert_eq!(
        actor
            .hub_count_post_operation(operation.clone())
            .await
            .unwrap(),
        Some(posted)
    );
    assert_eq!(actor.hub_stock(query()).await.unwrap(), page);
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_count_review(observed.original.id)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        actor
            .hub_count_post_operation(operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        actor
            .hub_count_observation_operation(observation_operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert!(
        actor
            .post_hub_count(
                DurableRequestId::new_v7(),
                input,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .is_err()
    );
    actor.close().await.unwrap();
}
#[tokio::test(flavor = "multi_thread")]
async fn opening_runtime_keeps_unknown_history_and_independent_count_reconciliation_without_intake_work()
 {
    let (actor, _, _) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let operation = DurableRequestId::new_v7();
    let input = OpeningCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(4),
        packs: packs(8),
        identification: PhysicalIdentification {
            label: HubLabel::new("Counted opening eight").unwrap(),
            observation: note("Separately observed opening stock"),
        },
        source: SourceAttribution::Unknown,
        history: OpeningHistory::Unknown,
        counted: Occurrence::Known(now()),
        observation: note("Earlier receipt and handling are unknown"),
    };
    let opened = actor
        .record_hub_opening(
            operation.clone(),
            input.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(
        actor
            .record_hub_opening(
                operation.clone(),
                input,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap(),
        opened
    );
    assert_eq!(
        actor
            .hub_opening_operation(operation.clone())
            .await
            .unwrap(),
        Some(opened.clone())
    );
    let page = actor.hub_stock(query()).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(page.rows[0].put_away.is_none());
    assert!(opened.original.group.work.is_empty());
    let observed = actor
        .observe_hub_count(
            DurableRequestId::new_v7(),
            CountObserveCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: catalog.revision(),
                group: opened.original.group.id,
                expected_group_revision: opened.original.group.revision,
                counted_packs: packs(6),
                cutoff: Occurrence::Known(now()),
                observation: note("Fictional count six of the eight opening packs"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let review = actor.hub_count_review(observed.original.id).await.unwrap();
    assert!(review.assessment.ready());
    let posted = actor
        .post_hub_count(
            DurableRequestId::new_v7(),
            CountPostCommand {
                expected_stock_revision: review.stock_revision,
                expected_catalog_revision: review.catalog_revision,
                count: observed.original.id,
                reviewed_activity: true,
                surplus_identification: None,
                reason: note("Review establishes only the present six packs"),
            },
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(posted.original.before.work.is_empty());
    let page = actor.hub_stock(query()).await.unwrap();
    assert_eq!(page.rows[0].position.packs, packs(6));
    assert!(page.rows[0].put_away.is_none());
    assert_eq!(
        actor
            .hub_opening_operation(operation.clone())
            .await
            .unwrap(),
        Some(opened)
    );
    actor.sign_out().await.unwrap();
    assert_eq!(
        actor
            .hub_opening_operation(operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    actor.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn count_and_opening_mutations_require_current_custody_while_original_recovery_remains_read_only()
 {
    let (actor, secrets, owner) = fixture().await;
    let catalog = actor.hub_references().await.unwrap();
    let opening_operation = DurableRequestId::new_v7();
    let opening = OpeningCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(4),
        packs: packs(8),
        identification: PhysicalIdentification {
            label: HubLabel::new("Eight separately counted opening packs").unwrap(),
            observation: note("Fictional present stock"),
        },
        source: SourceAttribution::Unknown,
        history: OpeningHistory::Unknown,
        counted: Occurrence::Known(now()),
        observation: note("Earlier receipt is unknown"),
    };
    let opened = actor
        .record_hub_opening(
            opening_operation.clone(),
            opening.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let observation_operation = DurableRequestId::new_v7();
    let observe = CountObserveCommand {
        expected_stock_revision: opened.original.stock_revision,
        expected_catalog_revision: catalog.revision(),
        group: opened.original.group.id,
        expected_group_revision: opened.original.group.revision,
        counted_packs: packs(6),
        cutoff: Occurrence::Known(now()),
        observation: note("Present count is six"),
    };
    let observed = actor
        .observe_hub_count(
            observation_operation.clone(),
            observe.clone(),
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    let review = actor.hub_count_review(observed.original.id).await.unwrap();
    assert!(review.assessment.ready());
    let before = actor.hub_stock(query()).await.unwrap();
    let post = CountPostCommand {
        expected_stock_revision: review.stock_revision,
        expected_catalog_revision: review.catalog_revision,
        count: observed.original.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: note("Reviewed count with current stock"),
    };
    let credential = secrets.load(owner).await.unwrap();
    secrets
        .delete(&DurableRequestId::new_v7(), owner)
        .await
        .unwrap();
    assert_eq!(
        actor.hub_count_review(observed.original.id).await.unwrap(),
        review
    );
    assert_eq!(
        actor
            .hub_count_observation_operation(observation_operation)
            .await
            .unwrap(),
        Some(observed)
    );
    assert_eq!(
        actor
            .hub_opening_operation(opening_operation)
            .await
            .unwrap(),
        Some(opened)
    );
    let post_operation = DurableRequestId::new_v7();
    assert_eq!(
        actor
            .post_hub_count(
                post_operation.clone(),
                post.clone(),
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    let observe_operation = DurableRequestId::new_v7();
    let mut observe = observe;
    observe.expected_stock_revision = before.revision;
    assert_eq!(
        actor
            .observe_hub_count(
                observe_operation.clone(),
                observe,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    let another_opening_operation = DurableRequestId::new_v7();
    let mut opening = opening;
    opening.expected_stock_revision = before.revision;
    opening.identification.label = HubLabel::new("Different newly counted opening packs").unwrap();
    assert_eq!(
        actor
            .record_hub_opening(
                another_opening_operation.clone(),
                opening,
                actor.snapshot().revision(),
                DEFAULT_COMMAND_TIMEOUT
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::CredentialMissing
    );
    for operation in [
        post_operation.clone(),
        observe_operation,
        another_opening_operation,
    ] {
        assert!(
            actor
                .adapter
                .database()
                .hub_submission(owner, operation)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(actor.hub_stock(query()).await.unwrap(), before);
    secrets
        .put(&DurableRequestId::new_v7(), owner, credential)
        .await
        .unwrap();
    actor
        .post_hub_count(
            post_operation,
            post,
            actor.snapshot().revision(),
            DEFAULT_COMMAND_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(
        actor.hub_stock(query()).await.unwrap().rows[0]
            .position
            .packs,
        packs(6)
    );
    actor.close().await.unwrap();
}
