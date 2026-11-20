use super::*;

#[test]
fn unamended_recipient_matches_original_label_and_id_with_dense_receipt_fields() {
    let mut snapshot = fixture();
    source(&mut snapshot.trace, 20, 21, SourceCertainty::Established);
    snapshot.trace.supplier = Some(record(
        30,
        ReferenceValue::Party {
            label: label("Original supplier"),
        },
    ));
    snapshot.trace.producer = Some(record(
        31,
        ReferenceValue::Party {
            label: label("Original producer"),
        },
    ));
    for text in ["Recipient A", &id(11).to_hex()] {
        let selection = TraceSelection {
            scope: TraceScope::All,
            text: text.into(),
        };
        assert!(!selection.includes(&snapshot.trace).unwrap());
        assert!(snapshot.includes_source_history(&selection).unwrap());
        let unresolved = TraceSelection {
            scope: TraceScope::Unresolved,
            ..selection
        };
        assert!(!snapshot.includes_source_history(&unresolved).unwrap());
    }
    assert!(snapshot.recipient_corrections.is_empty());
    let invalid = TraceSelection {
        scope: TraceScope::SourceImpact(id(20)),
        text: "Recipient A".into(),
    };
    assert_eq!(
        snapshot.includes_source_history(&invalid),
        Err(TraceError::InvalidQuery)
    );
    assert!(
        snapshot
            .includes_source_history(&TraceSelection {
                text: String::new(),
                ..invalid
            })
            .unwrap()
    );
}

#[test]
fn recipient_matching_is_bounded_and_distinguishes_unknown_from_an_unrelated_reference() {
    use crate::hub::corrections::AmendedRecipient;
    let selection = TraceSelection {
        scope: TraceScope::All,
        text: "Unknown recipient".into(),
    };
    assert!(
        selection
            .matches_recipient(&AmendedRecipient::Unknown)
            .unwrap()
    );
    assert!(
        !selection
            .matches_recipient(&AmendedRecipient::Known(
                fixture().exits[0].departure.recipient.clone()
            ))
            .unwrap()
    );
    assert_eq!(
        selection.matches_recipient(&AmendedRecipient::Known(fixture().trace.product)),
        Err(TraceError::InvalidSnapshot)
    );
}
