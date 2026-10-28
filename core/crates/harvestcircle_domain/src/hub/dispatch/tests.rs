use super::*;
use crate::hub::reference::{HubLabel, LocationPurpose, SourceAttribution};
use crate::hub::stock::{Occurrence, ReceiptCommand, ReceiptIdentities};

fn id(value: u8) -> HubRecordId {
    HubRecordId::from_bytes([value; 16]).unwrap()
}
fn rev(value: u64) -> HubRevision {
    HubRevision::from_value(value).unwrap()
}
fn packs(value: u32) -> WholePacks {
    WholePacks::from_value(value).unwrap()
}
fn note(value: &str) -> SourceProvenance {
    SourceProvenance::new(value).unwrap()
}
fn owner() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn now() -> UnixTimestamp {
    UnixTimestamp::from_seconds(100).unwrap()
}
fn ids(op: u8) -> DispatchIdentities {
    DispatchIdentities::from_operation_bytes(id(op)).unwrap()
}

struct Fixture {
    catalog: ReferenceCatalog,
    sources: Vec<DispatchSource>,
    stock: HubRevision,
}
impl Fixture {
    fn new() -> Self {
        let label = |value| HubLabel::new(value).unwrap();
        let records = vec![
            ReferenceRecord {
                id: id(1),
                revision: rev(1),
                value: ReferenceValue::Product {
                    label: label("Fictional apples"),
                },
            },
            ReferenceRecord {
                id: id(2),
                revision: rev(1),
                value: ReferenceValue::Pack {
                    label: label("Original crate"),
                    product: id(1),
                    nominal_grams: None,
                },
            },
            ReferenceRecord {
                id: id(3),
                revision: rev(1),
                value: ReferenceValue::Location {
                    label: label("Receiving"),
                    purpose: LocationPurpose::Receiving,
                },
            },
            ReferenceRecord {
                id: id(4),
                revision: rev(1),
                value: ReferenceValue::Party {
                    label: label("Recipient A"),
                },
            },
            ReferenceRecord {
                id: id(5),
                revision: rev(1),
                value: ReferenceValue::Party {
                    label: label("Recipient B"),
                },
            },
        ];
        let catalog = ReferenceCatalog::restore(rev(6), records).unwrap();
        let receipt = ReceiptCommand {
            expected_stock_revision: rev(1),
            expected_catalog_revision: catalog.revision(),
            variant: id(2),
            supplier: None,
            producer: None,
            source: SourceAttribution::Unknown,
            packs: packs(20),
            receiving: id(3),
            planned_location: None,
            occurred: Occurrence::Unknown,
            observation: note("Counted twenty fictional original crates"),
        }
        .plan(&catalog, rev(1), id(7), id(8), owner(), now())
        .unwrap();
        let group = PhysicalStockGroup {
            id: receipt.position.group,
            trace: receipt.trace.id,
            variant: id(2),
            parent: None,
            identification: None,
            location: id(3),
            packs: packs(20),
            revision: rev(1),
            causal_floor: None,
            work: vec![],
        };
        Self {
            catalog,
            sources: vec![DispatchSource {
                group,
                trace: receipt.trace,
            }],
            stock: rev(2),
        }
    }
    fn selection(&self) -> DispatchSelection {
        DispatchSelection::new(
            self.sources
                .iter()
                .map(|source| DispatchLineInput {
                    group: source.group.id,
                    expected_group_revision: source.group.revision,
                    packs: source.group.packs,
                })
                .collect(),
        )
        .unwrap()
    }
    fn command(&self, action: DispatchAction) -> DispatchCommand {
        DispatchCommand {
            expected_stock_revision: self.stock,
            expected_catalog_revision: self.catalog.revision(),
            action,
            observation: note("Reviewed fictional preparation"),
        }
    }
    fn context<'a>(&'a self, current: Option<&'a DispatchRecord>) -> DispatchContext<'a> {
        DispatchContext {
            hub: HubId::from_bytes([1; 16]).unwrap(),
            stock_revision: self.stock,
            catalog: &self.catalog,
            sources: &self.sources,
            current,
        }
    }
    fn prepare(&mut self, op: u8) -> DispatchRecord {
        let planned = self
            .command(DispatchAction::Prepare {
                recipient: id(4),
                lines: self.selection(),
            })
            .plan(&self.context(None), ids(op), owner(), now())
            .unwrap();
        self.stock = planned.stock_revision;
        planned.record
    }
    fn issue(&mut self, record: &DispatchRecord, op: u8) -> DispatchRecord {
        let planned = self
            .command(DispatchAction::Issue {
                preparation: record.id,
                expected_revision: record.revision,
            })
            .plan(&self.context(Some(record)), ids(op), owner(), now())
            .unwrap();
        self.stock = planned.stock_revision;
        planned.record
    }
}

