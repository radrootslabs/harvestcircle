use super::*;
use crate::hub::reference::{
    HubLabel, ReferenceValue, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
};
fn id(n: u64) -> HubRecordId {
    let mut b = [0; 16];
    b[8..].copy_from_slice(&n.to_be_bytes());
    HubRecordId::from_bytes(b).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new(
        "Independent original evidence reviewed; local manual follow-up remains required",
    )
    .unwrap()
}
fn catalog() -> ReferenceCatalog {
    let mut c = ReferenceCatalog::default();
    for (n, label) in [
        (1, "Issuer A"),
        (2, "Issuer B"),
        (3, "Recipient A"),
        (4, "Recipient B"),
    ] {
        c.create(
            c.revision(),
            id(n),
            ReferenceValue::Party {
                label: HubLabel::new(label).unwrap(),
            },
        )
        .unwrap();
    }
    c
}
fn source(n: u64, issuer: Option<u64>, certainty: SourceCertainty) -> SourceAttribution {
    SourceAttribution::Referenced {
        source: SourceReference {
            id: id(n),
            issuer: issuer.map(id),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("same printed code").unwrap(),
            provenance: note(),
        },
        certainty,
    }
}
fn original_source(c: &ReferenceCatalog) -> OriginalSourceAttribution {
    OriginalSourceAttribution {
        hub: HubId::from_bytes([1; 16]).unwrap(),
        operation: id(10),
        trace: id(11),
        stock_revision: rev(5),
        catalog_revision: c.revision(),
        recorded_at: at(100),
        actor: actor(),
        source: AmendedSource {
            source: source(12, Some(1), SourceCertainty::Established),
            issuer: Some(c.get(id(1)).unwrap().clone()),
        },
    }
}
fn cut<'a>(c: &'a ReferenceCatalog) -> AttributionCut<'a> {
    AttributionCut {
        hub: HubId::from_bytes([1; 16]).unwrap(),
        stock_revision: rev(20),
        catalog: c,
        latest_recorded_at: at(110),
        uncertain_submissions: false,
    }
}
fn source_command(c: &ReferenceCatalog) -> SourceAttributionCommand {
    SourceAttributionCommand {
        trace: id(11),
        expected_stock_revision: rev(20),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        source: source(13, Some(2), SourceCertainty::Established),
        basis: note(),
    }
}
fn downstream(c: &ReferenceCatalog) -> AttributionAffectedDeparture {
    AttributionAffectedDeparture {
        operation: id(30),
        populations: vec![id(11), id(99)],
        recipients: vec![
            AmendedRecipient::Known(c.get(id(3)).unwrap().clone()),
            AmendedRecipient::Unknown,
        ],
        issued_versions: vec![id(31), id(32)],
    }
}
fn original_recipient(c: &ReferenceCatalog) -> OriginalRecipientAttribution {
    OriginalRecipientAttribution {
        hub: HubId::from_bytes([1; 16]).unwrap(),
        family: CorrectedDepartureFamily::Normal,
        operation: id(30),
        stock_revision: rev(10),
        catalog_revision: c.revision(),
        recorded_at: at(100),
        actor: actor(),
        recipient: c.get(id(3)).unwrap().clone(),
        populations: vec![id(11), id(99)],
        issued_versions: vec![id(31), id(32)],
    }
}
fn recipient_command(c: &ReferenceCatalog) -> RecipientAttributionCommand {
    RecipientAttributionCommand {
        departure_operation: id(30),
        expected_stock_revision: rev(20),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        recipient: Some(id(4)),
        basis: note(),
    }
}
#[test]
fn wrong_issuer_after_issued_departure_preserves_original_and_complete_downstream_scope() {
    let c = catalog();
    let original = original_source(&c);
    let original_copy = original.clone();
    let event = downstream(&c);
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: std::slice::from_ref(&original.source.source),
        affected: std::slice::from_ref(&event),
    };
    let result = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    assert_eq!(original, original_copy);
    assert_eq!(result.original, original_copy);
    assert_eq!(result.before, original.source);
    assert_eq!(
        result.after.source,
        source(13, Some(2), SourceCertainty::Established)
    );
    for target in [
        CorrectionImpact::Source(id(12)),
        CorrectionImpact::Source(id(13)),
        CorrectionImpact::Population(id(11)),
        CorrectionImpact::Population(id(99)),
        CorrectionImpact::Departure(id(30)),
        CorrectionImpact::IssuedVersion(id(31)),
        CorrectionImpact::IssuedVersion(id(32)),
        CorrectionImpact::Recipient(id(3)),
        CorrectionImpact::UnresolvedRecipient(id(30)),
    ] {
        assert!(result.impact.contains(target));
    }
    assert_eq!(
        current_source_attribution(&original, std::slice::from_ref(&result)).unwrap(),
        &result.after
    );
    // The whole immutable issued event is an input; its versions are not changed.
    assert_eq!(event.issued_versions, vec![id(31), id(32)]);
}
#[test]
fn source_identity_content_is_immutable_and_matching_code_never_merges_issuers() {
    let c = catalog();
    let original = original_source(&c);
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
    };
    let bad = SourceAttributionCommand {
        source: source(12, Some(2), SourceCertainty::Established),
        ..source_command(&c)
    };
    assert_eq!(
        bad.plan(&context, id(40), actor(), at(120)),
        Err(AttributionError::SourceIdentityConflict)
    );
    assert!(
        source_command(&c)
            .plan(&context, id(40), actor(), at(120))
            .is_ok()
    );
    let no_op = SourceAttributionCommand {
        source: original.source.source.clone(),
        ..source_command(&c)
    };
    assert_eq!(
        no_op.plan(&context, id(40), actor(), at(120)),
        Err(AttributionError::NoChange)
    );
}
#[test]
fn unknown_possible_and_missing_issuer_remain_explicit_and_in_impact_history() {
    let c = catalog();
    let original = original_source(&c);
    let mut context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
    };
    let first = SourceAttributionCommand {
        source: SourceAttribution::Unknown,
        ..source_command(&c)
    }
    .plan(&context, id(40), actor(), at(120))
    .unwrap();
    assert_eq!(
        first.after,
        AmendedSource {
            source: SourceAttribution::Unknown,
            issuer: None
        }
    );
    assert!(
        first
            .impact
            .contains(CorrectionImpact::UnresolvedSource(original.trace))
    );
    let history = [first];
    context.history = &history;
    context.cut.stock_revision = rev(21);
    let command = SourceAttributionCommand {
        expected_stock_revision: rev(21),
        expected_prior: Some(id(40)),
        source: source(14, None, SourceCertainty::ReportedPossible),
        ..source_command(&c)
    };
    let second = command.plan(&context, id(41), actor(), at(121)).unwrap();
    assert_eq!(second.after.issuer, None);
    assert!(second.impact.contains(CorrectionImpact::Source(id(12))));
    assert!(
        second
            .impact
            .contains(CorrectionImpact::UnresolvedSource(original.trace))
    );
}
#[test]
fn competing_source_amendment_and_uncertain_or_wrong_scope_cannot_make_a_plan() {
    let c = catalog();
    let original = original_source(&c);
    let mut context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
    };
    let first = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    let history = [first];
    context.history = &history;
    context.cut.stock_revision = rev(21);
    assert_eq!(
        source_command(&c).plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::StaleRevision)
    );
    let command = SourceAttributionCommand {
        expected_stock_revision: rev(21),
        ..source_command(&c)
    };
    assert_eq!(
        command.plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::StaleRevision)
    );
    context.cut.uncertain_submissions = true;
    assert_eq!(
        command.plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::UncertainSubmission)
    );
    context.cut.uncertain_submissions = false;
    context.history = &[];
    context.cut.stock_revision = rev(20);
    assert_eq!(
        SourceAttributionCommand {
            trace: id(999),
            ..source_command(&c)
        }
        .plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::InvalidScope)
    );
    assert_eq!(
        source_command(&c).plan(&context, id(41), actor(), at(109)),
        Err(AttributionError::InvalidHistory)
    );
}
#[test]
fn damaged_source_chain_does_not_replace_the_original_or_drop_prior_associations() {
    let c = catalog();
    let original = original_source(&c);
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
    };
    let record = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    for bad in [
        SourceAttributionCorrection {
            before: record.after.clone(),
            ..record.clone()
        },
        SourceAttributionCorrection {
            prior: Some(id(999)),
            ..record.clone()
        },
        SourceAttributionCorrection {
            impact: CorrectionImpactSet::new(vec![CorrectionImpact::Population(id(11))]).unwrap(),
            ..record.clone()
        },
    ] {
        assert!(current_source_attribution(&original, &[bad]).is_err());
    }
    let mut original_changed = record.original.clone();
    original_changed.actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_eq!(
        current_source_attribution(&original_changed, &[record]),
        Err(AttributionError::InvalidHistory)
    );
}
#[test]
fn recipient_amendment_changes_whole_event_and_preserves_every_as_issued_version() {
    let c = catalog();
    let original = original_recipient(&c);
    let copy = original.clone();
    let context = RecipientAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
    };
    let record = recipient_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    assert_eq!(record.original, copy);
    assert_eq!(original, copy);
    assert_eq!(record.before, AmendedRecipient::Known(copy.recipient));
    assert_eq!(
        record.after,
        AmendedRecipient::Known(c.get(id(4)).unwrap().clone())
    );
    for target in [
        CorrectionImpact::Population(id(11)),
        CorrectionImpact::Population(id(99)),
        CorrectionImpact::Recipient(id(3)),
        CorrectionImpact::Recipient(id(4)),
        CorrectionImpact::IssuedVersion(id(31)),
        CorrectionImpact::IssuedVersion(id(32)),
        CorrectionImpact::Departure(id(30)),
    ] {
        assert!(record.impact.contains(target));
    }
    assert_eq!(
        current_recipient_attribution(&original, std::slice::from_ref(&record)).unwrap(),
        record.after
    );
    let mut incomplete = record;
    incomplete.original.populations.pop();
    assert!(current_recipient_attribution(&original, &[incomplete]).is_err());
}
#[test]
fn unknown_recipient_does_not_reuse_old_party_and_keeps_all_prior_follow_up() {
    let c = catalog();
    let original = original_recipient(&c);
    let mut context = RecipientAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
    };
    let first = RecipientAttributionCommand {
        recipient: None,
        ..recipient_command(&c)
    }
    .plan(&context, id(40), actor(), at(120))
    .unwrap();
    assert_eq!(first.after, AmendedRecipient::Unknown);
    assert!(
        first
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(id(30)))
    );
    let history = [first];
    context.history = &history;
    context.cut.stock_revision = rev(21);
    let second = RecipientAttributionCommand {
        expected_stock_revision: rev(21),
        expected_prior: Some(id(40)),
        ..recipient_command(&c)
    }
    .plan(&context, id(41), actor(), at(121))
    .unwrap();
    assert!(second.impact.contains(CorrectionImpact::Recipient(id(3))));
    assert!(second.impact.contains(CorrectionImpact::Recipient(id(4))));
    assert!(
        second
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(id(30)))
    );
}
#[test]
fn malformed_recipient_history_missing_documents_or_wrong_current_reference_fails() {
    let c = catalog();
    let original = original_recipient(&c);
    let context = RecipientAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
    };
    let record = recipient_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    let bad = RecipientAttributionCorrection {
        impact: CorrectionImpactSet::new(vec![
            CorrectionImpact::Departure(id(30)),
            CorrectionImpact::Population(id(11)),
            CorrectionImpact::Population(id(99)),
        ])
        .unwrap(),
        ..record
    };
    assert_eq!(
        current_recipient_attribution(&original, &[bad]),
        Err(AttributionError::InvalidHistory)
    );
    assert_eq!(
        RecipientAttributionCommand {
            recipient: Some(id(999)),
            ..recipient_command(&c)
        }
        .plan(&context, id(40), actor(), at(120)),
        Err(AttributionError::InvalidReference)
    );
    assert_eq!(
        RecipientAttributionCommand {
            recipient: Some(id(3)),
            ..recipient_command(&c)
        }
        .plan(&context, id(40), actor(), at(120)),
        Err(AttributionError::NoChange)
    );
}
#[test]
fn impact_capacity_never_truncates_downstream_evidence() {
    let exact: Vec<_> = (1..=HUB_CORRECTION_IMPACTS)
        .map(|n| CorrectionImpact::IssuedVersion(id(u64::try_from(n).unwrap())))
        .collect();
    assert_eq!(
        CorrectionImpactSet::new(exact.clone())
            .unwrap()
            .rows()
            .len(),
        HUB_CORRECTION_IMPACTS
    );
    let mut extra = exact;
    extra.push(CorrectionImpact::Population(id(1)));
    assert_eq!(
        CorrectionImpactSet::new(extra),
        Err(AttributionError::Capacity)
    );
    assert_eq!(
        CorrectionImpactSet::new(vec![CorrectionImpact::Population(id(1)); 2]),
        Err(AttributionError::InvalidHistory)
    );
    let c = catalog();
    let original = original_source(&c);
    let affected: Vec<_> = (0..HUB_CORRECTION_ROWS)
        .map(|n| AttributionAffectedDeparture {
            operation: id(10_000 + u64::try_from(n).unwrap()),
            populations: vec![id(11)],
            recipients: vec![AmendedRecipient::Unknown],
            issued_versions: vec![],
        })
        .collect();
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &affected,
    };
    assert_eq!(
        source_command(&c).plan(&context, id(40), actor(), at(120)),
        Err(AttributionError::Capacity)
    );
}
#[test]
fn manual_work_is_explicit_per_target_and_never_carries_to_a_new_correction() {
    let c = catalog();
    let original = original_recipient(&c);
    let record = recipient_command(&c)
        .plan(
            &RecipientAttributionContext {
                cut: cut(&c),
                original: &original,
                history: &[],
            },
            id(40),
            actor(),
            at(120),
        )
        .unwrap();
    let mut current = cut(&c);
    current.stock_revision = rev(21);
    current.latest_recorded_at = at(120);
    let mut context = CorrectionFollowUpContext {
        cut: current,
        correction: &record.stamp,
        impact: &record.impact,
        history: &[],
    };
    for target in record.impact.rows() {
        assert_eq!(
            context.current(*target).unwrap(),
            CorrectionManualWork::Outstanding
        );
    }
    let target = CorrectionImpact::Recipient(id(3));
    let command = CorrectionFollowUpCommand {
        correction: id(40),
        target,
        expected_stock_revision: rev(21),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        state: CorrectionManualWork::WorkRecorded,
        basis: note(),
    };
    let observation = command.plan(&context, id(41), actor(), at(121)).unwrap();
    assert_eq!(observation.stamp.basis, note());
    let history = [observation];
    context.history = &history;
    context.cut.stock_revision = rev(22);
    assert_eq!(
        context.current(target).unwrap(),
        CorrectionManualWork::WorkRecorded
    );
    assert_eq!(
        context
            .current(CorrectionImpact::IssuedVersion(id(31)))
            .unwrap(),
        CorrectionManualWork::Outstanding
    );
    assert_eq!(
        command.plan(&context, id(42), actor(), at(122)),
        Err(AttributionError::StaleRevision)
    );
    let reopened = CorrectionFollowUpCommand {
        expected_stock_revision: rev(22),
        expected_prior: Some(id(41)),
        state: CorrectionManualWork::Outstanding,
        ..command
    }
    .plan(&context, id(42), actor(), at(122))
    .unwrap();
    assert_eq!(reopened.prior, Some(id(41)));
    assert_eq!(history[0].state, CorrectionManualWork::WorkRecorded);
    let new_subject = AttributionStamp {
        operation: id(50),
        stock_revision: rev(22),
        ..record.stamp.clone()
    };
    let new = CorrectionFollowUpContext {
        cut: context.cut,
        correction: &new_subject,
        impact: &record.impact,
        history: &history,
    };
    assert_eq!(new.current(target), Err(AttributionError::InvalidHistory));
}
#[test]
fn manual_work_rejects_foreign_target_reused_operation_and_bad_original_clock() {
    let c = catalog();
    let original = original_recipient(&c);
    let record = recipient_command(&c)
        .plan(
            &RecipientAttributionContext {
                cut: cut(&c),
                original: &original,
                history: &[],
            },
            id(40),
            actor(),
            at(120),
        )
        .unwrap();
    let mut current = cut(&c);
    current.stock_revision = rev(21);
    current.latest_recorded_at = at(120);
    let context = CorrectionFollowUpContext {
        cut: current,
        correction: &record.stamp,
        impact: &record.impact,
        history: &[],
    };
    let command = CorrectionFollowUpCommand {
        correction: id(40),
        target: CorrectionImpact::Recipient(id(3)),
        expected_stock_revision: rev(21),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        state: CorrectionManualWork::WorkRecorded,
        basis: note(),
    };
    assert_eq!(
        command.plan(&context, id(40), actor(), at(121)),
        Err(AttributionError::InvalidHistory)
    );
    assert_eq!(
        command.plan(&context, id(41), actor(), at(119)),
        Err(AttributionError::InvalidHistory)
    );
    assert_eq!(
        CorrectionFollowUpCommand {
            target: CorrectionImpact::Recipient(id(999)),
            ..command
        }
        .plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::InvalidScope)
    );
}

