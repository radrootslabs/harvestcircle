use super::*;
mod cycles;
mod provenance;
use crate::hub::{
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceAttribution},
    returns::{ReturnCommand, ReturnContext, ReturnDepartureClaim},
};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent fictional evidence relates this whole restricted return")
        .unwrap()
}

struct Fixture {
    catalog: ReferenceCatalog,
    original: CountExplanationPlan,
    account: CountLossQuantityAccount,
    recipient: OriginalRecipientAttribution,
    recipient_history: Vec<RecipientAttributionCorrection>,
    returned: ReturnPlan,
    associations: Vec<CountReturnAssociation>,
    cut: HubRevision,
    clock: UnixTimestamp,
}
impl Fixture {
    fn new() -> Self {
        Self::with_quantity(1)
    }
    fn with_quantity(quantity: u32) -> Self {
        let (catalog, original, account) =
            super::super::quantity::count_return_policy_fixture(quantity);
        let known = &original.known_event;
        let recipient = OriginalRecipientAttribution {
            hub: known.hub,
            family: CorrectedDepartureFamily::ExplainedCountLoss,
            operation: known.operation,
            stock_revision: known.stock_revision,
            catalog_revision: known.catalog_revision,
            recorded_at: known.recorded_at,
            actor: known.actor,
            recipient: known.recipient.clone(),
            populations: vec![known.trace],
            issued_versions: Vec::new(),
        };
        let returned = returned(
            &catalog,
            &original,
            id(230),
            original.count.stock_revision,
            1,
            Occurrence::Known(at(100)),
            at(135),
        );
        let cut = known.stock_revision;
        Self {
            catalog,
            original,
            account,
            recipient,
            recipient_history: Vec::new(),
            returned,
            associations: Vec::new(),
            cut,
            clock: at(150),
        }
    }
    fn command(&self) -> CountReturnAssociationCommand {
        let source = self.original.historical_source.historical();
        CountReturnAssociationCommand {
            return_operation: self.returned.identities.operation,
            explanation_operation: self.original.operation,
            expected_stock_revision: self.cut,
            expected_catalog_revision: self.catalog.revision(),
            expected_account_prior: self
                .account
                .explanations()
                .iter()
                .map(|r| (r.stock_revision, r.operation))
                .chain(
                    self.account
                        .amendments()
                        .iter()
                        .map(|r| (r.stock_revision, r.operation)),
                )
                .max_by_key(|(r, _)| r.value())
                .map(|(_, op)| op),
            expected_returns_prior: self.associations.last().map(|r| r.operation),
            source_operation: source.operation,
            source_group: source.group.id,
            source_evidence: note(),
            quantity_evidence: note(),
            timing_evidence: note(),
        }
    }
    fn context(&self) -> CountReturnAssociationContext<'_> {
        CountReturnAssociationContext {
            hub: self.original.known_event.hub,
            stock_revision: self.cut,
            catalog: &self.catalog,
            latest_recorded_at: self.clock,
            returned: &self.returned,
            return_hub: self.original.known_event.hub,
            return_original_digest: [23; 32],
            explanation: &self.original,
            account: &self.account,
            recipient_original: &self.recipient,
            recipient_history: &self.recipient_history,
            associations: &self.associations,
            normal_returns: &[],
            prior_association_for_return: None,
            uncertain_submissions: false,
        }
    }
    fn plan(&self) -> Result<CountReturnAssociationPlan, CountReturnAssociationError> {
        self.command()
            .plan(&self.context(), id(240), self.original.actor, at(200))
    }
}
fn returned(
    catalog: &ReferenceCatalog,
    explanation: &CountExplanationPlan,
    operation: HubRecordId,
    cut: HubRevision,
    quantity: u32,
    occurred: Occurrence,
    recorded: UnixTimestamp,
) -> ReturnPlan {
    ReturnCommand {
        expected_stock_revision: cut,
        expected_catalog_revision: catalog.revision(),
        variant: explanation.known_event.variant.id,
        location: explanation.count.before.location,
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: packs(quantity),
        identification: PhysicalIdentification {
            label: HubLabel::new("Separately identifiable restricted return").unwrap(),
            observation: note(),
        },
        planned_location: None,
        departure: Some(ReturnDepartureClaim {
            departure: count_explanation_id(explanation.operation).unwrap(),
            group: Some(explanation.count.before.id),
        }),
        occurred,
        observation: note(),
    }
    .plan(
        &ReturnContext {
            hub: explanation.known_event.hub,
            stock_revision: cut,
            catalog,
            departure: None,
            established_return_totals: &[],
            current_allowances: None,
        },
        ReturnIdentities::from_operation_bytes(operation).unwrap(),
        explanation.actor,
        recorded,
    )
    .unwrap()
}

