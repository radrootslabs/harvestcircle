use super::*;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn revision(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn node(id_value: u8, parent: Option<u8>, born: u64, quantity: u32) -> PhysicalLineageNode {
    PhysicalLineageNode {
        id: id(id_value),
        parent: parent.map(id),
        trace: id(100),
        variant: id(101),
        created_revision: revision(born),
        packs: packs(quantity),
    }
}
fn graph(at: u64, nodes: Vec<PhysicalLineageNode>) -> PhysicalLineage {
    PhysicalLineage::new(
        HubId::from_bytes([1; 16]).unwrap(),
        id(100),
        id(101),
        revision(at),
        nodes,
    )
    .unwrap()
}

#[test]
fn captured_population_inherits_later_splits_but_excludes_already_detached_portions() {
    let original = graph(2, vec![node(1, None, 2, 20)]);
    let all = original.capture(&[id(1)]).unwrap();
    let separated = graph(3, vec![node(1, None, 2, 12), node(2, Some(1), 3, 8)]);
    let twelve = separated.capture(&[id(1)]).unwrap();
    let eight = separated.capture(&[id(2)]).unwrap();
    let nested = graph(
        4,
        vec![
            node(1, None, 2, 12),
            node(2, Some(1), 3, 3),
            node(3, Some(2), 4, 5),
        ],
    );
    assert_eq!(nested.resolve(&all).unwrap().packs(), packs(20));
    assert_eq!(nested.resolve(&twelve).unwrap().packs(), packs(12));
    assert_eq!(nested.resolve(&eight).unwrap().packs(), packs(8));
    assert_eq!(eight.anchors()[0].captured_packs, packs(8));
    let five = nested.capture(&[id(3)]).unwrap();
    let union = nested.resolve_union(&[eight.clone(), five]).unwrap();
    assert_eq!(union.packs(), packs(8));
    assert_eq!(
        union
            .groups()
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![id(2), id(3)]
    );
    assert_eq!(
        nested
            .resolve_union(&[eight.clone(), eight])
            .unwrap()
            .packs(),
        packs(8)
    );
}

#[test]
fn a_new_descendant_of_an_excluded_branch_does_not_reenter_the_selection() {
    let first = graph(3, vec![node(1, None, 2, 12), node(2, Some(1), 3, 8)]);
    let selected = first.capture(&[id(1)]).unwrap();
    let later = graph(
        6,
        vec![
            node(1, None, 2, 10),
            node(2, Some(1), 3, 3),
            node(3, Some(2), 4, 3),
            node(4, Some(3), 5, 2),
            node(5, Some(1), 6, 2),
        ],
    );
    let resolved = later.resolve(&selected).unwrap();
    assert_eq!(resolved.packs(), packs(12));
    assert_eq!(
        resolved
            .groups()
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![id(1), id(5)]
    );
    assert_eq!(selected.captured_revision(), revision(3));
    assert_eq!(selected.anchors()[0].captured_packs, packs(12));
}

#[test]
fn recording_unrelated_work_or_movement_does_not_change_a_groups_birth() {
    let before = graph(3, vec![node(1, None, 2, 12), node(2, Some(1), 3, 8)]);
    let selected = before.capture(&[id(1)]).unwrap();
    let later = graph(50, before.nodes().to_vec());
    let resolved = later.resolve(&selected).unwrap();
    assert_eq!(resolved.packs(), packs(12));
    assert_eq!(resolved.revision(), revision(50));
    assert_eq!(resolved.groups().len(), 1);
}

#[test]
fn retained_empty_ancestors_preserve_selection_without_minting_current_stock() {
    let before = graph(2, vec![node(1, None, 2, 20)]);
    let selected = before.capture(&[id(1)]).unwrap();
    let empty = graph(5, vec![node(1, None, 2, 0), node(2, Some(1), 3, 0)]);
    let resolved = empty.resolve(&selected).unwrap();
    assert_eq!(resolved.packs(), packs(0));
    assert!(resolved.groups().is_empty());
    assert!(empty.capture(&[id(1)]).is_err());
    assert_eq!(selected.anchors()[0].captured_packs, packs(20));
}

#[test]
fn historical_gaps_or_replaced_birth_identity_never_become_empty_success() {
    let later = graph(5, vec![node(1, None, 2, 20)]);
    let selected = later.capture(&[id(1)]).unwrap();
    let older = graph(4, vec![node(1, None, 2, 20)]);
    assert_eq!(
        older.resolve(&selected),
        Err(PhysicalScopeError::HistoricalGap)
    );
    let missing = graph(6, vec![node(2, None, 2, 20)]);
    assert_eq!(
        missing.resolve(&selected),
        Err(PhysicalScopeError::HistoricalGap)
    );
    let replaced = graph(6, vec![node(1, None, 3, 20)]);
    assert_eq!(
        replaced.resolve(&selected),
        Err(PhysicalScopeError::HistoricalGap)
    );
}

#[test]
fn selections_are_canonical_and_cannot_cross_hubs_or_native_source_identity() {
    let current = graph(3, vec![node(1, None, 2, 12), node(2, Some(1), 3, 8)]);
    let selected = current.capture(&[id(2), id(1)]).unwrap();
    assert_eq!(selected, current.capture(&[id(1), id(2)]).unwrap());
    assert_eq!(current.resolve(&selected).unwrap().packs(), packs(20));
    assert_eq!(
        current.capture(&[]),
        Err(PhysicalScopeError::InvalidSelection)
    );
    assert_eq!(
        current.capture(&[id(1), id(1)]),
        Err(PhysicalScopeError::InvalidSelection)
    );
    assert_eq!(
        current.capture(&[id(99)]),
        Err(PhysicalScopeError::InvalidSelection)
    );
    let foreign = PhysicalLineage::new(
        HubId::from_bytes([2; 16]).unwrap(),
        id(100),
        id(101),
        revision(3),
        current.nodes().to_vec(),
    )
    .unwrap();
    assert_eq!(
        foreign.resolve(&selected),
        Err(PhysicalScopeError::InvalidSelection)
    );
    assert!(
        current
            .resolve_union(&vec![selected; HUB_PHYSICAL_SCOPES + 1])
            .is_err()
    );
    assert_eq!(current.resolve_union(&[]).unwrap().packs(), packs(0));
}

#[test]
fn incomplete_cyclic_or_cross_source_lineage_is_rejected_before_resolution() {
    for nodes in [
        vec![node(1, None, 2, 20), node(1, Some(1), 3, 1)],
        vec![node(1, None, 2, 12), node(2, None, 3, 8)],
        vec![node(1, None, 2, 12), node(2, Some(99), 3, 8)],
        vec![node(1, Some(2), 2, 12), node(2, Some(1), 3, 8)],
        vec![node(1, None, 3, 12), node(2, Some(1), 2, 8)],
        vec![node(1, None, 2, 12), node(2, Some(1), 7, 8)],
        vec![
            node(1, None, 2, 12),
            PhysicalLineageNode {
                trace: id(99),
                ..node(2, Some(1), 3, 8)
            },
        ],
        vec![
            node(1, None, 2, 12),
            PhysicalLineageNode {
                variant: id(99),
                ..node(2, Some(1), 3, 8)
            },
        ],
    ] {
        assert_eq!(
            PhysicalLineage::new(
                HubId::from_bytes([1; 16]).unwrap(),
                id(100),
                id(101),
                revision(6),
                nodes
            ),
            Err(PhysicalScopeError::InvalidLineage)
        );
    }
    assert_eq!(
        PhysicalLineage::new(
            HubId::from_bytes([1; 16]).unwrap(),
            id(100),
            id(101),
            revision(6),
            vec![node(1, None, 2, 1_000_000), node(2, Some(1), 3, 1)]
        ),
        Err(PhysicalScopeError::QuantityOverflow)
    );
}

#[test]
fn exact_lineage_and_union_limits_preserve_all_groups_without_duplicate_quantity() {
    let original = graph(2, vec![node(1, None, 2, 256)]);
    let scope = original.capture(&[id(1)]).unwrap();
    let mut nodes = vec![node(1, None, 2, 1)];
    for index in 1..=255_u16 {
        let mut bytes = [0; 16];
        bytes[14..].copy_from_slice(&index.to_be_bytes());
        nodes.push(PhysicalLineageNode {
            id: HubRecordId::from_bytes(bytes).unwrap(),
            created_revision: revision(u64::from(index) + 2),
            ..node(2, Some(1), 3, 1)
        });
    }
    let current = graph(300, nodes.clone());
    let resolved = current
        .resolve_union(&vec![scope; HUB_PHYSICAL_SCOPES])
        .unwrap();
    assert_eq!(resolved.groups().len(), HUB_LINEAGE_GROUPS);
    assert_eq!(resolved.packs(), packs(256));
    let mut bytes = [0; 16];
    bytes[14..].copy_from_slice(&256_u16.to_be_bytes());
    nodes.push(PhysicalLineageNode {
        id: HubRecordId::from_bytes(bytes).unwrap(),
        ..node(2, Some(1), 299, 1)
    });
    assert_eq!(
        PhysicalLineage::new(
            HubId::from_bytes([1; 16]).unwrap(),
            id(100),
            id(101),
            revision(300),
            nodes
        ),
        Err(PhysicalScopeError::Capacity)
    );
}
