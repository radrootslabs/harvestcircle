use super::*;
use crate::hub::reference::{
    HubLabel, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
};
use crate::hub::stock::{OpeningIdentities, ReceiptIdentities};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note(v: &str) -> SourceProvenance {
    SourceProvenance::new(v).unwrap()
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
struct Fixture {
    catalog: ReferenceCatalog,
    departure: DepartureRecord,
    totals: Vec<EstablishedReturnTotal>,
    command: ReturnCommand,
    ids: ReturnIdentities,
}
impl Fixture {
    fn new(lines: &[u32]) -> Self {
        let (catalog, departure) = crate::hub::departure::tests::return_policy_fixture(lines);
        let totals = departure
            .paper
            .versions
            .last()
            .unwrap()
            .material
            .lines
            .iter()
            .map(|line| EstablishedReturnTotal {
                original_group: line.group,
                packs: packs(0),
            })
            .collect();
        let command = ReturnCommand {
            expected_stock_revision: departure.stock_revision,
            expected_catalog_revision: catalog.revision(),
            variant: id(2),
            location: id(3),
            returned_by: None,
            source: SourceAttribution::Unknown,
            packs: packs(2),
            identification: PhysicalIdentification {
                label: HubLabel::new("Returned two").unwrap(),
                observation: note("Two tagged original packs actually received back"),
            },
            planned_location: None,
            departure: Some(ReturnDepartureClaim {
                departure: departure.id,
                group: Some(id(11)),
            }),
            occurred: Occurrence::Unknown,
            observation: note("Return is new custody, not reversal of original departure"),
        };
        Self {
            catalog,
            departure,
            totals,
            command,
            ids: ReturnIdentities::from_operation_bytes(id(100)).unwrap(),
        }
    }
    fn context(&self) -> ReturnContext<'_> {
        ReturnContext {
            hub: self.departure.hub,
            stock_revision: self.command.expected_stock_revision,
            catalog: &self.catalog,
            departure: Some(&self.departure),
            established_return_totals: &self.totals,
        }
    }
    fn plan(&self) -> ReturnPlan {
        self.command
            .plan(&self.context(), self.ids, owner(), at(150))
            .unwrap()
    }
}
#[test]
fn return_two_keeps_departure_six_and_original_twenty_and_creates_separate_review_required_custody()
{
    let f = Fixture::new(&[6]);
    let original = f.departure.clone();
    let plan = f.plan();
    let prior = &f.departure.paper.versions.last().unwrap().material.lines[0];
    assert_eq!(prior.packs, packs(6));
    assert_eq!(prior.trace.original_packs, packs(20));
    assert_eq!(
        packs(20)
            .checked_sub(prior.packs)
            .unwrap()
            .checked_add(plan.intake.packs)
            .unwrap(),
        packs(16)
    );
    assert_eq!(plan.intake.packs, packs(2));
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(plan.intake.source, SourceAttribution::Unknown);
    assert!(plan.intake.returned_by.is_none());
    assert!(plan.group.work.is_empty());
    assert!(plan.intake_work.is_none());
    assert_eq!(plan.group.parent, None);
    assert_ne!(plan.group.trace, prior.trace.id);
    assert_ne!(plan.group.id, prior.group);
    assert_eq!(plan.linkage.established_group(), Some(prior.group));
    assert_eq!(plan.linkage.timing, ReturnTiming::Uncertain);
    assert_eq!(f.departure, original);
    assert_eq!(plan.linkage.claimed_source.unwrap().original, *prior);
}
#[test]
fn known_departure_does_not_invent_an_original_group_even_with_one_candidate() {
    for lines in [&[6][..], &[6, 4][..]] {
        let mut f = Fixture::new(lines);
        f.command.departure.as_mut().unwrap().group = None;
        let plan = f.plan();
        assert_eq!(plan.linkage.plausible_sources.len(), lines.len());
        assert_eq!(
            plan.linkage.resolution,
            ReturnLinkResolution::Unresolved(UnresolvedReturnLink::OriginalGroupUnknown)
        );
        assert!(plan.linkage.departure.is_some());
        assert!(plan.linkage.established_group().is_none());
        assert_eq!(plan.intake.source, SourceAttribution::Unknown);
        assert_eq!(plan.intake.packs, packs(2));
    }
}
#[test]
fn unknown_and_unrecorded_departure_keep_new_custody_without_false_history() {
    let mut f = Fixture::new(&[6]);
    f.command.departure = None;
    let context = ReturnContext {
        departure: None,
        established_return_totals: &[],
        ..f.context()
    };
    let plan = f.command.plan(&context, f.ids, owner(), at(150)).unwrap();
    assert_eq!(plan.linkage.resolution, ReturnLinkResolution::Unknown);
    assert!(plan.linkage.departure.is_none());
    f.command.departure = Some(ReturnDepartureClaim {
        departure: id(90),
        group: Some(id(11)),
    });
    let context = ReturnContext {
        departure: None,
        established_return_totals: &[],
        ..f.context()
    };
    let plan = f.command.plan(&context, f.ids, owner(), at(150)).unwrap();
    assert_eq!(plan.linkage.claim, f.command.departure);
    assert_eq!(plan.linkage.resolution, ReturnLinkResolution::Unrecorded);
    assert!(plan.linkage.plausible_sources.is_empty());
    assert_eq!(plan.intake.packs, packs(2));
}
#[test]
fn cumulative_exact_scope_is_bounded_but_inconsistent_linkage_does_not_discard_observed_return() {
    let mut f = Fixture::new(&[6]);
    f.totals[0].packs = packs(4);
    assert_eq!(
        f.plan().linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    f.totals[0].packs = packs(5);
    let plan = f.plan();
    assert_eq!(
        plan.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::QuantityConflict)
    );
    assert_eq!(plan.intake.packs, packs(2));
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
    assert_eq!(plan.linkage.established_group(), None);
    assert_eq!(plan.linkage.claim, f.command.departure);
    f.totals[0].packs = packs(6);
    assert!(f.plan().linkage.established_group().is_none());
}
#[test]
fn returning_party_source_original_recipient_and_acceptance_are_independent() {
    let mut f = Fixture::new(&[6]);
    f.command.returned_by = Some(id(5));
    f.command.source = SourceAttribution::Referenced {
        source: SourceReference {
            id: f.ids.source,
            issuer: Some(id(4)),
            kind: SourceReferenceKind::Other,
            code: SourceCode::new("Return-label").unwrap(),
            provenance: note("Manual separate label observation"),
        },
        certainty: SourceCertainty::ReportedPossible,
    };
    let plan = f.plan();
    assert_eq!(plan.intake.returned_by.unwrap().id, id(5));
    assert_eq!(plan.intake.source_issuer.unwrap().id, id(4));
    assert_eq!(plan.linkage.departure.unwrap().recipient.id, id(4));
    assert_eq!(plan.intake.source, f.command.source);
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
}
#[test]
fn requested_return_work_is_fresh_pending_not_resurrected_departure_credit() {
    let mut f = Fixture::new(&[6]);
    f.command.planned_location = Some(id(6));
    let plan = f.plan();
    let work = plan.intake_work.as_ref().unwrap();
    assert_eq!(work.quantity, QuantityWork::pending(packs(2)));
    assert_eq!(work.trace, plan.intake.population);
    assert_eq!(work.target.as_ref().unwrap().id, id(6));
    assert_eq!(plan.group.work[0].progress, WorkProgress::Pending);
    assert_eq!(plan.group.work[0].requirement, *work);
    assert_ne!(work.id, f.departure.work_changes[0].before.id);
    assert_eq!(
        f.departure.work_changes[0]
            .after
            .quantity
            .no_longer_required(),
        packs(6)
    );
    assert_eq!(
        f.departure.work_changes[0].after.quantity.remaining(),
        packs(14)
    );
    f.command.planned_location = Some(f.command.location);
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::InvalidPlannedLocation)
    );
}
#[test]
fn originals_keep_old_labels_while_a_new_return_uses_the_current_catalog_cut() {
    let mut f = Fixture::new(&[6]);
    let old = f.plan();
    let product = f
        .catalog
        .require_kind(id(1), ReferenceKind::Product)
        .unwrap()
        .clone();
    f.catalog
        .revise(
            f.catalog.revision(),
            id(1),
            product.revision,
            ReferenceValue::Product {
                label: HubLabel::new("Renamed current apples").unwrap(),
            },
        )
        .unwrap();
    f.command.expected_catalog_revision = f.catalog.revision();
    f.ids = ReturnIdentities::from_operation_bytes(id(101)).unwrap();
    let new = f.plan();
    assert_eq!(old.intake.product, product);
    assert_ne!(new.intake.product, product);
    assert_eq!(
        new.linkage.claimed_source.unwrap().original.product,
        product
    );
    assert_eq!(old.linkage.departure, new.linkage.departure);
}
#[test]
fn complete_native_line_totals_and_bound_departure_are_required() {
    let mut f = Fixture::new(&[6, 4]);
    let saved = f.totals.clone();
    f.totals.pop();
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::InvalidNativeContext)
    );
    f.totals = saved.clone();
    f.totals[1].original_group = f.totals[0].original_group;
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::InvalidNativeContext)
    );
    f.totals = saved;
    f.totals[0].packs = packs(7);
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::InvalidNativeContext)
    );
    f.totals[0].packs = packs(0);
    let wrong_hub = ReturnContext {
        hub: HubId::from_bytes([2; 16]).unwrap(),
        ..f.context()
    };
    assert_eq!(
        f.command.plan(&wrong_hub, f.ids, owner(), at(150)),
        Err(ReturnError::InvalidNativeContext)
    );
}
#[test]
fn missing_claimed_group_stays_unresolved_without_choosing_a_plausible_match() {
    let mut f = Fixture::new(&[6, 4]);
    f.command.departure.as_mut().unwrap().group = Some(id(90));
    let plan = f.plan();
    assert_eq!(
        plan.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::ClaimedGroupMissing)
    );
    assert_eq!(plan.linkage.plausible_sources.len(), 2);
    assert!(plan.linkage.claimed_source.is_none());
    assert_eq!(plan.linkage.claim, f.command.departure);
}
#[test]
fn contradictory_actual_times_keep_returned_custody_but_not_an_established_link() {
    let mut f = Fixture::new(&[6]);
    f.departure.occurred = Occurrence::Known(at(80));
    for group in &mut f.departure.consumed {
        group.causal_floor = Some(at(80));
    }
    f.departure.validate().unwrap();
    f.command.occurred = Occurrence::Known(at(70));
    let plan = f.plan();
    assert_eq!(plan.linkage.timing, ReturnTiming::Contradictory);
    assert_eq!(
        plan.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::TimeConflict)
    );
    assert_eq!(plan.intake.occurred, Occurrence::Known(at(70)));
    assert_eq!(plan.intake.packs, packs(2));
    f.command.occurred = Occurrence::Interval {
        earliest: at(70),
        latest: at(90),
    };
    assert_eq!(f.plan().linkage.timing, ReturnTiming::Uncertain);
}
#[test]
fn identities_repeat_stably_and_are_distinct_from_receipt_and_opening_namespaces() {
    let f = Fixture::new(&[6]);
    assert_eq!(f.plan(), f.plan());
    assert_ne!(
        f.ids.population,
        ReceiptIdentities::from_operation_bytes(f.ids.operation)
            .unwrap()
            .trace
    );
    assert_ne!(
        f.ids.population,
        OpeningIdentities::from_operation_bytes(f.ids.operation)
            .unwrap()
            .population
    );
    let mut bad = f.ids;
    bad.group = bad.population;
    assert_eq!(
        f.command.plan(&f.context(), bad, owner(), at(150)),
        Err(ReturnError::InvalidIdentity)
    );
}
#[test]
fn stale_empty_future_and_changed_semantics_cannot_be_mistaken_for_the_original() {
    let mut f = Fixture::new(&[6]);
    let initial = f.command.fingerprint(f.departure.hub, owner(), f.ids);
    f.command.departure.as_mut().unwrap().group = None;
    assert_ne!(
        f.command.fingerprint(f.departure.hub, owner(), f.ids),
        initial
    );
    f.command.packs = packs(0);
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::EmptyQuantity)
    );
    f.command.packs = packs(2);
    f.command.occurred = Occurrence::Known(at(151));
    assert!(matches!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(ReturnError::Stock(StockError::InvalidOccurrence))
    ));
    f.command.occurred = Occurrence::Unknown;
    let context = ReturnContext {
        stock_revision: HubRevision::from_value(f.command.expected_stock_revision.value() + 1)
            .unwrap(),
        ..f.context()
    };
    assert_eq!(
        f.command.plan(&context, f.ids, owner(), at(150)),
        Err(ReturnError::StaleRevision)
    );
}

