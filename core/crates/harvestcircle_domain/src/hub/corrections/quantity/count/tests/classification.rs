use super::*;

mod maximum;

struct ClassificationFixture {
    fixture: Fixture,
    original: CountExplanationPlan,
    account: CountLossQuantityAccount,
    recipient: OriginalRecipientAttribution,
    observation: CorrectionObservationRecord,
}
impl ClassificationFixture {
    fn new(corrected: u32) -> Self {
        let fixture = Fixture::new();
        let original = fixture.plan().unwrap();
        let account = CountLossQuantityAccount::restore(
            fixture.account.original(),
            vec![CountLossExplanation {
                operation: original.operation,
                stock_revision: original.explanation.stock_revision,
                packs: packs(1),
            }],
        )
        .unwrap();
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
            issued_versions: vec![],
        };
        let observation = CorrectionObservationCommand {
            previous: None,
            facts: CorrectionObservedFacts::CountClassification {
                explanation_operation: original.operation,
                corrected_packs: packs(corrected),
                report: None,
                basis: note(),
            },
        }
        .preserve(
            known.hub,
            known.stock_revision,
            fixture.catalog.revision(),
            id(220),
            owner(),
            at(160),
        )
        .unwrap();
        Self {
            fixture,
            original,
            account,
            recipient,
            observation,
        }
    }
    fn command(&self) -> CountClassificationCommand {
        CountClassificationCommand {
            observation_operation: self.observation.operation,
            expected_stock_revision: self.observation.stock_revision,
            expected_catalog_revision: self.fixture.catalog.revision(),
            expected_account_prior: self.account.explanations().last().map(|r| r.operation),
            expected_prior: None,
        }
    }
    fn context(&self) -> CountClassificationContext<'_> {
        CountClassificationContext {
            hub: self.recipient.hub,
            stock_revision: self.observation.stock_revision,
            catalog: &self.fixture.catalog,
            latest_recorded_at: at(160),
            original: &self.original,
            account: &self.account,
            recipient_original: &self.recipient,
            recipient_history: &[],
            observation: &self.observation,
            observation_successor: None,
            observation_used_by: None,
            report: None,
            prior_report_review: None,
            known_departures: vec![CorrectionKnownDeparture {
                original: &self.original.known_event,
                recipient_original: &self.recipient,
                recipient_history: &[],
                quantity: KnownCorrectionQuantity::CountExplanation {
                    count: &self.original.count,
                    account: &self.account,
                },
            }],
            uncertain_submissions: false,
            unresolved_return_association: false,
        }
    }
    fn plan(&self) -> Result<CountClassificationPlan, QuantityEffectError> {
        self.command()
            .plan(&self.context(), id(221), owner(), at(170))
    }
}

#[test]
fn zero_reclassifies_allowance_without_changing_original_physical_or_work_effect() {
    let f = ClassificationFixture::new(0);
    let result = f.plan().unwrap();
    assert_eq!(result.before.unexplained(), packs(1));
    assert_eq!(result.after.unexplained(), packs(2));
    assert_eq!(result.after.effective_explanations()[0].packs, packs(0));
    assert_eq!(result.after.explanations()[0].packs, packs(1));
    assert_eq!(result.original, f.original.known_event);
    assert_eq!(result.count, f.fixture.count);
    assert_eq!(result.observation, f.observation);
    assert_eq!(result.report, None);
    assert_eq!(
        result.amendment.stock_revision,
        f.observation.stock_revision.next().unwrap()
    );
}

#[test]
fn corrected_total_is_bounded_and_unchanged_quantity_is_not_a_new_effect() {
    assert!(ClassificationFixture::new(2).plan().is_ok());
    assert_eq!(
        ClassificationFixture::new(1).plan(),
        Err(QuantityEffectError::InvalidFacts)
    );
    assert!(matches!(
        ClassificationFixture::new(3).plan(),
        Err(QuantityEffectError::Accounting(_))
    ));
}

