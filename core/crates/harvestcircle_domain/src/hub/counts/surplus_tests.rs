use super::*;
use crate::hub::{
    stock::{OpeningIdentities, PopulationOrigin},
    treatment::AcceptanceStatus,
};

fn surplus(
    catalog: &ReferenceCatalog,
    group: &PhysicalStockGroup,
    count: &CountObservation,
) -> CountPostPlan {
    let mut current = context(catalog, group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut posting = post(count, &current);
    posting.surplus_identification = Some(PhysicalIdentification {
        label: HubLabel::new("Separately tagged extras").unwrap(),
        observation: text("Two extra crates physically identified separately from the twenty"),
    });
    posting
        .plan(count, &current, id(21), actor(), at(110))
        .unwrap()
}

#[test]
fn count_surplus_is_new_unknown_stock_without_a_delivery_or_inherited_work_or_acceptance() {
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(22);
    let count = observation(&catalog, &group, &command);
    let before = group.clone();
    let plan = surplus(&catalog, &group, &count);
    let new = plan.surplus().unwrap().unwrap();
    assert_eq!(new.population.original_packs, packs(2));
    assert_eq!(
        new.population.origin,
        PopulationOrigin::CountedSurplus {
            count: count.id,
            related_population: group.trace
        }
    );
    assert_eq!(
        new.population.source,
        crate::hub::reference::SourceAttribution::Unknown
    );
    assert!(new.population.source_needs_review());
    assert!(
        new.population.supplier.is_none()
            && new.population.producer.is_none()
            && new.population.source_issuer.is_none()
    );
    assert!(new.population.planned_location.is_none());
    assert_eq!(new.population.occurred, Occurrence::Known(at(80)));
    assert_eq!(new.population.recorded_at, at(110));
    assert_eq!(new.group.packs, packs(2));
    assert_eq!(new.group.trace, new.population.id);
    assert!(new.group.parent.is_none() && new.group.work.is_empty());
    assert_eq!(
        new.group.identification.as_ref().unwrap().label.as_str(),
        "Separately tagged extras"
    );
    assert_eq!(new.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(group, before);
    assert_eq!(plan.before, before);
    assert_eq!(count.scope.recorded_packs, packs(20));
}

#[test]
fn surplus_identity_is_stable_and_distinct_from_reconciliation_opening_and_original_stock() {
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(22);
    let count = observation(&catalog, &group, &command);
    let plan = surplus(&catalog, &group, &count);
    let a = plan.surplus().unwrap().unwrap();
    assert_eq!(plan.surplus().unwrap().unwrap(), a);
    let ids = CountSurplusIdentities::from_operation(plan.operation).unwrap();
    assert_eq!(a.population.id, ids.population);
    assert_eq!(a.group.id, ids.group);
    assert_ne!(ids, CountSurplusIdentities::from_operation(id(22)).unwrap());
    let opening = OpeningIdentities::from_operation_bytes(plan.operation).unwrap();
    let values = [
        ids.population,
        ids.group,
        plan.id,
        count.id,
        group.id,
        group.trace,
        opening.population,
        opening.group,
    ];
    assert_eq!(
        values
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        values.len()
    );
}

#[test]
fn surplus_keeps_posting_reference_names_and_nonpositive_variance_creates_no_population() {
    let (mut catalog, group, mut command) = fixture();
    let shortage = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(shortage.id);
    assert!(
        post(&shortage, &current)
            .plan(&shortage, &current, id(21), actor(), at(110))
            .unwrap()
            .surplus()
            .unwrap()
            .is_none()
    );
    command.counted_packs = packs(22);
    let count = observation(&catalog, &group, &command);
    catalog
        .revise(
            catalog.revision(),
            id(1),
            rev(1),
            ReferenceValue::Product {
                label: HubLabel::new("Posting-time apples").unwrap(),
            },
        )
        .unwrap();
    let plan = surplus(&catalog, &group, &count);
    let new = plan.surplus().unwrap().unwrap();
    assert_eq!(
        new.population.product.value.label().as_str(),
        "Posting-time apples"
    );
    assert_eq!(new.population.product.revision, rev(2));
    assert_eq!(
        plan.count.scope.product.value.label().as_str(),
        "Fictional apples"
    );
    assert_eq!(plan.count.scope.product.revision, rev(1));
}