#[test]
fn variant_conflict_preserves_known_departure_and_observed_new_pack_without_attribution() {
    let mut f = Fixture::new(&[6]);
    f.catalog
        .create(
            f.catalog.revision(),
            id(9),
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Different original pack").unwrap(),
                nominal_grams: None,
            },
        )
        .unwrap();
    f.command.expected_catalog_revision = f.catalog.revision();
    f.command.variant = id(9);
    let plan = f.plan();
    assert_eq!(
        plan.linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::VariantConflict)
    );
    assert_eq!(
        plan.linkage.claimed_source.unwrap().original.variant.id,
        id(2)
    );
    assert!(plan.linkage.plausible_sources.is_empty());
    assert!(plan.linkage.departure.is_some());
    assert_eq!(plan.intake.variant.id, id(9));
    assert_eq!(plan.intake.packs, packs(2));
    f.command.departure.as_mut().unwrap().group = None;
    assert_eq!(
        f.plan().linkage.resolution,
        ReturnLinkResolution::Unresolved(UnresolvedReturnLink::NoMatchingVariant)
    );
}

#[test]
fn overlength_native_totals_are_rejected_before_collecting_unbounded_context() {
    let f = Fixture::new(&[6]);
    let totals: Vec<_> = (1..=crate::hub::dispatch::HUB_DISPATCH_LINES + 1)
        .map(|n| {
            let mut bytes = [0; 16];
            bytes[8..].copy_from_slice(&u64::try_from(n).unwrap().to_be_bytes());
            EstablishedReturnTotal {
                original_group: HubRecordId::from_bytes(bytes).unwrap(),
                packs: packs(0),
            }
        })
        .collect();
    let context = ReturnContext {
        established_return_totals: &totals,
        ..f.context()
    };
    assert_eq!(
        f.command.plan(&context, f.ids, owner(), at(150)),
        Err(ReturnError::InvalidNativeContext)
    );
}
