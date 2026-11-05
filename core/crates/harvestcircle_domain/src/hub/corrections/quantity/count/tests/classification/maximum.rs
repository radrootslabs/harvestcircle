use super::*;

fn operation(n: usize) -> HubRecordId {
    let mut bytes = [0xA6; 16];
    bytes[8..].copy_from_slice(&(n as u64).to_be_bytes());
    HubRecordId::from_bytes(bytes).unwrap()
}

#[test]
fn maximum_shared_count_history_classifies_one_original_without_repeating_account_effects() {
    let mut f = ClassificationFixture::new(0);
    f.observation.stock_revision = rev(5000);
    let count = HUB_CORRECTION_ROWS / 2;
    let mut explanations = f.account.explanations().to_vec();
    let mut amendments = Vec::new();
    for index in 1..count {
        let explanation = CountLossExplanation {
            operation: operation(2 * index),
            stock_revision: rev(20 + 2 * index as u64),
            packs: packs(1),
        };
        explanations.push(explanation);
        amendments.push(CountExplanationAmendment {
            operation: operation(2 * index + 1),
            stock_revision: rev(explanation.stock_revision.value() + 1),
            explanation: explanation.operation,
            before: packs(1),
            after: packs(0),
        });
    }
    f.account = CountLossQuantityAccount::restore_corrected(
        f.account.original(),
        explanations.clone(),
        amendments,
        vec![],
    )
    .unwrap();
    assert_eq!(f.account.explanations().len(), 2048);
    assert_eq!(f.account.amendments().len(), 2047);
    assert_eq!(f.account.unexplained(), packs(1));
    let events: Vec<_> = explanations
        .iter()
        .skip(1)
        .map(|row| ActualKnownDeparture {
            operation: row.operation,
            stock_revision: row.stock_revision,
            ..f.original.known_event.clone()
        })
        .collect();
    let recipients: Vec<_> = events
        .iter()
        .map(|event| OriginalRecipientAttribution {
            operation: event.operation,
            stock_revision: event.stock_revision,
            ..f.recipient.clone()
        })
        .collect();
    let mut context = f.context();
    context.stock_revision = rev(6000);
    context
        .known_departures
        .extend(events.iter().zip(&recipients).map(|(event, recipient)| {
            CorrectionKnownDeparture {
                original: event,
                recipient_original: recipient,
                recipient_history: &[],
                quantity: KnownCorrectionQuantity::CountExplanation {
                    count: &f.original.count,
                    account: &f.account,
                },
            }
        }));
    let mut command = f.command();
    command.expected_stock_revision = rev(6000);
    command.expected_account_prior = f.account.amendments().last().map(|row| row.operation);
    let started = std::time::Instant::now();
    let result = command.plan(&context, id(221), owner(), at(170)).unwrap();
    println!(
        "maximum shared classification: 2048 explanations/4095 prior rows, {:.3}s",
        started.elapsed().as_secs_f64()
    );
    assert_eq!(result.after.explanations(), f.account.explanations());
    assert_eq!(result.after.amendments().len(), 2048);
    assert_eq!(result.after.unexplained(), packs(2));
    assert!(
        result
            .after
            .effective_explanations()
            .iter()
            .all(|row| row.packs == packs(0))
    );
    assert_eq!(result.original, f.original.known_event);
    assert_eq!(result.count, f.original.count);
    assert_eq!(result.after.original(), result.before.original());
}

#[test]
fn shared_account_reuse_accepts_equal_distinct_objects_and_rejects_conflicting_histories() {
    let mut f = ClassificationFixture::new(0);
    f.observation.stock_revision = rev(15);
    let mut event = f.original.known_event.clone();
    event.operation = id(230);
    event.stock_revision = rev(10);
    let recipient = OriginalRecipientAttribution {
        operation: event.operation,
        stock_revision: event.stock_revision,
        ..f.recipient.clone()
    };
    let mut explanations = f.account.explanations().to_vec();
    explanations.push(CountLossExplanation {
        operation: event.operation,
        stock_revision: event.stock_revision,
        packs: packs(1),
    });
    f.account = CountLossQuantityAccount::restore(f.account.original(), explanations).unwrap();
    for case in 0..3 {
        let mut other_count = f.original.count.clone();
        let mut other_account = f.account.clone();
        match case {
            1 => {
                other_count.reason =
                    SourceProvenance::new("Distinct conflicting original count reason").unwrap()
            }
            2 => {
                other_account = CountLossQuantityAccount::restore_corrected(
                    f.account.original(),
                    f.account.explanations().to_vec(),
                    vec![CountExplanationAmendment {
                        operation: id(231),
                        stock_revision: rev(11),
                        explanation: event.operation,
                        before: packs(1),
                        after: packs(0),
                    }],
                    vec![],
                )
                .unwrap()
            }
            _ => {}
        }
        let other = CorrectionKnownDeparture {
            original: &event,
            recipient_original: &recipient,
            recipient_history: &[],
            quantity: KnownCorrectionQuantity::CountExplanation {
                count: &other_count,
                account: &other_account,
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
        command.expected_account_prior = Some(event.operation);
        let result = command.plan(&context, id(221), owner(), at(170));
        if case == 0 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(QuantityEffectError::InvalidHistory));
        }
    }
}
