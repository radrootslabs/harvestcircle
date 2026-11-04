//! A historical source can have several valid present descendants. The exact
//! physical balancing choice must belong to the reviewed command intent.
use super::*;
use crate::hub::actuals::{ActualPhysicalActivity, ActualPhysicalActivityKind};
use crate::hub::movement::PhysicalIdentification;
use crate::hub::portions::PhysicalLineageNode;
use crate::hub::reference::HubLabel;

fn descendants() -> (Fixture, PhysicalStockGroup, Vec<ActualPhysicalActivity>) {
    let mut fixture = Fixture::new();
    // Historical source snapshot12, six already departed, then the remaining
    // six split3/3. The reused trace/work fixture retains its original20 baseline;
    // this is a pure scope proof, not native split/move conservation acceptance.
    // Both current candidates have revision3. The child moved away and back
    // after the claimed departure without lowering its current causal floor.
    fixture.group.packs = packs(3);
    fixture.group.revision = rev(3);
    fixture.group.causal_floor = Some(at(110));
    let mut child = fixture.group.clone();
    child.id = id(61);
    child.parent = Some(fixture.group.id);
    child.causal_floor = Some(at(112));
    child.identification = Some(PhysicalIdentification {
        label: HubLabel::new("Separate balancing descendant").unwrap(),
        observation: note(),
    });
    let mut nodes = fixture.graph.nodes().to_vec();
    nodes
        .iter_mut()
        .find(|node| node.id == fixture.group.id)
        .unwrap()
        .packs = packs(3);
    nodes.push(PhysicalLineageNode {
        id: child.id,
        parent: child.parent,
        trace: child.trace,
        variant: child.variant,
        created_revision: rev(7),
        packs: child.packs,
        recovery: None,
    });
    fixture.graph = PhysicalLineage::new(
        fixture.graph.hub(),
        fixture.graph.trace(),
        fixture.graph.variant(),
        fixture.graph.revision(),
        nodes,
    )
    .unwrap();
    let other_location = fixture
        .catalog
        .records()
        .find(|record| {
            record.value.kind() == ReferenceKind::Location && record.id != fixture.group.location
        })
        .unwrap()
        .id;
    let activity = [
        (
            7,
            110,
            130,
            fixture.group.location,
            ActualPhysicalActivityKind::GroupBorn,
        ),
        (
            8,
            111,
            140,
            other_location,
            ActualPhysicalActivityKind::LocationChanged,
        ),
        (
            9,
            112,
            145,
            fixture.group.location,
            ActualPhysicalActivityKind::LocationChanged,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (revision, occurred, recorded, location, kind))| ActualPhysicalActivity {
            sequence: u32::try_from(index + 1).unwrap(),
            operation: id(62 + u8::try_from(index).unwrap()),
            actor: owner(),
            hub: fixture.graph.hub(),
            stock_revision: rev(revision),
            trace: child.trace,
            group: child.id,
            kind,
            location: Some(location),
            occurred: Occurrence::Known(at(occurred)),
            recorded_at: at(recorded),
        },
    )
    .collect();
    (fixture, child, activity)
}

#[test]
fn additional_intent_cannot_silently_choose_another_valid_same_revision_descendant() {
    let (fixture, child, activity) = descendants();
    let command = fixture.command();
    let mut context = fixture.context();
    context.historical_source.subsequent_activity = &activity;
    let original = command.plan(&context, id(250), owner(), at(170)).unwrap();
    assert_eq!(original.reduction.before.id, fixture.group.id);
    assert_eq!(original.reduction.actual_departed, packs(2));
    context.historical_source.balancing_group = &child;
    assert_eq!(
        command
            .plan(&context, id(250), owner(), at(170))
            .unwrap_err(),
        QuantityEffectError::InvalidScope
    );
    let mut explicit_child = command.clone();
    explicit_child.balancing_group = child.id;
    let chosen = explicit_child
        .plan(&context, id(250), owner(), at(170))
        .unwrap();
    assert_eq!(chosen.reduction.before.id, child.id);
    assert_eq!(chosen.reduction.actual_departed, packs(2));
    assert_eq!(chosen.reduction.on_hand_after, packs(1));
    assert_eq!(chosen.after_causal_floor, Some(at(112)));
    assert_ne!(
        command.fingerprint(fixture.graph.hub(), owner(), id(250)),
        explicit_child.fingerprint(fixture.graph.hub(), owner(), id(250))
    );
    context.historical_source.balancing_group = &fixture.group;
    assert_eq!(
        explicit_child
            .plan(&context, id(250), owner(), at(170))
            .unwrap_err(),
        QuantityEffectError::InvalidScope
    );
}
