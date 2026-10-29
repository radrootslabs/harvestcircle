use super::*;

#[test]
fn original_observation_seals_captured_facts_and_command_binding() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let input = command.fingerprint(hub(), actor(), id(20));
    let digest = count_observation_result_digest(&input, &count).unwrap();
    assert_eq!(
        digest,
        count_observation_result_digest(&input, &count).unwrap()
    );
    assert_ne!(
        digest,
        count_observation_result_digest(&[1; 32], &count).unwrap()
    );
    let mut changed = count.clone();
    changed.scope.recorded_packs = packs(21);
    assert_ne!(
        digest,
        count_observation_result_digest(&input, &changed).unwrap()
    );
    changed = count.clone();
    changed.scope.latest_physical_recorded_at = at(51);
    assert_ne!(
        digest,
        count_observation_result_digest(&input, &changed).unwrap()
    );
    changed = count.clone();
    changed.scope.previous_reconciliation = Some(id(30));
    assert_ne!(
        digest,
        count_observation_result_digest(&input, &changed).unwrap()
    );
    changed = count.clone();
    changed.scope.product.value = ReferenceValue::Product {
        label: HubLabel::new("Renamed captured product").unwrap(),
    };
    assert_ne!(
        digest,
        count_observation_result_digest(&input, &changed).unwrap()
    );
    changed = count.clone();
    changed.recorded_at = at(101);
    assert_ne!(
        digest,
        count_observation_result_digest(&input, &changed).unwrap()
    );
    changed.id = id(99);
    assert_eq!(
        count_observation_result_digest(&input, &changed),
        Err(CountError::InvalidHistory)
    );
}

fn shortage() -> (CountPostCommand, CountPostPlan) {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let command = post(&count, &current);
    let result = command
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    (command, result)
}

#[test]
fn original_reconciliation_seals_captured_stock_and_work_results() {
    let (command, result) = shortage();
    let input = command.fingerprint(hub(), actor(), id(21));
    let digest = count_post_result_digest(&input, &result).unwrap();
    assert_eq!(digest, count_post_result_digest(&input, &result).unwrap());
    assert_ne!(digest, count_post_result_digest(&[1; 32], &result).unwrap());
    let mut changed = result.clone();
    changed.before.parent = Some(id(50));
    changed.before.identification = Some(PhysicalIdentification {
        label: HubLabel::new("Original identified portion").unwrap(),
        observation: text("Captured portion identification"),
    });
    assert_ne!(digest, count_post_result_digest(&input, &changed).unwrap());
    changed = result.clone();
    changed.recorded_at = at(111);
    assert_ne!(digest, count_post_result_digest(&input, &changed).unwrap());
    changed = result.clone();
    changed.before.causal_floor = Some(at(41));
    assert_ne!(digest, count_post_result_digest(&input, &changed).unwrap());
    changed = result.clone();
    changed.id = id(99);
    assert_eq!(
        count_post_result_digest(&input, &changed),
        Err(CountError::InvalidHistory)
    );
}

#[test]
fn historical_validation_rejects_missing_work_invented_completion_and_wrong_scope() {
    let (_, result) = shortage();
    let mut changed = result.clone();
    let CountEffect::Shortage { work_changes, .. } = &mut changed.effect else {
        panic!("shortage")
    };
    work_changes.clear();
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = result.clone();
    let CountEffect::Shortage { work_changes, .. } = &mut changed.effect else {
        panic!("shortage")
    };
    work_changes[0].after.quantity =
        QuantityWork::restore(packs(20), packs(2), packs(0), packs(18)).unwrap();
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = result.clone();
    changed.before.location = id(4);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = result.clone();
    changed.count.scope.latest_physical_recorded_at = at(90);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = result;
    changed.confirmed_on_hand_at = at(79);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
}

#[test]
fn validation_binds_every_pending_work_change_exactly_once() {
    let (catalog, mut group, command) = fixture();
    let mut second = group.work[0].clone();
    second.requirement.id = id(13);
    group.work.push(second);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut result = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let CountEffect::Shortage { work_changes, .. } = &mut result.effect else {
        panic!("shortage")
    };
    work_changes[1] = work_changes[0].clone();
    assert_eq!(result.validate(), Err(CountError::InvalidHistory));
}