#[test]
fn preparation_and_issue_preserve_physical_inputs_and_original_reviewed_material() {
    let mut f = Fixture::new();
    let original = f.sources.clone();
    let draft = f.prepare(20);
    let saved = draft.clone();
    let issued = f.issue(&draft, 21);
    assert_eq!(f.sources, original);
    assert_eq!(draft, saved);
    assert!(draft.versions.is_empty());
    assert!(issued.draft.is_none());
    let paper = &issued.versions[0];
    assert_eq!(paper.material.total_packs().unwrap(), 20);
    assert_eq!(paper.material.recipient.id, id(4));
    assert_eq!(paper.number, 1);
    assert_eq!(
        paper.material.lines[0].trace.source,
        SourceAttribution::Unknown
    );
    assert!(paper.material.lines[0].trace.source_needs_review());
    assert_eq!(paper.material.lines[0].group, original[0].group.id);
    assert_eq!(paper.material.lines[0].location.id, id(3));
    assert_eq!(paper.material.lines[0].trace, original[0].trace);
    assert_eq!(paper.material, draft.draft.unwrap());
    assert_eq!(paper.status, IdentificationStatus::Issued);
    assert_eq!(paper.previous, None);
}

#[test]
fn two_nonreserving_drafts_can_select_and_issue_the_same_stock_without_reserving_it() {
    let mut f = Fixture::new();
    let original = f.sources.clone();
    let a = f.prepare(20);
    let b = f.prepare(21);
    let issued_a = f.issue(&a, 22);
    let issued_b = f.issue(&b, 23);
    assert_eq!(issued_a.versions[0].material.total_packs().unwrap(), 20);
    assert_eq!(issued_b.versions[0].material.total_packs().unwrap(), 20);
    assert_ne!(issued_a.versions[0].id, issued_b.versions[0].id);
    assert_eq!(f.sources, original);
}

#[test]
fn current_catalog_change_requires_review_and_cannot_rewrite_an_issued_snapshot() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let records = f
        .catalog
        .records()
        .cloned()
        .map(|mut record| {
            if record.id == id(4) {
                record.revision = rev(2);
                record.value = ReferenceValue::Party {
                    label: HubLabel::new("Renamed recipient A").unwrap(),
                };
            }
            record
        })
        .collect();
    f.catalog = ReferenceCatalog::restore(rev(7), records).unwrap();
    let issue = f.command(DispatchAction::Issue {
        preparation: draft.id,
        expected_revision: draft.revision,
    });
    assert_eq!(
        issue.plan(&f.context(Some(&draft)), ids(21), owner(), now()),
        Err(DispatchError::PreviewChanged)
    );
    let changed = f
        .command(DispatchAction::Revise {
            preparation: draft.id,
            expected_revision: draft.revision,
            recipient: id(4),
            lines: f.selection(),
        })
        .plan(&f.context(Some(&draft)), ids(22), owner(), now())
        .unwrap();
    f.stock = changed.stock_revision;
    let issued = f.issue(&changed.record, 23);
    let original = issued.versions[0].clone();
    assert_eq!(
        original.material.recipient.value.label().as_str(),
        "Renamed recipient A"
    );
    assert_eq!(
        draft.draft.unwrap().recipient.value.label().as_str(),
        "Recipient A"
    );
    let edit = f.command(DispatchAction::Revise {
        preparation: issued.id,
        expected_revision: issued.revision,
        recipient: id(5),
        lines: f.selection(),
    });
    assert_eq!(
        edit.plan(&f.context(Some(&issued)), ids(24), owner(), now()),
        Err(DispatchError::InvalidState)
    );
    assert_eq!(issued.versions[0], original);
}

