use super::*;

fn paper(c: &ReferenceCatalog) -> AttributionAffectedIssuedVersion {
    AttributionAffectedIssuedVersion {
        version: id(31),
        stock_revision: rev(10),
        catalog_revision: c.revision(),
        populations: vec![id(11), id(99)],
        recipient: c.get(id(3)).unwrap().clone(),
    }
}

#[test]
fn source_scope_retains_whole_issued_paper_without_inventing_a_departure() {
    let c = catalog();
    let original = original_source(&c);
    let papers = [paper(&c)];
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
        issued_versions: &papers,
    };
    let plan = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    for target in [
        CorrectionImpact::Population(id(11)),
        CorrectionImpact::Population(id(99)),
        CorrectionImpact::Recipient(id(3)),
        CorrectionImpact::IssuedVersion(id(31)),
    ] {
        assert!(plan.impact.contains(target));
    }
    assert!(
        !plan
            .impact
            .rows()
            .iter()
            .any(|r| matches!(r, CorrectionImpact::Departure(_)))
    );
}

#[test]
fn malformed_duplicate_future_or_unbounded_issued_scope_cannot_be_a_source_plan() {
    let c = catalog();
    let original = original_source(&c);
    for fault in 0..9 {
        let mut papers = vec![paper(&c)];
        let expected = match fault {
            0 => {
                papers[0].populations = vec![id(99)];
                AttributionError::InvalidScope
            }
            1 => {
                papers[0].populations = vec![id(11), id(11)];
                AttributionError::InvalidHistory
            }
            2 => {
                papers[0].stock_revision = rev(21);
                AttributionError::InvalidScope
            }
            3 => {
                papers[0].catalog_revision = c.revision().next().unwrap();
                AttributionError::InvalidScope
            }
            4 => {
                papers[0].version = id(40);
                AttributionError::InvalidScope
            }
            5 => {
                papers.push(papers[0].clone());
                AttributionError::InvalidScope
            }
            6 => {
                papers[0].recipient.value = ReferenceValue::Product {
                    label: HubLabel::new("Not a recipient").unwrap(),
                };
                AttributionError::InvalidReference
            }
            7 => {
                papers = vec![paper(&c); HUB_CORRECTION_IMPACTS + 1];
                AttributionError::Capacity
            }
            _ => {
                papers[0].stock_revision = HubRevision::INITIAL;
                AttributionError::InvalidHistory
            }
        };
        let context = SourceAttributionContext {
            cut: cut(&c),
            original: &original,
            history: &[],
            known_sources: &[],
            affected: &[],
            issued_versions: &papers,
        };
        assert_eq!(
            source_command(&c).plan(&context, id(40), actor(), at(120)),
            Err(expected),
            "fault {fault}"
        );
    }
}

#[test]
fn a_later_source_amendment_cannot_drop_an_earlier_issued_version_or_other_population() {
    let c = catalog();
    let original = original_source(&c);
    let papers = [paper(&c)];
    let context = SourceAttributionContext {
        cut: cut(&c),
        original: &original,
        history: &[],
        known_sources: &[],
        affected: &[],
        issued_versions: &papers,
    };
    let first = source_command(&c)
        .plan(&context, id(40), actor(), at(120))
        .unwrap();
    let history = [first];
    let mut next = source_command(&c);
    next.expected_stock_revision = rev(21);
    next.expected_prior = Some(id(40));
    next.source = SourceAttribution::Unknown;
    let mut context = SourceAttributionContext {
        cut: AttributionCut {
            stock_revision: rev(21),
            latest_recorded_at: at(120),
            ..cut(&c)
        },
        original: &original,
        history: &history,
        known_sources: &[],
        affected: &[],
        issued_versions: &[],
    };
    assert_eq!(
        next.plan(&context, id(41), actor(), at(121)),
        Err(AttributionError::InvalidScope)
    );
    context.issued_versions = &papers;
    assert!(
        next.plan(&context, id(41), actor(), at(121))
            .unwrap()
            .impact
            .contains(CorrectionImpact::Population(id(99)))
    );
}