#[test]
fn later_source_correction_cannot_drop_old_unknown_recipient_or_issued_versions() {
    let c = catalog();
    let original = original_source(&c);
    let event = downstream(&c);
    let first = source_command(&c)
        .plan(
            &SourceAttributionContext {
                cut: cut(&c),
                original: &original,
                history: &[],
                known_sources: &[],
                affected: std::slice::from_ref(&event),
            },
            id(40),
            actor(),
            at(120),
        )
        .unwrap();
    let history = [first];
    let mut old_cut = cut(&c);
    old_cut.stock_revision = rev(21);
    old_cut.latest_recorded_at = at(120);
    let mut old_work = CorrectionFollowUpContext {
        cut: old_cut,
        correction: &history[0].stamp,
        impact: &history[0].impact,
        history: &[],
    };
    let observation = CorrectionFollowUpCommand {
        correction: id(40),
        target: CorrectionImpact::IssuedVersion(id(31)),
        expected_stock_revision: rev(21),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        state: CorrectionManualWork::WorkRecorded,
        basis: note(),
    }
    .plan(&old_work, id(45), actor(), at(121))
    .unwrap();
    let old_history = [observation];
    old_work.history = &old_history;
    old_work.cut.stock_revision = rev(22);
    assert_eq!(
        old_work
            .current(CorrectionImpact::IssuedVersion(id(31)))
            .unwrap(),
        CorrectionManualWork::WorkRecorded
    );

    let mut current = cut(&c);
    current.stock_revision = rev(22);
    current.latest_recorded_at = at(121);
    let command = SourceAttributionCommand {
        expected_stock_revision: rev(22),
        expected_prior: Some(id(40)),
        source: source(14, Some(1), SourceCertainty::Established),
        ..source_command(&c)
    };
    let mut context = SourceAttributionContext {
        cut: current,
        original: &original,
        history: &history,
        known_sources: &[],
        affected: &[],
    };
    assert_eq!(
        command.plan(&context, id(41), actor(), at(122)),
        Err(AttributionError::InvalidScope)
    );
    let mut missing = event.clone();
    missing.issued_versions.pop();
    let incomplete = [missing];
    context.affected = &incomplete;
    assert_eq!(
        command.plan(&context, id(41), actor(), at(122)),
        Err(AttributionError::InvalidScope)
    );
    let mut missing = event.clone();
    missing.recipients.pop();
    let incomplete = [missing];
    context.affected = &incomplete;
    assert_eq!(
        command.plan(&context, id(41), actor(), at(122)),
        Err(AttributionError::InvalidScope)
    );
    context.affected = std::slice::from_ref(&event);
    let next = command.plan(&context, id(41), actor(), at(122)).unwrap();
    assert!(
        history[0]
            .impact
            .rows()
            .iter()
            .all(|target| next.impact.contains(*target))
    );
    let mut damaged = next.clone();
    damaged.impact = CorrectionImpactSet::new(
        next.impact
            .rows()
            .iter()
            .copied()
            .filter(|t| !matches!(t, CorrectionImpact::UnresolvedRecipient(_)))
            .collect(),
    )
    .unwrap();
    assert_eq!(
        current_source_attribution(&original, &[history[0].clone(), damaged]),
        Err(AttributionError::InvalidHistory)
    );
    let mut follow_cut = cut(&c);
    follow_cut.stock_revision = rev(23);
    let fresh = CorrectionFollowUpContext {
        cut: follow_cut,
        correction: &next.stamp,
        impact: &next.impact,
        history: &[],
    };
    assert_eq!(
        fresh
            .current(CorrectionImpact::IssuedVersion(id(31)))
            .unwrap(),
        CorrectionManualWork::Outstanding
    );
}
#[test]
fn source_reconstruction_checks_identity_reuse_after_an_intervening_identity() {
    let c = catalog();
    let original = original_source(&c);
    let mut context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
    };
    let first = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    let h1 = [first];
    context.history = &h1;
    context.cut.stock_revision = rev(21);
    let second = SourceAttributionCommand {
        expected_stock_revision: rev(21),
        expected_prior: Some(id(40)),
        source: source(14, Some(1), SourceCertainty::Established),
        ..source_command(&c)
    }
    .plan(&context, id(41), actor(), at(121))
    .unwrap();
    let h2 = [h1[0].clone(), second];
    context.history = &h2;
    context.cut.stock_revision = rev(22);
    let command = SourceAttributionCommand {
        expected_stock_revision: rev(22),
        expected_prior: Some(id(41)),
        source: source(13, Some(1), SourceCertainty::Established),
        ..source_command(&c)
    };
    assert_eq!(
        command.plan(&context, id(42), actor(), at(122)),
        Err(AttributionError::SourceIdentityConflict)
    );
    let good = SourceAttributionCommand {
        source: source(15, Some(1), SourceCertainty::Established),
        ..command
    }
    .plan(&context, id(42), actor(), at(122))
    .unwrap();
    let mut bad = good;
    bad.after.source = source(13, Some(1), SourceCertainty::Established);
    assert_eq!(
        current_source_attribution(&original, &[h2[0].clone(), h2[1].clone(), bad]),
        Err(AttributionError::SourceIdentityConflict)
    );
}
#[test]
fn supplied_original_event_identity_cannot_be_reused_as_a_new_source_operation() {
    let c = catalog();
    let original = original_source(&c);
    let event = downstream(&c);
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: std::slice::from_ref(&event),
    };
    assert_eq!(
        source_command(&c).plan(&context, event.operation, actor(), at(120)),
        Err(AttributionError::InvalidScope)
    );
    let mut actual = original_recipient(&c);
    actual.family = CorrectedDepartureFamily::ReviewedActual;
    assert_eq!(
        current_recipient_attribution(&actual, &[]),
        Err(AttributionError::InvalidHistory)
    );
}