#[test]
fn stale_heads_pending_and_consumed_or_superseded_observations_fail_closed() {
    let f = ClassificationFixture::new(0);
    for field in 0..4 {
        let mut command = f.command();
        match field {
            0 => command.expected_stock_revision = rev(99),
            1 => command.expected_catalog_revision = rev(99),
            2 => command.expected_account_prior = None,
            _ => command.expected_prior = Some(id(230)),
        }
        assert_eq!(
            command.plan(&f.context(), id(221), owner(), at(170)),
            Err(QuantityEffectError::StaleRevision)
        );
    }
    for field in 0..4 {
        let mut context = f.context();
        match field {
            0 => context.uncertain_submissions = true,
            1 => context.observation_used_by = Some(id(230)),
            2 => context.observation_successor = Some(id(230)),
            _ => context.prior_report_review = Some(id(230)),
        }
        let expected = if field == 0 {
            QuantityEffectError::UncertainSubmission
        } else {
            QuantityEffectError::AlreadyReviewed
        };
        assert_eq!(
            f.command().plan(&context, id(221), owner(), at(170)),
            Err(expected)
        );
    }
}

#[test]
fn established_returns_and_unknown_associations_block_only_unsafe_decrease() {
    let mut f = ClassificationFixture::new(0);
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        f.account.explanations().to_vec(),
        vec![],
        vec![CountExplanationReturns {
            explanation: f.original.operation,
            packs: packs(1),
        }],
    )
    .unwrap();
    assert!(matches!(f.plan(), Err(QuantityEffectError::Accounting(_))));
    let f = ClassificationFixture::new(0);
    let mut context = f.context();
    context.unresolved_return_association = true;
    assert_eq!(
        f.command().plan(&context, id(221), owner(), at(170)),
        Err(QuantityEffectError::Accounting(
            CorrectionError::ReturnConflict
        ))
    );
    let f = ClassificationFixture::new(2);
    let mut context = f.context();
    context.unresolved_return_association = true;
    assert!(
        f.command()
            .plan(&context, id(221), owner(), at(170))
            .is_ok()
    );
}

#[test]
fn classification_intent_binds_observation_both_cuts_and_both_heads() {
    let f = ClassificationFixture::new(0);
    let command = f.command();
    let digest = command.fingerprint(f.recipient.hub, owner(), id(221));
    for field in 0..5 {
        let mut changed = command.clone();
        match field {
            0 => changed.observation_operation = id(230),
            1 => changed.expected_stock_revision = rev(99),
            2 => changed.expected_catalog_revision = rev(99),
            3 => changed.expected_account_prior = None,
            _ => changed.expected_prior = Some(id(230)),
        }
        assert_ne!(
            digest,
            changed.fingerprint(f.recipient.hub, owner(), id(221))
        );
    }
}

#[test]
fn optional_report_correlates_corrected_total_and_exact_original_source() {
    let mut f = ClassificationFixture::new(2);
    let mut raw = f.fixture.report.clone();
    raw = raw
        .report
        .preserve(
            raw.hub,
            f.original.known_event.stock_revision,
            f.fixture.catalog.revision(),
            id(222),
            owner(),
            at(155),
        )
        .unwrap();
    raw.report.packs = Some(packs(2));
    // Distinct rows must have distinct revisions, even when their clocks overlap.
    f.observation = f
        .observation
        .observation
        .preserve(
            f.recipient.hub,
            raw.stock_revision,
            f.fixture.catalog.revision(),
            id(220),
            owner(),
            at(160),
        )
        .unwrap();
    let CorrectionObservedFacts::CountClassification { report, .. } =
        &mut f.observation.observation.facts
    else {
        unreachable!()
    };
    *report = Some(raw.id);
    let mut context = f.context();
    context.report = Some(&raw);
    assert!(
        f.command()
            .plan(&context, id(221), owner(), at(170))
            .is_ok()
    );
    for field in 0..5 {
        let mut changed = raw.clone();
        match field {
            0 => changed.report.packs = Some(packs(1)), // delta is not the corrected total
            1 => changed.report.recipient = Some(id(3)),
            2 => changed.report.occurred = Occurrence::Known(at(71)),
            3 => {
                changed.report.sources =
                    ActualReportSources::EstablishedClaim(ReportedActualSource {
                        trace: f.original.known_event.trace,
                        group: Some(id(230)),
                        location: None,
                    })
            }
            _ => changed.report.variant = Some(id(230)),
        }
        let mut context = f.context();
        context.report = Some(&changed);
        assert!(
            f.command()
                .plan(&context, id(221), owner(), at(170))
                .is_err(),
            "field {field}"
        );
    }
    raw.report.sources = ActualReportSources::Unknown;
    raw.report.recipient = None;
    raw.report.occurred = Occurrence::Unknown;
    raw.report.packs = None;
    let mut context = f.context();
    context.report = Some(&raw);
    let result = f
        .command()
        .plan(&context, id(221), owner(), at(170))
        .unwrap();
    assert_eq!(result.report, Some(raw));
}

