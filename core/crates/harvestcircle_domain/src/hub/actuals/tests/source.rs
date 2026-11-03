use super::*;

pub(super) fn witness(f: &Fixture) -> HistoricalActualSource {
    HistoricalActualSource {
        actor: owner(),
        operation: id(200),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(f.original.stock_revision.value() - 1).unwrap(),
        catalog_revision: f.catalog.revision(),
        group: (&f.group).into(),
        location: f.catalog.get(f.group.location).unwrap().clone(),
        occurred: known(100),
        recorded_at: at(100),
    }
}
fn resolve(
    f: &Fixture,
    source: &HistoricalActualSource,
    activity: &[ActualPhysicalActivity],
) -> Result<ResolvedActualSource, ActualReportError> {
    ActualSourceContext {
        source,
        lineage: &f.graph,
        balancing_group: &f.group,
        subsequent_activity: activity,
    }
    .resolve(&f.original, f.facts.occurred, at(170))
}
fn event(
    f: &Fixture,
    kind: ActualPhysicalActivityKind,
    occurred: Occurrence,
) -> ActualPhysicalActivity {
    ActualPhysicalActivity {
        actor: owner(),
        sequence: 10,
        operation: id(201),
        hub: f.graph.hub(),
        trace: f.group.trace,
        group: f.group.id,
        location: None,
        stock_revision: f.graph.revision(),
        kind,
        occurred,
        recorded_at: at(145),
    }
}

#[test]
fn historical_snapshot_can_match_the_report_when_current_location_has_changed_later() {
    let mut f = Fixture::new(known(120));
    let source = witness(&f);
    f.original.report.sources = ActualReportSources::EstablishedClaim(ReportedActualSource {
        trace: source.group.trace,
        group: Some(source.group.id),
        location: Some(source.location.id),
    });
    let target = f.group.work[0].requirement.target.as_ref().unwrap().id;
    f.group.location = target;
    f.group.work[0].progress = WorkProgress::Completed;
    f.group.work[0].requirement.quantity =
        crate::hub::stock::QuantityWork::restore(packs(20), packs(6), packs(0), packs(14)).unwrap();
    let moved = event(&f, ActualPhysicalActivityKind::LocationChanged, known(130));
    let resolved = resolve(&f, &source, &[moved]).unwrap();
    assert_eq!(resolved.historical().location.id, source.location.id);
    assert_ne!(resolved.historical().location.id, f.group.location);
    assert_eq!(resolved.balancing_group(), f.group.id);
    assert_eq!(f.group.packs, packs(6));
}

#[test]
fn earlier_or_uncertain_location_changes_require_a_different_proven_source_witness() {
    let f = Fixture::new(known(120));
    let source = witness(&f);
    for occurred in [known(110), interval(115, 125), Occurrence::Unknown] {
        let changed = event(&f, ActualPhysicalActivityKind::LocationChanged, occurred);
        assert_eq!(
            resolve(&f, &source, &[changed]),
            Err(ActualReportError::HistoricalSourceUnresolved)
        );
    }
}

#[test]
fn later_reconciled_count_blocks_a_second_reduction_of_already_accounted_missing_stock() {
    let f = Fixture::new(known(120));
    let source = witness(&f);
    let counted = event(&f, ActualPhysicalActivityKind::CountReconciled, known(140));
    assert_eq!(
        resolve(&f, &source, &[counted]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
}

#[test]
fn narrowed_original_arrival_cannot_lend_an_expanded_current_cohort_even_when_requested_packs_fit()
{
    let f = Fixture::new(known(120));
    let mut source = witness(&f);
    source.group.packs = packs(5);
    assert_eq!(f.group.packs, packs(6));
    assert!(f.facts.packs.value() <= 5);
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
}

#[test]
fn old_root_capture_includes_later_descendants_but_later_root_capture_excludes_detached_groups() {
    let f = Fixture::new(known(120));
    let mut source = witness(&f);
    let root = f.group.parent.unwrap();
    let node = f.graph.node(root).unwrap();
    source.group.id = root;
    source.group.parent = None;
    source.group.packs = packs(20);
    source.group.revision = HubRevision::INITIAL;
    source.stock_revision = node.created_revision;
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
    let mut birth = event(&f, ActualPhysicalActivityKind::GroupBorn, known(105));
    birth.stock_revision = f.graph.node(f.group.id).unwrap().created_revision;
    birth.location = Some(source.location.id);
    assert!(resolve(&f, &source, &[birth.clone()]).is_ok());
    birth.occurred = known(125);
    assert_eq!(
        resolve(&f, &source, &[birth]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
    source.stock_revision = HubRevision::from_value(f.original.stock_revision.value() - 1).unwrap();
    source.group.packs = node.packs;
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
}

#[test]
fn source_and_occurrence_unknowns_never_silently_select_an_unsupported_historical_group() {
    let mut f = Fixture::new(known(120));
    let source = witness(&f);
    f.original.report.sources = ActualReportSources::Possible(vec![ReportedActualSource {
        trace: id(210),
        group: None,
        location: None,
    }]);
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
    f.original.report.sources = ActualReportSources::Unknown;
    f.original.report.occurred = Occurrence::Unknown;
    f.facts.occurred = Occurrence::Unknown;
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
    f.facts.occurred = known(90);
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::HistoricalSourceUnresolved)
    );
}

#[test]
fn resolved_source_does_not_rewrite_an_asserted_variant_or_occurrence() {
    let mut f = Fixture::new(known(120));
    let source = witness(&f);
    f.original.report.variant = Some(id(240));
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::ConflictingEvidence)
    );
    f.original.report.variant = None;
    f.facts.occurred = known(121);
    assert_eq!(
        resolve(&f, &source, &[]),
        Err(ActualReportError::ConflictingEvidence)
    );
}

#[test]
fn native_activity_bounds_order_scope_and_original_snapshot_damage_fail_closed() {
    let f = Fixture::new(known(120));
    let source = witness(&f);
    let mut damaged = source.clone();
    damaged.location = f.catalog.get(f.group.variant).unwrap().clone();
    assert_eq!(
        resolve(&f, &damaged, &[]),
        Err(ActualReportError::InvalidHistory)
    );
    let mut damaged = source.clone();
    damaged.group.packs = packs(0);
    assert_eq!(
        resolve(&f, &damaged, &[]),
        Err(ActualReportError::InvalidHistory)
    );
    let mut damaged = source.clone();
    damaged.group.parent = Some(damaged.group.id);
    assert_eq!(
        resolve(&f, &damaged, &[]),
        Err(ActualReportError::InvalidHistory)
    );
    let event = event(
        &f,
        ActualPhysicalActivityKind::OtherPhysicalEffect,
        known(130),
    );
    let mut wrong = event.clone();
    wrong.hub = HubId::from_bytes([220; 16]).unwrap();
    assert_eq!(
        resolve(&f, &source, &[wrong]),
        Err(ActualReportError::InvalidNativeContext)
    );
    assert_eq!(
        resolve(&f, &source, &[event.clone(), event.clone()]),
        Err(ActualReportError::InvalidNativeContext)
    );
    let mut next = event.clone();
    next.sequence += 1;
    assert!(resolve(&f, &source, &[event.clone(), next.clone()]).is_ok());
    next.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_eq!(
        resolve(&f, &source, &[event, next]),
        Err(ActualReportError::InvalidNativeContext)
    );
}