#[test]
fn recipient_competing_head_and_uncertain_cut_cannot_change_the_whole_event() {
    let c = catalog();
    let original = original_recipient(&c);
    let mut context = RecipientAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
    };
    let first = recipient_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    let history = [first];
    context.history = &history;
    context.cut.stock_revision = rev(21);
    let command = RecipientAttributionCommand {
        expected_stock_revision: rev(21),
        recipient: Some(id(3)),
        ..recipient_command(&c)
    };
    assert_eq!(
        command.plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::StaleRevision)
    );
    let command = RecipientAttributionCommand {
        expected_prior: Some(id(40)),
        ..command
    };
    context.cut.uncertain_submissions = true;
    assert_eq!(
        command.plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::UncertainSubmission)
    );
    context.cut.uncertain_submissions = false;
    assert!(command.plan(&context, id(41), actor(), at(121)).is_ok());
    assert_eq!(history[0].original, original);
}
#[test]
fn damaged_manual_prior_and_source_issuer_snapshots_fail_closed() {
    let c = catalog();
    let original = original_recipient(&c);
    let correction = recipient_command(&c)
        .plan(
            &RecipientAttributionContext {
                cut: cut(&c),
                original: &original,
                history: &[],
            },
            id(40),
            actor(),
            at(120),
        )
        .unwrap();
    let mut current = cut(&c);
    current.stock_revision = rev(21);
    let mut context = CorrectionFollowUpContext {
        cut: current,
        correction: &correction.stamp,
        impact: &correction.impact,
        history: &[],
    };
    let observation = CorrectionFollowUpCommand {
        correction: id(40),
        target: CorrectionImpact::Recipient(id(3)),
        expected_stock_revision: rev(21),
        expected_catalog_revision: c.revision(),
        expected_prior: None,
        state: CorrectionManualWork::WorkRecorded,
        basis: note(),
    }
    .plan(&context, id(41), actor(), at(121))
    .unwrap();
    let bad = [CorrectionFollowUpObservation {
        prior: Some(id(999)),
        ..observation
    }];
    context.history = &bad;
    context.cut.stock_revision = rev(22);
    assert_eq!(
        context.current(CorrectionImpact::Recipient(id(3))),
        Err(AttributionError::InvalidHistory)
    );
    let mut source = original_source(&c);
    source.source.issuer = Some(c.get(id(2)).unwrap().clone());
    assert_eq!(
        current_source_attribution(&source, &[]),
        Err(AttributionError::InvalidReference)
    );
}