#[test]
fn whole_return_predating_explanation_review_adds_only_an_established_classification_floor() {
    let f = Fixture::new();
    let old = (f.returned.clone(), f.original.clone(), f.account.clone());
    let plan = f.plan().unwrap();
    assert!(f.returned.intake.recorded_at < f.original.recorded_at);
    assert_eq!(plan.association.timing, ReturnTiming::AfterOrSame);
    assert_eq!(plan.association.packs, packs(1));
    assert_eq!(plan.association.return_original_digest, [23; 32]);
    assert_eq!(plan.command, f.command());
    assert_eq!(plan.returned_original, f.returned);
    assert_eq!(
        plan.returned_original.linkage.resolution,
        ReturnLinkResolution::Unrecorded
    );
    assert_eq!(plan.before.explained(), plan.after.explained());
    assert_eq!(plan.before.unexplained(), plan.after.unexplained());
    assert_eq!(
        plan.after.effective_explanations()[0].established_returns,
        packs(1)
    );
    assert_eq!(
        (f.returned.clone(), f.original.clone(), f.account.clone()),
        old
    );
    assert_eq!(plan.returned_original.group, f.returned.group);
    assert_eq!(
        plan.returned_original.acceptance,
        crate::hub::treatment::AcceptanceStatus::ReviewRequired
    );
    assert_eq!(
        CountLossQuantityAccount::restore_corrected(
            plan.after.original(),
            plan.after.explanations().to_vec(),
            vec![CountExplanationAmendment {
                operation: id(241),
                stock_revision: plan.association.stock_revision.next().unwrap(),
                explanation: f.original.operation,
                before: packs(1),
                after: packs(0)
            }],
            vec![CountExplanationReturns {
                explanation: f.original.operation,
                packs: packs(1)
            }]
        ),
        Err(CorrectionError::ReturnConflict)
    );
}

#[test]
fn uncertain_physical_time_stays_explicit_and_contradictory_time_cannot_establish_scope() {
    let mut f = Fixture::new();
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(230),
        f.original.count.stock_revision,
        1,
        Occurrence::Unknown,
        at(135),
    );
    assert_eq!(
        f.plan().unwrap().association.timing,
        ReturnTiming::Uncertain
    );
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(230),
        f.original.count.stock_revision,
        1,
        Occurrence::Known(at(60)),
        at(135),
    );
    assert_eq!(
        f.plan(),
        Err(CountReturnAssociationError::ContradictoryTiming)
    );
}

#[test]
fn scope_heads_cuts_pending_and_original_normal_association_are_independent_fences() {
    let f = Fixture::new();
    let mut command = f.command();
    command.expected_account_prior = None;
    assert_eq!(
        command.plan(&f.context(), id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::StaleRevision)
    );
    command = f.command();
    command.expected_returns_prior = Some(id(243));
    assert_eq!(
        command.plan(&f.context(), id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::StaleRevision)
    );
    command = f.command();
    command.expected_stock_revision = f.cut.next().unwrap();
    assert_eq!(
        command.plan(&f.context(), id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::StaleRevision)
    );
    command = f.command();
    command.source_group = id(244);
    assert_eq!(
        command.plan(&f.context(), id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::WrongSource)
    );
    let mut context = f.context();
    context.uncertain_submissions = true;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::UncertainSubmission)
    );
    context = f.context();
    context.prior_association_for_return = Some(id(245));
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::AlreadyAssociated)
    );
    let mut normal = f.returned.clone();
    normal.linkage.resolution = ReturnLinkResolution::EstablishedScope;
    context = f.context();
    context.returned = &normal;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::AlreadyAssociated)
    );
}

