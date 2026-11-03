use super::*;
use crate::hub::movement::{PhysicalIdentification, WorkProgress};
use crate::hub::portions::{PhysicalLineageNode, SplitCommand};
use crate::hub::reference::HubLabel;
use crate::hub::stock::QuantityWork;
use crate::hub::treatment::{Hold, TreatmentStamp};
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
    group: PhysicalStockGroup,
    lineage: PhysicalLineage,
    treatment: TreatmentState,
    command: DispositionCommand,
    ids: DispositionIdentities,
}
impl Fixture {
    fn new() -> Self {
        let (catalog, group, lineage, treatment) =
            crate::hub::departure::tests::disposition_policy_fixture();
        let command = DispositionCommand {
            expected_stock_revision: lineage.revision(),
            expected_catalog_revision: catalog.revision(),
            group: group.id,
            expected_group_revision: group.revision,
            packs: group.packs,
            kind: DispositionKind::Destroyed,
            occurred: Occurrence::Unknown,
            observation: note("Observed destruction of the selected six original crates"),
        };
        Self {
            catalog,
            group,
            lineage,
            treatment,
            command,
            ids: DispositionIdentities::from_operation_bytes(id(100)).unwrap(),
        }
    }
    fn context(&self) -> DispositionContext<'_> {
        DispositionContext {
            hub: self.lineage.hub(),
            stock_revision: self.lineage.revision(),
            catalog: &self.catalog,
            group: &self.group,
            lineage: &self.lineage,
            treatment: &self.treatment,
            activity: DispositionActivity::default(),
        }
    }
    fn plan(&self) -> DispositionRecord {
        self.command
            .plan(&self.context(), self.ids, owner(), at(150))
            .unwrap()
    }
}
#[test]
fn actual_disposition_retires_only_selected_pending_work_without_fabricating_completion() {
    let f = Fixture::new();
    let before = f.group.clone();
    let record = f.plan();
    assert_eq!(record.consumed.before, before);
    assert_eq!(record.kind, DispositionKind::Destroyed);
    let work = &record.work_changes[0];
    assert_eq!(work.retired, packs(6));
    assert_eq!(work.after.quantity.required(), packs(20));
    assert_eq!(work.after.quantity.completed(), packs(0));
    assert_eq!(work.after.quantity.no_longer_required(), packs(6));
    assert_eq!(work.after.quantity.remaining(), packs(14));
    assert_eq!(f.group, before);
    record.validate().unwrap();
}
#[test]
fn completed_and_previously_retired_work_remain_historical_progress() {
    for progress in [WorkProgress::Completed, WorkProgress::NoLongerRequired] {
        let mut f = Fixture::new();
        let completed = if progress == WorkProgress::Completed {
            20
        } else {
            0
        };
        f.group.work[0].progress = progress;
        f.group.work[0].requirement.quantity =
            QuantityWork::restore(packs(20), packs(completed), packs(20 - completed), packs(0))
                .unwrap();
        let record = f.plan();
        assert!(record.work_changes.is_empty());
        assert_eq!(record.consumed.before.work, f.group.work);
    }
}
#[test]
fn held_stock_can_be_disposed_without_releasing_the_hold_or_making_a_buyer_departure() {
    let mut f = Fixture::new();
    let scope = f.lineage.capture(&[f.group.id]).unwrap();
    let next = HubRevision::from_value(f.lineage.revision().value() + 1).unwrap();
    f.lineage = PhysicalLineage::new(
        f.lineage.hub(),
        f.lineage.trace(),
        f.lineage.variant(),
        next,
        f.lineage.nodes().to_vec(),
    )
    .unwrap();
    let hold = Hold {
        stamp: TreatmentStamp {
            id: id(80),
            stock_revision: next,
            actor: owner(),
            basis: f.catalog.handling_basis(),
            recorded_at: at(100),
            occurred: Occurrence::Unknown,
            observation: note("Independent hold remains active"),
        },
        scope,
        release: None,
    };
    f.treatment = TreatmentState::restore(
        &f.lineage,
        f.treatment.decisions().to_vec(),
        vec![hold.clone()],
    )
    .unwrap();
    f.command.expected_stock_revision = next;
    let projection = f
        .treatment
        .project(&f.lineage, f.catalog.handling_basis())
        .unwrap();
    let selected = TreatmentSelection::new(vec![f.group.id]).unwrap();
    assert!(
        projection
            .require(&selected, PhysicalAction::NormalDeparture)
            .is_err()
    );
    f.plan().validate().unwrap();
    assert_eq!(f.treatment.holds(), &[hold]);
}
#[test]
fn partial_disposition_requires_an_observed_separate_physical_scope() {
    let mut f = Fixture::new();
    f.command.packs = packs(2);
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(DispositionError::PhysicalSeparationRequired)
    );
    let split = SplitCommand {
        expected_stock_revision: f.lineage.revision(),
        expected_catalog_revision: f.catalog.revision(),
        group: f.group.id,
        expected_group_revision: f.group.revision,
        packs: packs(2),
        identification: PhysicalIdentification {
            label: HubLabel::new("Distinct two").unwrap(),
            observation: note("Two physically separated tagged crates"),
        },
        occurred: Occurrence::Unknown,
        observation: note("Observed actual separation before removal"),
    }
    .plan(&f.catalog, f.lineage.revision(), &f.group, id(90), at(140))
    .unwrap();
    let mut nodes = f.lineage.nodes().to_vec();
    nodes.iter_mut().find(|n| n.id == f.group.id).unwrap().packs = split.remainder.packs;
    nodes.push(PhysicalLineageNode {
        recovery: None,
        id: split.portion.id,
        parent: split.portion.parent,
        trace: split.portion.trace,
        variant: split.portion.variant,
        created_revision: split.stock_revision,
        packs: split.portion.packs,
    });
    f.lineage = PhysicalLineage::new(
        f.lineage.hub(),
        f.lineage.trace(),
        f.lineage.variant(),
        split.stock_revision,
        nodes,
    )
    .unwrap();
    f.group = split.portion;
    f.command.expected_stock_revision = split.stock_revision;
    f.command.group = f.group.id;
    f.command.expected_group_revision = f.group.revision;
    let record = f.plan();
    assert_eq!(record.consumed.before.packs, packs(2));
    assert_eq!(record.work_changes[0].retired, packs(2));
    assert_eq!(record.work_changes[0].after.quantity.remaining(), packs(18));
    assert_eq!(split.remainder.packs, packs(4));
}
#[test]
fn stale_uncertain_and_unresolved_context_never_produces_a_removal_plan() {
    let mut f = Fixture::new();
    f.command.expected_group_revision = HubRevision::from_value(2).unwrap();
    assert_eq!(
        f.command.plan(&f.context(), f.ids, owner(), at(150)),
        Err(DispositionError::StaleRevision)
    );
    f.command.expected_group_revision = f.group.revision;
    for activity in [
        DispositionActivity {
            uncertain_submissions: true,
            unresolved_actuals: false,
        },
        DispositionActivity {
            uncertain_submissions: false,
            unresolved_actuals: true,
        },
    ] {
        let context = DispositionContext {
            activity,
            ..f.context()
        };
        assert_eq!(
            f.command.plan(&context, f.ids, owner(), at(150)),
            Err(DispositionError::UnresolvedActivity)
        );
    }
    let context = DispositionContext {
        hub: HubId::from_bytes([2; 16]).unwrap(),
        ..f.context()
    };
    assert_eq!(
        f.command.plan(&context, f.ids, owner(), at(150)),
        Err(DispositionError::InvalidNativeContext)
    );
}
#[test]
fn physical_time_and_original_work_changes_are_validated_not_overwritten() {
    let mut f = Fixture::new();
    f.group.causal_floor = Some(at(100));
    f.command.occurred = Occurrence::Known(at(99));
    assert!(
        f.command
            .plan(&f.context(), f.ids, owner(), at(150))
            .is_err()
    );
    f.command.occurred = Occurrence::Unknown;
    let record = f.plan();
    assert_eq!(record.consumed.causal_floor, Some(at(100)));
    let mut corrupt = record.clone();
    corrupt.work_changes.clear();
    assert_eq!(corrupt.validate(), Err(DispositionError::InvalidHistory));
    let mut corrupt = record;
    corrupt.consumed.after_revision = corrupt.consumed.before.revision;
    assert_eq!(corrupt.validate(), Err(DispositionError::InvalidHistory));
}
#[test]
fn identities_and_kind_fingerprints_do_not_alias_another_physical_effect() {
    let mut f = Fixture::new();
    assert_eq!(f.plan(), f.plan());
    assert_ne!(
        f.ids.disposition,
        crate::hub::departure::DepartureIdentities::from_operation_bytes(f.ids.operation)
            .unwrap()
            .departure
    );
    let hash = f.command.fingerprint(f.lineage.hub(), owner(), f.ids);
    f.command.kind = DispositionKind::OtherObservedRemoval;
    assert_ne!(hash, f.command.fingerprint(f.lineage.hub(), owner(), f.ids));
    let ids = DispositionIdentities {
        disposition: f.ids.operation,
        ..f.ids
    };
    assert_eq!(
        f.command.plan(&f.context(), ids, owner(), at(150)),
        Err(DispositionError::InvalidIdentity)
    );
}