#[test]
fn surplus_seal_requires_unknown_source_and_new_acceptance_review() {
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(22);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut posting = post(&count, &current);
    posting.surplus_identification = Some(PhysicalIdentification {
        label: HubLabel::new("Separate surplus").unwrap(),
        observation: text("Two separately tagged extras of unknown origin"),
    });
    let mut result = posting
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let input = posting.fingerprint(hub(), actor(), id(21));
    let digest = count_post_result_digest(&input, &result).unwrap();
    let CountEffect::Surplus { identification, .. } = &mut result.effect else {
        panic!("surplus")
    };
    identification.observation = text("Different captured surplus observation");
    assert_ne!(digest, count_post_result_digest(&input, &result).unwrap());
    let CountEffect::Surplus { acceptance, .. } = &mut result.effect else {
        panic!("surplus")
    };
    *acceptance = crate::hub::treatment::AcceptanceStatus::Accepted;
    assert_eq!(
        count_post_result_digest(&input, &result),
        Err(CountError::InvalidHistory)
    );
}

#[test]
fn posting_retains_its_own_reference_cut_without_rewriting_observation_labels() {
    let (mut catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    for (record, value) in [
        (
            id(1),
            ReferenceValue::Product {
                label: HubLabel::new("Posting apples").unwrap(),
            },
        ),
        (
            id(3),
            ReferenceValue::Location {
                label: HubLabel::new("Posting Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
    ] {
        let revision = catalog.get(record).unwrap().revision;
        catalog
            .revise(catalog.revision(), record, revision, value)
            .unwrap();
    }
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let result = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    assert_eq!(
        result.references.product.value.label().as_str(),
        "Posting apples"
    );
    assert_eq!(
        result.references.location.value.label().as_str(),
        "Posting Receiving"
    );
    assert_eq!(result.references.product.revision, rev(2));
    assert_eq!(result.references.location.revision, rev(2));
    assert_eq!(result.references.variant, count.scope.variant);
    assert_eq!(
        result.count.scope.product.value.label().as_str(),
        "Fictional apples"
    );
    assert_eq!(
        result.count.scope.location.value.label().as_str(),
        "Receiving"
    );
    let original = result.clone();
    catalog
        .revise(
            catalog.revision(),
            id(1),
            rev(2),
            ReferenceValue::Product {
                label: HubLabel::new("Later apples").unwrap(),
            },
        )
        .unwrap();
    assert_eq!(result, original);
    assert_ne!(result.references.product, *catalog.get(id(1)).unwrap());
}

#[test]
fn posting_digest_binds_reference_values_and_versions_at_the_posting_cut() {
    let (command, result) = shortage();
    let input = command.fingerprint(hub(), actor(), id(21));
    let digest = count_post_result_digest(&input, &result).unwrap();
    for kind in [ReferenceKind::Product, ReferenceKind::Location] {
        let mut changed = result.clone();
        let reference = match kind {
            ReferenceKind::Product => &mut changed.references.product,
            ReferenceKind::Location => &mut changed.references.location,
            _ => unreachable!(),
        };
        match &mut reference.value {
            ReferenceValue::Product { label } | ReferenceValue::Location { label, .. } => {
                *label = HubLabel::new("Different original posting label").unwrap()
            }
            _ => unreachable!(),
        }
        assert_ne!(digest, count_post_result_digest(&input, &changed).unwrap());
        changed = result.clone();
        match kind {
            ReferenceKind::Product => changed.references.product.revision = rev(2),
            ReferenceKind::Location => changed.references.location.revision = rev(2),
            _ => unreachable!(),
        }
        assert_ne!(digest, count_post_result_digest(&input, &changed).unwrap());
    }
}

#[test]
fn posting_original_rejects_wrong_reference_scope_future_versions_and_changed_pack_or_purpose() {
    let (_, original) = shortage();
    let mut changed = original.clone();
    changed.references.product.id = id(99);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = original.clone();
    changed.references.location.revision = rev(changed.catalog_revision.value() + 1);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = original.clone();
    changed.references.variant.revision = rev(2);
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = original.clone();
    changed.references.variant.value = ReferenceValue::Pack {
        product: id(1),
        label: HubLabel::new("Changed immutable pack").unwrap(),
        nominal_grams: None,
    };
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
    changed = original;
    changed.references.location.value = ReferenceValue::Location {
        label: HubLabel::new("Receiving").unwrap(),
        purpose: LocationPurpose::Storage,
    };
    assert_eq!(changed.validate(), Err(CountError::InvalidHistory));
}
