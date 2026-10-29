use super::*;

fn plan(group: &PhysicalStockGroup, counted: u32, catalog: &ReferenceCatalog) -> CountPostPlan {
    let (_, _, mut command) = fixture();
    command.counted_packs = packs(counted);
    let count = observation(catalog, group, &command);
    let mut current = context(catalog, group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap()
}

#[test]
fn missing_pending_packs_leave_retired_history_without_creating_a_physical_group() {
    let (catalog, group, _) = fixture();
    let result = plan(&group, 18, &catalog);
    assert_eq!(
        result.work_scope_changes().unwrap(),
        vec![CountWorkScopeChange {
            work: group.work[0].requirement.id,
            before_progress: WorkProgress::Pending,
            before_scoped: packs(20),
            on_hand_after: packs(18),
            removed: packs(2),
            removed_progress: WorkProgress::NoLongerRequired,
        }]
    );
    assert_eq!(result.before, group);
    assert_eq!(result.before.id, group.id);
    let CountEffect::Shortage { work_changes, .. } = result.effect else {
        panic!("shortage")
    };
    assert_eq!(work_changes[0].after.quantity.completed(), packs(0));
    assert_eq!(
        work_changes[0].after.quantity.no_longer_required(),
        packs(2)
    );
    assert_eq!(work_changes[0].after.quantity.remaining(), packs(18));
}

#[test]
fn missing_completed_and_retired_scope_keeps_its_original_progress_and_global_counters() {
    let (catalog, mut group, _) = fixture();
    for progress in [WorkProgress::Completed, WorkProgress::NoLongerRequired] {
        group.work[0].progress = progress;
        group.work[0].requirement.quantity = match progress {
            WorkProgress::Completed => {
                QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap()
            }
            WorkProgress::NoLongerRequired => {
                QuantityWork::restore(packs(20), packs(0), packs(20), packs(0)).unwrap()
            }
            WorkProgress::Pending => unreachable!(),
        };
        let result = plan(&group, 18, &catalog);
        let scopes = result.work_scope_changes().unwrap();
        assert_eq!(scopes[0].before_progress, progress);
        assert_eq!(scopes[0].removed_progress, progress);
        assert_eq!(
            scopes[0]
                .on_hand_after
                .checked_add(scopes[0].removed)
                .unwrap(),
            packs(20)
        );
        let CountEffect::Shortage { work_changes, .. } = &result.effect else {
            panic!("shortage")
        };
        assert!(work_changes.is_empty());
        assert_eq!(result.before.work[0].requirement, group.work[0].requirement);
    }
}

#[test]
fn scope_accounting_uses_the_counted_portion_without_rewriting_global_required_quantity() {
    let (catalog, mut group, mut command) = fixture();
    group.packs = packs(8);
    command.counted_packs = packs(6);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let result = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let scopes = result.work_scope_changes().unwrap();
    assert_eq!(
        (
            scopes[0].before_scoped,
            scopes[0].on_hand_after,
            scopes[0].removed
        ),
        (packs(8), packs(6), packs(2))
    );
    let CountEffect::Shortage { work_changes, .. } = result.effect else {
        panic!("shortage")
    };
    assert_eq!(work_changes[0].after.quantity.required(), packs(20));
    assert_eq!(work_changes[0].after.quantity.remaining(), packs(18));
}

#[test]
fn zero_count_preserves_all_removed_work_scope_and_matching_count_has_no_removed_scope() {
    let (catalog, group, _) = fixture();
    let scopes = plan(&group, 0, &catalog).work_scope_changes().unwrap();
    assert_eq!(scopes[0].on_hand_after, packs(0));
    assert_eq!(scopes[0].removed, packs(20));
    assert!(
        plan(&group, 20, &catalog)
            .work_scope_changes()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn multiple_requirements_keep_independent_removed_progress() {
    let (catalog, mut group, _) = fixture();
    let mut completed = group.work[0].clone();
    completed.requirement.id = id(13);
    completed.progress = WorkProgress::Completed;
    completed.requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    group.work.push(completed);
    let result = plan(&group, 18, &catalog);
    let scopes = result.work_scope_changes().unwrap();
    assert_eq!(scopes.len(), 2);
    assert_eq!(scopes[0].removed_progress, WorkProgress::NoLongerRequired);
    assert_eq!(scopes[1].removed_progress, WorkProgress::Completed);
    assert_ne!(scopes[0].work, scopes[1].work);
    let CountEffect::Shortage { work_changes, .. } = result.effect else {
        panic!("shortage")
    };
    assert_eq!(work_changes.len(), 1);
}
