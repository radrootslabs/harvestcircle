use super::*;
use crate::hub::dispatch::{
    DispatchAction, DispatchCommand, DispatchContext, DispatchIdentities, DispatchLineInput,
    DispatchSelection,
};
use crate::hub::movement::{PhysicalIdentification, WorkAllocation};
use crate::hub::portions::PhysicalLineageNode;
use crate::hub::reference::{
    HubLabel, LocationPurpose, ReferenceRecord, ReferenceValue, SourceAttribution,
};
use crate::hub::stock::ReceiptCommand;
use crate::hub::treatment::{AcceptanceDecision, AcceptanceStatus, Hold, TreatmentStamp};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(text: &str) -> SourceProvenance {
    SourceProvenance::new(text).unwrap()
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn now() -> UnixTimestamp {
    UnixTimestamp::from_seconds(100).unwrap()
}
fn ids() -> DepartureIdentities {
    DepartureIdentities::from_operation_bytes(id(30)).unwrap()
}
struct Fixture {
    catalog: ReferenceCatalog,
    sources: Vec<DispatchSource>,
    treatments: Vec<DepartureTreatment>,
    paper: DispatchRecord,
}
impl Fixture {
    fn new(selected: &[u32], completed: u32) -> Self {
        let value = |n, value| ReferenceRecord {
            id: id(n),
            revision: rev(1),
            value,
        };
        let label = |v| HubLabel::new(v).unwrap();
        let catalog = ReferenceCatalog::restore(
            rev(7),
            vec![
                value(
                    1,
                    ReferenceValue::Product {
                        label: label("Fictional apples"),
                    },
                ),
                value(
                    2,
                    ReferenceValue::Pack {
                        product: id(1),
                        label: label("Original crate"),
                        nominal_grams: None,
                    },
                ),
                value(
                    3,
                    ReferenceValue::Location {
                        label: label("Receiving"),
                        purpose: LocationPurpose::Receiving,
                    },
                ),
                value(
                    4,
                    ReferenceValue::Party {
                        label: label("Recipient A"),
                    },
                ),
                value(
                    5,
                    ReferenceValue::Party {
                        label: label("Recipient B"),
                    },
                ),
                value(
                    6,
                    ReferenceValue::Location {
                        label: label("A3"),
                        purpose: LocationPurpose::Storage,
                    },
                ),
            ],
        )
        .unwrap();
        let intake = ReceiptCommand {
            expected_stock_revision: rev(1),
            expected_catalog_revision: catalog.revision(),
            variant: id(2),
            supplier: None,
            producer: None,
            source: SourceAttribution::Unknown,
            packs: packs(20),
            receiving: id(3),
            planned_location: Some(id(6)),
            occurred: Occurrence::Unknown,
            observation: note("Counted twenty original fictional crates"),
        }
        .plan(&catalog, rev(1), id(7), id(10), owner(), now())
        .unwrap();
        let requirement = WorkRequirement {
            id: id(8),
            trace: intake.trace.id,
            variant: id(2),
            target: Some(
                catalog
                    .require_kind(id(6), crate::hub::reference::ReferenceKind::Location)
                    .unwrap()
                    .clone(),
            ),
            quantity: QuantityWork::restore(
                packs(20),
                packs(completed),
                packs(0),
                packs(20 - completed),
            )
            .unwrap(),
            revision: rev(2),
        };
        let sources: Vec<_> = selected
            .iter()
            .enumerate()
            .map(|(i, n)| DispatchSource {
                group: PhysicalStockGroup {
                    id: id(11 + u8::try_from(i).unwrap()),
                    trace: intake.trace.id,
                    variant: id(2),
                    parent: Some(id(10)),
                    identification: Some(PhysicalIdentification {
                        label: label("Separately tagged crates"),
                        observation: note("Observed physical separation"),
                    }),
                    location: id(3),
                    packs: packs(*n),
                    revision: rev(1),
                    causal_floor: None,
                    work: vec![WorkAllocation {
                        requirement: requirement.clone(),
                        progress: WorkProgress::Pending,
                    }],
                },
                trace: intake.trace.clone(),
            })
            .collect();
        let mut nodes = vec![PhysicalLineageNode {
            id: id(10),
            parent: None,
            trace: intake.trace.id,
            variant: id(2),
            created_revision: rev(2),
            packs: packs(20 - completed - selected.iter().sum::<u32>()),
        }];
        if completed > 0 {
            nodes.push(PhysicalLineageNode {
                id: id(13),
                parent: Some(id(10)),
                trace: intake.trace.id,
                variant: id(2),
                created_revision: rev(3),
                packs: packs(completed),
            });
        }
        nodes.extend(
            sources
                .iter()
                .enumerate()
                .map(|(i, s)| PhysicalLineageNode {
                    id: s.group.id,
                    parent: s.group.parent,
                    trace: s.group.trace,
                    variant: s.group.variant,
                    created_revision: rev(3 + u64::try_from(i).unwrap() + u64::from(completed > 0)),
                    packs: s.group.packs,
                }),
        );
        let hub = HubId::from_bytes([1; 16]).unwrap();
        let capture =
            PhysicalLineage::new(hub, intake.trace.id, id(2), rev(4), nodes.clone()).unwrap();
        let scope = capture
            .capture(&sources.iter().map(|s| s.group.id).collect::<Vec<_>>())
            .unwrap();
        let graph = PhysicalLineage::new(hub, intake.trace.id, id(2), rev(10), nodes).unwrap();
        let stamp = TreatmentStamp {
            id: id(18),
            stock_revision: rev(5),
            actor: owner(),
            basis: catalog.handling_basis(),
            recorded_at: now(),
            occurred: Occurrence::Unknown,
            observation: note("Explicit sample acceptance"),
        };
        let state = TreatmentState::restore(
            &graph,
            vec![AcceptanceDecision {
                stamp,
                scope,
                status: AcceptanceStatus::Accepted,
            }],
            vec![],
        )
        .unwrap();
        let paper = Self::paper(&catalog, &sources, hub);
        Self {
            catalog,
            sources,
            treatments: vec![DepartureTreatment {
                lineage: graph,
                state,
            }],
            paper,
        }
    }
    fn paper(catalog: &ReferenceCatalog, sources: &[DispatchSource], hub: HubId) -> DispatchRecord {
        let selection = DispatchSelection::new(
            sources
                .iter()
                .map(|source| DispatchLineInput {
                    group: source.group.id,
                    expected_group_revision: source.group.revision,
                    packs: source.group.packs,
                })
                .collect(),
        )
        .unwrap();
        let draft = DispatchCommand {
            expected_stock_revision: rev(6),
            expected_catalog_revision: catalog.revision(),
            action: DispatchAction::Prepare {
                recipient: id(4),
                lines: selection,
            },
            observation: note("Reviewed original identified crates"),
        }
        .plan(
            &DispatchContext {
                hub,
                stock_revision: rev(6),
                catalog,
                sources,
                current: None,
            },
            DispatchIdentities::from_operation_bytes(id(20)).unwrap(),
            owner(),
            now(),
        )
        .unwrap()
        .record;
        DispatchCommand {
            expected_stock_revision: rev(7),
            expected_catalog_revision: catalog.revision(),
            action: DispatchAction::Issue {
                preparation: draft.id,
                expected_revision: draft.revision,
            },
            observation: note("Explicit sample issue"),
        }
        .plan(
            &DispatchContext {
                hub,
                stock_revision: rev(7),
                catalog,
                sources,
                current: Some(&draft),
            },
            DispatchIdentities::from_operation_bytes(id(21)).unwrap(),
            owner(),
            now(),
        )
        .unwrap()
        .record
    }
    fn context(&self) -> DepartureContext<'_> {
        DepartureContext {
            hub: self.paper.hub,
            stock_revision: rev(10),
            catalog: &self.catalog,
            paper: &self.paper,
            sources: &self.sources,
            treatments: &self.treatments,
            prior_departure: None,
        }
    }
    fn command(&self) -> DepartureCommand {
        DepartureCommand {
            expected_stock_revision: rev(10),
            expected_catalog_revision: self.catalog.revision(),
            preparation: self.paper.id,
            expected_preparation_revision: self.paper.revision,
            identification: self.paper.versions.last().unwrap().id,
            occurred: Occurrence::Unknown,
            observation: note("Observed actual departure"),
        }
    }
    fn depart(&self) -> DepartureRecord {
        self.command()
            .plan(&self.context(), ids(), owner(), now())
            .unwrap()
    }
}
#[test]
fn whole_multiline_departure_is_one_plan_with_one_shared_work_revision_and_no_fabricated_completion()
 {
    let f = Fixture::new(&[6, 4], 0);
    let original = f.sources.clone();
    let record = f.depart();
    assert_eq!(record.physical_event_count(), 2);
    assert_eq!(record.stock_revision, rev(11));
    assert_eq!(record.work_changes.len(), 1);
    let work = &record.work_changes[0];
    assert_eq!(work.retired, packs(10));
    assert_eq!(work.after.revision, rev(3));
    assert_eq!(work.after.quantity.completed(), packs(0));
    assert_eq!(work.after.quantity.no_longer_required(), packs(10));
    assert_eq!(work.after.quantity.remaining(), packs(10));
    assert_eq!(f.sources, original);
    assert_eq!(record.paper, f.paper);
    record.validate().unwrap();
}
#[test]
fn receiving_exit_retires_five_pending_packs_and_keeps_eight_completed_and_seven_remaining() {
    let f = Fixture::new(&[5], 8);
    let record = f.depart();
    let q = record.work_changes[0].after.quantity;
    assert_eq!(
        (
            q.required(),
            q.completed(),
            q.no_longer_required(),
            q.remaining()
        ),
        (packs(20), packs(8), packs(5), packs(7))
    );
}
#[test]
fn completed_scope_exits_without_retiring_or_rewriting_earlier_completion() {
    let mut f = Fixture::new(&[5], 8);
    f.sources[0].group.location = id(6);
    f.sources[0].group.work[0].progress = WorkProgress::Completed;
    f.paper = Fixture::paper(&f.catalog, &f.sources, f.paper.hub);
    let record = f.depart();
    assert!(record.work_changes.is_empty());
    assert_eq!(
        record.consumed[0].before.work[0]
            .requirement
            .quantity
            .completed(),
        packs(8)
    );
}
#[test]
fn every_line_requires_current_acceptance_and_independent_holds_never_authorize_partial_success() {
    let mut f = Fixture::new(&[6, 4], 0);
    let treatment = &mut f.treatments[0];
    let scope = treatment
        .lineage
        .restore_scope(
            rev(6),
            vec![crate::hub::portions::PhysicalScopeAnchor {
                group: id(12),
                created_revision: rev(4),
                captured_packs: packs(4),
            }],
        )
        .unwrap();
    let stamp = TreatmentStamp {
        id: id(19),
        stock_revision: rev(7),
        actor: owner(),
        basis: f.catalog.handling_basis(),
        recorded_at: now(),
        occurred: Occurrence::Unknown,
        observation: note("Independent current hold"),
    };
    treatment.state = TreatmentState::restore(
        &treatment.lineage,
        treatment.state.decisions().to_vec(),
        vec![Hold {
            stamp,
            scope,
            release: None,
        }],
    )
    .unwrap();
    let original = f.sources.clone();
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::Treatment(TreatmentError::ActionBlocked))
    );
    assert_eq!(f.sources, original);
    let treatment = &mut f.treatments[0];
    treatment.state = TreatmentState::restore(&treatment.lineage, vec![], vec![]).unwrap();
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::Treatment(TreatmentError::ActionBlocked))
    );
}
#[test]
fn stale_or_changed_material_and_forged_ids_have_no_departure_plan() {
    let mut f = Fixture::new(&[6, 4], 0);
    let mut command = f.command();
    command.expected_stock_revision = rev(9);
    assert_eq!(
        command.plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::StaleRevision)
    );
    let mut wrong = ids();
    wrong.departure = id(31);
    assert_eq!(
        f.command().plan(&f.context(), wrong, owner(), now()),
        Err(DepartureError::InvalidIdentity)
    );
    let records = f
        .catalog
        .records()
        .cloned()
        .map(|mut record| {
            if record.id == id(4) {
                record.revision = rev(2);
                record.value = ReferenceValue::Party {
                    label: HubLabel::new("Renamed current recipient").unwrap(),
                };
            }
            record
        })
        .collect::<Vec<_>>();
    f.catalog = ReferenceCatalog::restore(rev(8), records).unwrap();
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::MaterialChanged)
    );
}
#[test]
fn changed_work_originals_and_treatment_cut_fail_as_a_whole() {
    let mut f = Fixture::new(&[6, 4], 0);
    f.sources[1].group.work[0].requirement.revision = rev(3);
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::InvalidSources)
    );
    let f = Fixture::new(&[6, 4], 0);
    let mut context = f.context();
    context.stock_revision = rev(11);
    let mut command = f.command();
    command.expected_stock_revision = rev(11);
    assert_eq!(
        command.plan(&context, ids(), owner(), now()),
        Err(DepartureError::InvalidSources)
    );
}
#[test]
fn original_plan_checks_all_recorded_effects_and_usage_blocks_new_commands() {
    let f = Fixture::new(&[5], 8);
    let record = f.depart();
    let mut context = f.context();
    context.prior_departure = Some(&record);
    assert_eq!(
        f.command().plan(&context, ids(), owner(), now()),
        Err(DepartureError::AlreadyUsed)
    );
    let mut corrupt = record.clone();
    corrupt.consumed[0].before.packs = packs(4);
    assert!(corrupt.validate().is_err());
    let mut corrupt = record.clone();
    corrupt.work_changes.clear();
    assert!(corrupt.validate().is_err());
    let mut corrupt = record;
    corrupt.consumed[0].after_revision = rev(7);
    assert!(corrupt.validate().is_err());
}
#[test]
fn future_occurrence_and_selected_causal_floor_never_become_retrospective_permission() {
    let mut f = Fixture::new(&[5], 8);
    let mut command = f.command();
    command.occurred = Occurrence::Known(UnixTimestamp::from_seconds(101).unwrap());
    assert!(command.plan(&f.context(), ids(), owner(), now()).is_err());
    f.sources[0].group.causal_floor = Some(UnixTimestamp::from_seconds(90).unwrap());
    command.occurred = Occurrence::Known(UnixTimestamp::from_seconds(89).unwrap());
    assert!(command.plan(&f.context(), ids(), owner(), now()).is_err());
}