#[test]
fn whole_return_cannot_exceed_effective_explanation_or_reuse_an_earlier_return() {
    let mut f = Fixture::new();
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(230),
        f.original.count.stock_revision,
        2,
        Occurrence::Known(at(100)),
        at(135),
    );
    assert_eq!(
        f.plan(),
        Err(CountReturnAssociationError::Accounting(
            CorrectionError::ReturnConflict
        ))
    );
    let mut f = Fixture::new();
    let first = f.plan().unwrap();
    f.account = first.after;
    f.cut = first.association.stock_revision;
    f.clock = at(200);
    f.associations.push(first.association);
    assert_eq!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(205)),
        Err(CountReturnAssociationError::AlreadyAssociated)
    );
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(231),
        f.cut,
        1,
        Occurrence::Known(at(110)),
        at(205),
    );
    f.cut = f.returned.stock_revision;
    f.clock = at(205);
    assert_eq!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(210)),
        Err(CountReturnAssociationError::Accounting(
            CorrectionError::ReturnConflict
        ))
    );
    f.associations.clear();
    assert_eq!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(210)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
}

#[test]
fn contradictory_original_claim_is_retained_and_cannot_be_silently_reassigned() {
    let mut f = Fixture::new();
    let claim = ReturnDepartureClaim {
        departure: id(250),
        group: Some(id(251)),
    };
    f.returned.linkage.claim = Some(claim);
    assert_eq!(f.plan(), Err(CountReturnAssociationError::ConflictingClaim));
    assert_eq!(f.returned.linkage.claim, Some(claim));
    let mut f = Fixture::new();
    f.returned.linkage.claim = None;
    f.returned.linkage.resolution = ReturnLinkResolution::Unknown;
    assert_eq!(
        f.plan().unwrap().returned_original.linkage.resolution,
        ReturnLinkResolution::Unknown
    );
}

#[test]
fn association_identity_and_every_source_cut_head_and_evidence_are_purpose_bound() {
    let f = Fixture::new();
    let command = f.command();
    let hub = f.original.known_event.hub;
    let actor = f.original.actor;
    let digest = command.fingerprint(hub, actor, id(240));
    assert_ne!(
        count_return_association_id(id(240)).unwrap(),
        count_explanation_id(id(240)).unwrap()
    );
    assert_ne!(
        count_return_association_id(id(240)).unwrap(),
        count_classification_id(id(240)).unwrap()
    );
    for field in 0..11 {
        let mut changed = command.clone();
        match field {
            0 => changed.return_operation = id(242),
            1 => changed.explanation_operation = id(242),
            2 => changed.expected_stock_revision = f.cut.next().unwrap(),
            3 => changed.expected_catalog_revision = f.catalog.revision().next().unwrap(),
            4 => changed.expected_account_prior = None,
            5 => changed.expected_returns_prior = Some(id(242)),
            6 => changed.source_operation = id(242),
            7 => changed.source_group = id(242),
            8 => {
                changed.source_evidence = SourceProvenance::new("Changed source evidence").unwrap()
            }
            9 => {
                changed.quantity_evidence =
                    SourceProvenance::new("Changed quantity evidence").unwrap()
            }
            _ => {
                changed.timing_evidence = SourceProvenance::new("Changed timing evidence").unwrap()
            }
        }
        assert_ne!(
            changed.fingerprint(hub, actor, id(240)),
            digest,
            "field {field}"
        );
    }
}