#[test]
fn disposition_seal_binds_original_input_and_distinct_removal_facts() {
    let f = Fixture::new();
    let record = f.plan();
    let input = f.command.fingerprint(record.hub, owner(), f.ids);
    let digest = disposition_result_digest(&input, &record).unwrap();
    assert_eq!(digest, disposition_result_digest(&input, &record).unwrap());
    assert!(disposition_result_digest(&[0; 32], &record).is_err());
    let mut changed = record.clone();
    changed.kind = DispositionKind::OtherObservedRemoval;
    assert!(disposition_result_digest(&input, &changed).is_err());
    changed = record.clone();
    changed.work_changes.clear();
    assert!(disposition_result_digest(&input, &changed).is_err());
    assert_eq!(f.plan(), record);
}

#[test]
fn disposition_seal_covers_reference_content_identity_and_causal_history() {
    let f = Fixture::new();
    let record = f.plan();
    let input = f.command.fingerprint(record.hub, owner(), f.ids);
    let digest = disposition_result_digest(&input, &record).unwrap();
    let mut changed = record.clone();
    changed.product.value = ReferenceValue::Product {
        label: HubLabel::new("Different recorded product label").unwrap(),
    };
    assert_ne!(digest, disposition_result_digest(&input, &changed).unwrap());
    changed = record.clone();
    changed
        .consumed
        .before
        .identification
        .as_mut()
        .unwrap()
        .observation = note("Different retained physical observation");
    assert_ne!(digest, disposition_result_digest(&input, &changed).unwrap());
    changed = record.clone();
    changed.recorded_at = at(151);
    assert_ne!(digest, disposition_result_digest(&input, &changed).unwrap());
    changed = record.clone();
    changed.consumed.before.causal_floor = Some(at(149));
    changed.consumed.causal_floor = Some(at(149));
    assert_ne!(digest, disposition_result_digest(&input, &changed).unwrap());
}