#[test]
fn captured_dependencies_and_inventory_cannot_alias_the_new_operation_or_be_omitted() {
    let f = ClassificationFixture::new(0);
    for operation in [
        f.observation.operation,
        f.observation.id,
        f.original.operation,
        f.original.count.operation,
        f.original.count.count.operation,
        f.fixture.report.operation,
        f.fixture.source.operation,
    ] {
        assert_eq!(
            f.command().plan(&f.context(), operation, owner(), at(170)),
            Err(QuantityEffectError::InvalidHistory)
        );
    }
    let mut context = f.context();
    context.known_departures.clear();
    assert_eq!(
        f.command().plan(&context, id(221), owner(), at(170)),
        Err(QuantityEffectError::InvalidHistory)
    );
    let mut context = f.context();
    context.known_departures.push(context.known_departures[0]);
    assert_eq!(
        f.command().plan(&context, id(221), owner(), at(170)),
        Err(QuantityEffectError::InvalidHistory)
    );
}

#[test]
fn unrelated_valid_known_histories_participate_in_global_identity_and_revision_fences() {
    let mut f = ClassificationFixture::new(0);
    f.observation.stock_revision = rev(15);
    let mut event = f.original.known_event.clone();
    event.operation = id(230);
    event.stock_revision = rev(10);
    let recipient = OriginalRecipientAttribution {
        family: CorrectedDepartureFamily::ReviewedActual,
        operation: event.operation,
        stock_revision: event.stock_revision,
        ..f.recipient.clone()
    };
    let change = RecipientAttributionCommand {
        departure_operation: event.operation,
        expected_stock_revision: rev(10),
        expected_catalog_revision: f.fixture.catalog.revision(),
        expected_prior: None,
        recipient: None,
        basis: note(),
    }
    .plan(
        &RecipientAttributionContext {
            linked_issued_versions: &[],
            original: &recipient,
            history: &[],
            cut: AttributionCut {
                hub: event.hub,
                stock_revision: rev(10),
                catalog: &f.fixture.catalog,
                latest_recorded_at: at(160),
                uncertain_submissions: false,
            },
        },
        id(231),
        owner(),
        at(160),
    )
    .unwrap();
    for mode in 0..4 {
        let amendment = DepartureQuantityAmendment {
            operation: if mode == 3 {
                f.observation.operation
            } else {
                id(232)
            },
            stock_revision: if mode == 2 {
                f.observation.stock_revision
            } else {
                rev(11)
            },
            before: packs(1),
            after: packs(0),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(1)),
        };
        let accounts = [DepartureQuantityAccount::restore(
            OriginalDepartureQuantity {
                operation: event.operation,
                group: event.lines[0].group,
                trace: event.trace,
                variant: event.variant.id,
                stock_revision: event.stock_revision,
                packs: packs(1),
            },
            if mode == 0 { vec![] } else { vec![amendment] },
            packs(0),
        )
        .unwrap()];
        let history = if mode == 0 {
            vec![change.clone()]
        } else {
            vec![]
        };
        let other = CorrectionKnownDeparture {
            original: &event,
            recipient_original: &recipient,
            recipient_history: &history,
            quantity: KnownCorrectionQuantity::Physical(&accounts),
        };
        assert!(
            other
                .associations_through(event.hub, rev(20), f.fixture.catalog.revision(), at(160))
                .is_ok()
        );
        let mut context = f.context();
        context.stock_revision = rev(20);
        context.known_departures.push(other);
        let mut command = f.command();
        command.expected_stock_revision = rev(20);
        if mode < 2 {
            assert!(command.plan(&context, id(221), owner(), at(170)).is_ok());
        }
        let operation = if mode == 0 {
            id(231)
        } else if mode == 1 {
            id(232)
        } else {
            id(221)
        };
        assert_eq!(
            command.plan(&context, operation, owner(), at(170)),
            Err(QuantityEffectError::InvalidHistory),
            "mode {mode}"
        );
    }
}