#[test]
fn proposed_source_observation_recipient_and_prior_return_operations_cannot_alias_a_dependency() {
    let f = Fixture::new();
    assert!(f.plan().is_ok());
    for operation in [
        f.original.historical_source.historical().operation,
        f.original.count.count.operation,
    ] {
        assert_eq!(
            f.command()
                .plan(&f.context(), operation, f.original.actor, at(200)),
            Err(CountReturnAssociationError::InvalidHistory)
        );
    }
    let mut f = Fixture::new();
    let correction = RecipientAttributionCommand {
        departure_operation: f.original.operation,
        expected_stock_revision: f.cut,
        expected_catalog_revision: f.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            cut: AttributionCut {
                hub: f.original.known_event.hub,
                stock_revision: f.cut,
                catalog: &f.catalog,
                latest_recorded_at: f.clock,
                uncertain_submissions: false,
            },
            original: &f.recipient,
            history: &[],
            linked_issued_versions: &[],
        },
        id(250),
        f.original.actor,
        at(160),
    )
    .unwrap();
    f.cut = correction.stamp.stock_revision;
    f.clock = correction.stamp.recorded_at;
    f.recipient_history.push(correction);
    let positive = f.plan().unwrap();
    assert_eq!(positive.recipient_history.len(), 1);
    assert_eq!(
        f.command()
            .plan(&f.context(), id(250), f.original.actor, at(200)),
        Err(CountReturnAssociationError::InvalidHistory)
    );

    let mut f = Fixture::with_quantity(2);
    let first = f.plan().unwrap();
    f.account = first.after;
    f.cut = first.association.stock_revision;
    f.clock = at(200);
    f.associations.push(first.association);
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(231),
        f.cut,
        1,
        Occurrence::Known(at(110)),
        at(205),
    );
    f.cut = f.returned.stock_revision;
    f.clock = at(205);
    let positive = f
        .command()
        .plan(&f.context(), id(242), f.original.actor, at(210))
        .unwrap();
    assert_eq!(
        positive.after.effective_explanations()[0].established_returns,
        packs(2)
    );
    assert_eq!(positive.prior_associations, f.associations);
    assert!(positive.normal_returns.is_empty());
    assert_eq!(
        f.command()
            .plan(&f.context(), id(230), f.original.actor, at(210)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
}

#[test]
fn independent_originals_cannot_claim_the_same_revision_or_change_a_prior_operation_revision() {
    let mut f = Fixture::new();
    assert!(f.plan().is_ok());
    f.returned.stock_revision = f.original.known_event.stock_revision;
    assert_eq!(f.plan(), Err(CountReturnAssociationError::InvalidHistory));

    let mut f = Fixture::with_quantity(2);
    let first = f.plan().unwrap();
    f.account = first.after;
    f.cut = first.association.stock_revision;
    f.clock = at(200);
    f.associations.push(first.association);
    f.returned = returned(
        &f.catalog,
        &f.original,
        id(231),
        f.cut,
        1,
        Occurrence::Known(at(110)),
        at(205),
    );
    f.cut = f.returned.stock_revision;
    f.clock = at(205);
    assert!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(210))
            .is_ok()
    );
    let pristine = f.associations[0].clone();
    f.associations[0].return_stock_revision = f.original.known_event.stock_revision;
    assert!(f.associations[0].validate().is_ok());
    assert_eq!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(210)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    f.associations[0] = pristine;
    f.associations[0].explanation_stock_revision = f.original.report.stock_revision;
    assert!(f.associations[0].validate().is_ok());
    assert_eq!(
        f.command()
            .plan(&f.context(), id(242), f.original.actor, at(210)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
}

#[test]
fn actual_normal_return_scope_and_zero_effective_count_cannot_be_reused() {
    let f = Fixture::new();
    assert!(f.plan().is_ok());
    let (normal_hub, normal, digest) = crate::hub::returns::normal_return_policy_fixture();
    assert_eq!(
        normal.linkage.resolution,
        ReturnLinkResolution::EstablishedScope
    );
    let mut context = f.context();
    context.returned = &normal;
    context.return_hub = normal_hub;
    context.return_original_digest = digest;
    let mut command = f.command();
    command.return_operation = normal.identities.operation;
    assert_eq!(
        command.plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::AlreadyAssociated)
    );

    let mut f = Fixture::new();
    let amendment = CountExplanationAmendment {
        operation: id(249),
        stock_revision: f.cut.next().unwrap(),
        explanation: f.original.operation,
        before: packs(1),
        after: packs(0),
    };
    f.cut = amendment.stock_revision;
    f.clock = at(170);
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        f.account.explanations().to_vec(),
        vec![amendment],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        f.plan(),
        Err(CountReturnAssociationError::Accounting(
            CorrectionError::ReturnConflict
        ))
    );
}

#[test]
fn native_return_hub_and_all_three_evidence_fields_remain_explicit() {
    let f = Fixture::new();
    let baseline = f.plan().unwrap();
    let mut foreign = f.context();
    foreign.return_hub = HubId::from_bytes([99; 16]).unwrap();
    assert_eq!(
        f.command()
            .plan(&foreign, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    for field in 0..3 {
        let mut command = f.command();
        let changed = SourceProvenance::new("A separately recorded association basis").unwrap();
        match field {
            0 => command.source_evidence = changed,
            1 => command.quantity_evidence = changed,
            _ => command.timing_evidence = changed,
        }
        let plan = command
            .plan(&f.context(), id(240), f.original.actor, at(200))
            .unwrap();
        assert_eq!(plan.command, command);
        assert_ne!(plan, baseline);
        assert_eq!(plan.association, baseline.association);
    }
}