#[test]
fn unused_superseded_and_different_version_never_ship_as_the_active_issue() {
    let mut f = Fixture::new(&[5], 8);
    let unused = DispatchCommand {
        expected_stock_revision: rev(10),
        expected_catalog_revision: f.catalog.revision(),
        action: DispatchAction::MarkUnused {
            preparation: f.paper.id,
            expected_revision: f.paper.revision,
        },
        observation: note("Explicitly abandoned sample issue"),
    }
    .plan(
        &DispatchContext {
            hub: f.paper.hub,
            stock_revision: rev(10),
            catalog: &f.catalog,
            sources: &[],
            current: Some(&f.paper),
        },
        DispatchIdentities::from_operation_bytes(id(22)).unwrap(),
        owner(),
        now(),
    )
    .unwrap()
    .record;
    f.paper = unused;
    let mut context = f.context();
    context.stock_revision = rev(12);
    let mut command = f.command();
    command.expected_stock_revision = rev(12);
    assert_eq!(
        command.plan(&context, ids(), owner(), now()),
        Err(DepartureError::IdentificationUnavailable)
    );
    let mut f = Fixture::new(&[5], 8);
    let old = f.command();
    f.paper = DispatchCommand {
        expected_stock_revision: rev(10),
        expected_catalog_revision: f.catalog.revision(),
        action: DispatchAction::Replace {
            preparation: f.paper.id,
            expected_revision: f.paper.revision,
            recipient: id(5),
            lines: f
                .paper
                .versions
                .last()
                .unwrap()
                .material
                .selection()
                .unwrap(),
        },
        observation: note("Reviewed recipient replacement"),
    }
    .plan(
        &DispatchContext {
            hub: f.paper.hub,
            stock_revision: rev(10),
            catalog: &f.catalog,
            sources: &f.sources,
            current: Some(&f.paper),
        },
        DispatchIdentities::from_operation_bytes(id(22)).unwrap(),
        owner(),
        now(),
    )
    .unwrap()
    .record;
    let mut context = f.context();
    context.stock_revision = rev(12);
    let mut command = old;
    command.expected_stock_revision = rev(12);
    command.expected_preparation_revision = f.paper.revision;
    assert_eq!(
        command.plan(&context, ids(), owner(), now()),
        Err(DepartureError::IdentificationUnavailable)
    );
}
#[test]
fn missing_or_inconsistent_current_physical_treatment_never_uses_cached_eligibility() {
    let mut f = Fixture::new(&[6, 4], 0);
    f.treatments.clear();
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::InvalidSources)
    );
    let mut f = Fixture::new(&[6, 4], 0);
    let original = &f.treatments[0].lineage;
    let nodes = original
        .nodes()
        .iter()
        .map(|node| PhysicalLineageNode {
            packs: if node.id == id(12) {
                packs(3)
            } else {
                node.packs
            },
            ..*node
        })
        .collect();
    f.treatments[0].lineage = PhysicalLineage::new(
        original.hub(),
        original.trace(),
        original.variant(),
        original.revision(),
        nodes,
    )
    .unwrap();
    assert_eq!(
        f.command().plan(&f.context(), ids(), owner(), now()),
        Err(DepartureError::InvalidSources)
    );
}
#[test]
fn departure_identity_and_complete_command_intent_have_independently_derived_vectors() {
    let f = Fixture::new(&[6, 4], 0);
    let command = f.command();
    assert_eq!(ids().departure.to_hex(), "71537c06acd99ee84100a6f56ac60c39");
    let original = command.fingerprint(f.paper.hub, owner(), ids());
    assert_eq!(
        original
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>(),
        "7f7ebc28ad2ae12c00a75609f4c29936bb607c6100e0d503b3dba504d6b3800e"
    );
    for changed in [
        DepartureCommand {
            expected_stock_revision: rev(11),
            ..command.clone()
        },
        DepartureCommand {
            expected_catalog_revision: rev(8),
            ..command.clone()
        },
        DepartureCommand {
            preparation: id(32),
            ..command.clone()
        },
        DepartureCommand {
            expected_preparation_revision: rev(3),
            ..command.clone()
        },
        DepartureCommand {
            identification: id(33),
            ..command.clone()
        },
        DepartureCommand {
            occurred: Occurrence::Known(UnixTimestamp::from_seconds(90).unwrap()),
            ..command.clone()
        },
        DepartureCommand {
            observation: note("Changed actual observation"),
            ..command.clone()
        },
    ] {
        assert_ne!(original, changed.fingerprint(f.paper.hub, owner(), ids()));
    }
    assert_ne!(
        original,
        command.fingerprint(HubId::from_bytes([2; 16]).unwrap(), owner(), ids())
    );
    assert_ne!(
        original,
        command.fingerprint(
            f.paper.hub,
            owner(),
            DepartureIdentities::from_operation_bytes(id(31)).unwrap()
        )
    );
}
#[test]
fn unbounded_or_duplicate_original_line_effects_never_validate() {
    let f = Fixture::new(&[5], 8);
    let mut record = f.depart();
    record.consumed = vec![record.consumed[0].clone(); 65];
    assert!(record.validate().is_err());
}

#[test]
fn original_digest_binds_paper_material_recording_and_causal_originals() {
    let f = Fixture::new(&[5], 8);
    let record = f.depart();
    let input = [34; 32];
    let original = departure_result_digest(&input, &record).unwrap();
    let mut renamed = record.clone();
    renamed.paper.versions[0].material.recipient.value = ReferenceValue::Party {
        label: HubLabel::new("Changed historical recipient").unwrap(),
    };
    assert_ne!(original, departure_result_digest(&input, &renamed).unwrap());
    let mut clock = record.clone();
    clock.recorded_at = UnixTimestamp::from_seconds(99).unwrap();
    assert_ne!(original, departure_result_digest(&input, &clock).unwrap());
    let mut causal = record.clone();
    causal.consumed[0].before.causal_floor = Some(UnixTimestamp::from_seconds(90).unwrap());
    causal.consumed[0].causal_floor = causal.consumed[0].before.causal_floor;
    assert_ne!(original, departure_result_digest(&input, &causal).unwrap());
    assert_ne!(
        original,
        departure_result_digest(&[35; 32], &record).unwrap()
    );
    let mut corrupt = record;
    corrupt.work_changes.clear();
    assert!(departure_result_digest(&input, &corrupt).is_err());
}