#[test]
fn unrelated_count_classification_history_also_reserves_its_operation_identity() {
    let mut f = ClassificationFixture::new(0);
    f.observation.stock_revision = rev(15);
    let mut count = f.original.count.clone();
    count.operation = id(240);
    count.id = crate::hub::counts::count_reconciliation_id(count.operation).unwrap();
    count.count.operation = id(241);
    count.count.id = crate::hub::counts::count_record_id(count.count.operation).unwrap();
    count.count.scope.stock_revision = rev(8);
    count.count.recorded_revision = rev(9);
    count.stock_revision = rev(10);
    count.validate().unwrap();
    let mut event = f.original.known_event.clone();
    event.operation = id(230);
    event.stock_revision = rev(11);
    let recipient = OriginalRecipientAttribution {
        operation: event.operation,
        stock_revision: event.stock_revision,
        ..f.recipient.clone()
    };
    let account = CountLossQuantityAccount::restore_corrected(
        OriginalCountLossQuantity {
            reconciliation: count.id,
            operation: count.operation,
            stock_revision: count.stock_revision,
            trace: event.trace,
            variant: event.variant.id,
            packs: packs(2),
        },
        vec![CountLossExplanation {
            operation: event.operation,
            stock_revision: rev(11),
            packs: packs(1),
        }],
        vec![CountExplanationAmendment {
            operation: id(232),
            stock_revision: rev(12),
            explanation: event.operation,
            before: packs(1),
            after: packs(0),
        }],
        vec![],
    )
    .unwrap();
    let other = CorrectionKnownDeparture {
        original: &event,
        recipient_original: &recipient,
        recipient_history: &[],
        quantity: KnownCorrectionQuantity::CountExplanation {
            count: &count,
            account: &account,
        },
    };
    assert!(
        other
            .associations_through(event.hub, rev(20), f.fixture.catalog.revision(), at(160))
            .is_ok()
    );
    let mut context = f.context();
    context.stock_revision = rev(20);
    context.known_departures.push(other);
    let mut command = f.command();
    command.expected_stock_revision = rev(20);
    assert!(command.plan(&context, id(221), owner(), at(170)).is_ok());
    assert_eq!(
        command.plan(&context, id(232), owner(), at(170)),
        Err(QuantityEffectError::InvalidHistory)
    );
}