#[test]
fn replacement_is_linked_and_unused_is_explicit_without_changing_as_issued_versions() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let issued = f.issue(&draft, 21);
    let first = issued.versions[0].clone();
    let replace = f.command(DispatchAction::Replace {
        preparation: issued.id,
        expected_revision: issued.revision,
        recipient: id(5),
        lines: f.selection(),
    });
    let planned = replace
        .plan(&f.context(Some(&issued)), ids(22), owner(), now())
        .unwrap();
    f.stock = planned.stock_revision;
    let replaced = planned.record;
    let newer = &replaced.versions[1];
    assert_eq!(replaced.versions[0].material, first.material);
    assert_eq!(replaced.versions[0].issued, first.issued);
    assert_eq!(
        replaced.versions[0].status,
        IdentificationStatus::Superseded
    );
    assert_eq!(newer.previous, Some(first.id));
    assert_eq!(newer.number, 2);
    assert_eq!(newer.material.recipient.id, id(5));
    assert_eq!(replaced.versions[0].closed.as_ref(), Some(&newer.issued));
    let unused = f.command(DispatchAction::MarkUnused {
        preparation: replaced.id,
        expected_revision: replaced.revision,
    });
    let mut context = f.context(Some(&replaced));
    context.sources = &[];
    let closed = unused.plan(&context, ids(23), owner(), now()).unwrap();
    f.stock = closed.stock_revision;
    assert_eq!(
        closed.record.versions[1].status,
        IdentificationStatus::Unused
    );
    assert_eq!(closed.record.versions[1].material, newer.material);
    let forbidden = f.command(DispatchAction::Replace {
        preparation: replaced.id,
        expected_revision: closed.record.revision,
        recipient: id(4),
        lines: f.selection(),
    });
    assert_eq!(
        forbidden.plan(&f.context(Some(&closed.record)), ids(24), owner(), now()),
        Err(DispatchError::InvalidState)
    );
}

#[test]
fn preparation_requires_whole_identified_groups_and_closed_bounded_selections() {
    let f = Fixture::new();
    let line = f.selection().lines()[0];
    for lines in [
        vec![],
        vec![line, line],
        vec![DispatchLineInput {
            packs: packs(0),
            ..line
        }],
    ] {
        assert!(DispatchSelection::new(lines).is_err());
    }
    assert_eq!(
        DispatchSelection::new(
            (1..=65)
                .map(|v| DispatchLineInput {
                    group: id(v),
                    ..line
                })
                .collect()
        ),
        Err(DispatchError::Capacity)
    );
    let partial = DispatchSelection::new(vec![DispatchLineInput {
        packs: packs(6),
        ..line
    }])
    .unwrap();
    let command = f.command(DispatchAction::Prepare {
        recipient: id(4),
        lines: partial,
    });
    assert_eq!(
        command.plan(&f.context(None), ids(20), owner(), now()),
        Err(DispatchError::PhysicalSeparationRequired)
    );
    let wrong = f.command(DispatchAction::Prepare {
        recipient: id(1),
        lines: f.selection(),
    });
    assert_eq!(
        wrong.plan(&f.context(None), ids(20), owner(), now()),
        Err(DispatchError::InvalidSelection)
    );
}