#[test]
fn distinct_physical_lines_cannot_share_an_amendment_and_global_history_reserves_one_row() {
    let mut f = ClassificationFixture::new(0);
    f.observation.stock_revision = rev(5000);
    let mut event = f.original.known_event.clone();
    event.operation = id(230);
    event.stock_revision = rev(10);
    let mut second = event.lines[0].clone();
    second.group = id(233);
    event.lines.push(second);
    let recipient = OriginalRecipientAttribution {
        family: CorrectedDepartureFamily::ReviewedActual,
        operation: event.operation,
        stock_revision: event.stock_revision,
        ..f.recipient.clone()
    };
    let make_account = |group, start: u64, rows: usize| {
        let mut history = Vec::new();
        for index in 0..rows {
            let ordinal = start + index as u64;
            let decrease = index % 2 == 0;
            history.push(DepartureQuantityAmendment {
                operation: HubRecordId::from_bytes((10_000 + ordinal as u128).to_be_bytes())
                    .unwrap(),
                stock_revision: rev(20 + ordinal),
                before: packs(u32::from(decrease)),
                after: packs(u32::from(!decrease)),
                delta: if decrease {
                    DepartureQuantityDelta::IdentifiedFoundRemainder(packs(1))
                } else {
                    DepartureQuantityDelta::AdditionalDeparture(packs(1))
                },
            });
        }
        DepartureQuantityAccount::restore(
            OriginalDepartureQuantity {
                operation: event.operation,
                group,
                trace: event.trace,
                variant: event.variant.id,
                stock_revision: event.stock_revision,
                packs: packs(1),
            },
            history,
            packs(0),
        )
        .unwrap()
    };
    for case in 0..4 {
        let accounts = match case {
            0 => vec![
                make_account(event.lines[0].group, 0, 1),
                make_account(event.lines[1].group, 1, 1),
            ],
            1 => vec![
                make_account(event.lines[0].group, 0, 1),
                make_account(event.lines[1].group, 0, 1),
            ],
            2 => vec![
                make_account(event.lines[0].group, 0, 2047),
                make_account(event.lines[1].group, 2048, 2047),
            ],
            _ => vec![
                make_account(event.lines[0].group, 0, 2048),
                make_account(event.lines[1].group, 2048, 2047),
            ],
        };
        let other = CorrectionKnownDeparture {
            original: &event,
            recipient_original: &recipient,
            recipient_history: &[],
            quantity: KnownCorrectionQuantity::Physical(&accounts),
        };
        assert!(
            other
                .associations_through(event.hub, rev(6000), f.fixture.catalog.revision(), at(160))
                .is_ok()
        );
        let mut context = f.context();
        context.stock_revision = rev(6000);
        context.known_departures.push(other);
        let mut command = f.command();
        command.expected_stock_revision = rev(6000);
        let result = command.plan(&context, id(221), owner(), at(170));
        match case {
            0 | 2 => {
                assert!(result.is_ok(), "case {case}: {result:?}");
            }
            1 => assert_eq!(result, Err(QuantityEffectError::InvalidHistory)),
            _ => assert_eq!(
                result,
                Err(QuantityEffectError::Actual(ActualReportError::Capacity))
            ),
        }
    }
}

#[test]
fn later_classification_requires_latest_account_and_target_heads_and_keeps_original() {
    let mut f = ClassificationFixture::new(0);
    let first = f.plan().unwrap();
    f.account = first.after;
    f.observation = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: f.original.operation,
            corrected_packs: packs(1),
            report: None,
            basis: note(),
        },
    }
    .preserve(
        f.recipient.hub,
        first.amendment.stock_revision,
        f.fixture.catalog.revision(),
        id(222),
        owner(),
        at(160),
    )
    .unwrap();
    let mut command = f.command();
    command.expected_account_prior = Some(first.operation);
    command.expected_prior = Some(first.operation);
    let result = command
        .plan(&f.context(), id(223), owner(), at(170))
        .unwrap();
    assert_eq!(result.amendment.before, packs(0));
    assert_eq!(result.amendment.after, packs(1));
    assert_eq!(result.after.amendments().len(), 2);
    assert_eq!(result.original, first.original);
    assert_eq!(result.count, first.count);
    for target in [false, true] {
        let mut stale = command.clone();
        if target {
            stale.expected_prior = None;
        } else {
            stale.expected_account_prior = None;
        }
        assert_eq!(
            stale.plan(&f.context(), id(223), owner(), at(170)),
            Err(QuantityEffectError::StaleRevision)
        );
    }
}