#[test]
fn stale_versions_foreign_scope_and_changed_native_identity_have_no_success_plan() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let mut command = f.command(DispatchAction::Issue {
        preparation: draft.id,
        expected_revision: draft.revision,
    });
    command.expected_stock_revision = rev(1);
    assert_eq!(
        command.plan(&f.context(Some(&draft)), ids(21), owner(), now()),
        Err(DispatchError::StaleRevision)
    );
    command.expected_stock_revision = f.stock;
    let mut context = f.context(Some(&draft));
    context.hub = HubId::from_bytes([2; 16]).unwrap();
    assert_eq!(
        command.plan(&context, ids(21), owner(), now()),
        Err(DispatchError::StaleRevision)
    );
    let mut wrong = ids(21);
    wrong.identification = id(90);
    assert_eq!(
        command.plan(&f.context(Some(&draft)), wrong, owner(), now()),
        Err(DispatchError::InvalidSelection)
    );
    f.stock = rev(HubRevision::MAXIMUM);
    command.expected_stock_revision = f.stock;
    assert_eq!(
        command.plan(&f.context(Some(&draft)), ids(21), owner(), now()),
        Err(DispatchError::RevisionExhausted)
    );
}

#[test]
fn multi_source_counts_are_bounded_without_treating_all_lines_as_one_variant_quantity() {
    let mut f = Fixture::new();
    f.sources[0].group.packs = packs(1_000_000);
    f.sources[0].trace.original_packs = packs(1_000_000);
    let mut second = f.sources[0].clone();
    second.group.id = id(9);
    second.group.trace = id(10);
    second.trace.id = id(10);
    f.sources.push(second);
    let draft = f.prepare(20);
    let material = draft.draft.unwrap();
    assert_eq!(material.lines.len(), 2);
    assert_eq!(material.total_packs().unwrap(), 2_000_000);
    assert_ne!(material.lines[0].trace.id, material.lines[1].trace.id);
}

#[test]
fn original_record_validation_denies_broken_version_chains_and_terminal_shapes() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let issued = f.issue(&draft, 21);
    let mut broken = issued.clone();
    broken.versions[0].previous = Some(id(99));
    assert!(broken.validate().is_err());
    let mut broken = issued.clone();
    broken.versions[0].status = IdentificationStatus::Superseded;
    assert!(broken.validate().is_err());
    let mut broken = issued.clone();
    broken.versions[0].id = id(99);
    assert!(broken.validate().is_err());
    let mut broken = issued.clone();
    broken.revision = rev(8);
    assert!(broken.validate().is_err());
    let mut broken = issued.clone();
    let repeated_line = broken.versions[0].material.lines[0].clone();
    broken.versions[0].material.lines.push(repeated_line);
    assert!(broken.validate().is_err());
    let mut broken = draft;
    broken.created.operation = id(99);
    assert!(broken.validate().is_err());
    assert!(issued.validate().is_ok());
}

#[test]
fn intent_is_canonical_and_binds_material_preconditions_actor_and_native_ids() {
    let f = Fixture::new();
    let command = f.command(DispatchAction::Prepare {
        recipient: id(4),
        lines: f.selection(),
    });
    let hub = f.context(None).hub;
    let hash = command.fingerprint(hub, owner(), ids(20));
    assert_eq!(
        ids(20).preparation.to_hex(),
        "c425a34b90f4b50205037e1d43ccdbfc"
    );
    assert_eq!(
        ids(20).identification.to_hex(),
        "33523823c54d5700a43afb6454a4e6fe"
    );
    let encoded: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        encoded,
        "efc46bd4eb2aa73bf59c3f03a014c65950936a0353f8651bf71f57adefe12177"
    );
    let other =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_ne!(hash, command.fingerprint(hub, other, ids(20)));
    let mut changed = command.clone();
    changed.observation = note("Another basis");
    assert_ne!(hash, changed.fingerprint(hub, owner(), ids(20)));
    let mut changed = command.clone();
    changed.expected_catalog_revision = rev(5);
    assert_ne!(hash, changed.fingerprint(hub, owner(), ids(20)));
    let changed = f.command(DispatchAction::Prepare {
        recipient: id(5),
        lines: f.selection(),
    });
    assert_ne!(hash, changed.fingerprint(hub, owner(), ids(20)));
    assert_ne!(hash, command.fingerprint(hub, owner(), ids(21)));
    assert_ne!(
        hash,
        command.fingerprint(HubId::from_bytes([2; 16]).unwrap(), owner(), ids(20))
    );
    assert_ne!(
        ids(20).identification,
        ReceiptIdentities::from_operation_bytes(id(20))
            .unwrap()
            .trace
    );
}

#[test]
fn exact_version_limit_keeps_originals_and_still_allows_marking_the_last_issue_unused() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let mut record = f.issue(&draft, 21);
    let first = record.versions[0].clone();
    for index in 1..HUB_IDENTIFICATION_VERSIONS {
        let recipient = if index % 2 == 1 { id(5) } else { id(4) };
        let plan = f
            .command(DispatchAction::Replace {
                preparation: record.id,
                expected_revision: record.revision,
                recipient,
                lines: f.selection(),
            })
            .plan(
                &f.context(Some(&record)),
                ids(22 + index as u8),
                owner(),
                now(),
            )
            .unwrap();
        f.stock = plan.stock_revision;
        record = plan.record;
    }
    assert_eq!(record.versions.len(), HUB_IDENTIFICATION_VERSIONS);
    assert_eq!(record.versions[0].material, first.material);
    assert_eq!(record.versions[0].issued, first.issued);
    let blocked = f.command(DispatchAction::Replace {
        preparation: record.id,
        expected_revision: record.revision,
        recipient: id(4),
        lines: f.selection(),
    });
    assert_eq!(
        blocked.plan(&f.context(Some(&record)), ids(90), owner(), now()),
        Err(DispatchError::Capacity)
    );
    let unused = f.command(DispatchAction::MarkUnused {
        preparation: record.id,
        expected_revision: record.revision,
    });
    let ended = unused
        .plan(&f.context(Some(&record)), ids(91), owner(), now())
        .unwrap();
    assert_eq!(
        ended.record.versions.last().unwrap().status,
        IdentificationStatus::Unused
    );
}

#[test]
fn changed_physical_group_blocks_issue_and_noop_replacement_cannot_mint_a_new_version() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let issued = f.issue(&draft, 21);
    let same = f.command(DispatchAction::Replace {
        preparation: issued.id,
        expected_revision: issued.revision,
        recipient: id(4),
        lines: f.selection(),
    });
    assert_eq!(
        same.plan(&f.context(Some(&issued)), ids(22), owner(), now()),
        Err(DispatchError::NoMaterialChange)
    );
    let other = f.prepare(23);
    f.sources[0].group.revision = rev(2);
    f.sources[0].group.packs = packs(12);
    let issue = f.command(DispatchAction::Issue {
        preparation: other.id,
        expected_revision: other.revision,
    });
    assert_eq!(
        issue.plan(&f.context(Some(&other)), ids(24), owner(), now()),
        Err(DispatchError::StaleRevision)
    );
}

#[test]
fn original_digest_binds_resolved_labels_sources_clocks_and_closed_history() {
    let mut f = Fixture::new();
    let draft = f.prepare(20);
    let issued = f.issue(&draft, 21);
    let input = [33; 32];
    let original = dispatch_result_digest(&input, &issued).unwrap();
    let mut renamed = issued.clone();
    renamed.versions[0].material.recipient.value = ReferenceValue::Party {
        label: HubLabel::new("Changed historical recipient").unwrap(),
    };
    assert_ne!(original, dispatch_result_digest(&input, &renamed).unwrap());
    let mut source = issued.clone();
    source.versions[0].material.lines[0].trace.observation = note("Changed original observation");
    assert_ne!(original, dispatch_result_digest(&input, &source).unwrap());
    let mut clock = issued.clone();
    clock.created.recorded_at = UnixTimestamp::from_seconds(99).unwrap();
    assert_ne!(original, dispatch_result_digest(&input, &clock).unwrap());
    assert_ne!(
        original,
        dispatch_result_digest(&[34; 32], &issued).unwrap()
    );
    let mut corrupt = issued;
    corrupt.versions[0].number = 2;
    assert_eq!(
        dispatch_result_digest(&input, &corrupt),
        Err(DispatchError::InvalidHistory)
    );
}
