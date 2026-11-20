//! Local rendering of already checked native facts. This never sends a message,
//! acknowledges follow-up, supplies admission or changes physical accounting.
use super::*;
use harvestcircle_domain::hub::corrections::{AmendedRecipient, current_recipient_attribution};

pub(super) fn render(out: &mut Html, snapshot: &TraceSnapshot) -> Result<(), LocalOutputError> {
    if snapshot.count_return_uncertain {
        out.paragraph("Return association remains uncertain and needs review.")?;
    }
    if let Some(inventory) = &snapshot.count_interpretations {
        let mut accounts = std::collections::BTreeSet::new();
        let mut effective = std::collections::BTreeMap::new();
        for (record, meaning) in inventory.facts().records.iter().zip(inventory.views()) {
            let value = record
                .count_explanation
                .as_ref()
                .ok_or(LocalOutputError::InvalidSnapshot)?;
            if accounts.insert(value.count.operation) {
                for row in value.account.effective_explanations() {
                    if effective
                        .insert(row.original.operation, row.established_returns)
                        .is_some()
                    {
                        return Err(LocalOutputError::InvalidSnapshot);
                    }
                }
                out.heading("Count shortage account: original and current")?;
                out.field("Count operation", &operation_text(value.count.operation))?;
                out.field("Count reconciliation", &value.count.id.to_hex())?;
                out.field(
                    "Original missing whole packs",
                    &value.account.original().packs.value().to_string(),
                )?;
                out.field(
                    "Current explained whole packs",
                    &value.account.explained().value().to_string(),
                )?;
                out.field(
                    "Current unexplained whole packs",
                    &value.account.unexplained().value().to_string(),
                )?;
                for change in value.account.amendments() {
                    out.field(
                        "Classification operation",
                        &operation_text(change.operation),
                    )?;
                    out.field(
                        "Classified explanation",
                        &operation_text(change.explanation),
                    )?;
                    out.field("Before whole packs", &change.before.value().to_string())?;
                    out.field("After whole packs", &change.after.value().to_string())?;
                    out.field(
                        "Classification stock revision",
                        &change.stock_revision.value().to_string(),
                    )?;
                }
            }
            out.heading("Retained explanation and current meaning")?;
            out.field(
                "Explanation operation",
                &operation_text(record.original.operation),
            )?;
            out.field(
                "Original explained whole packs",
                &meaning.original_packs.value().to_string(),
            )?;
            out.field(
                "Current explained whole packs",
                &meaning.effective_packs.value().to_string(),
            )?;
            reference(out, "Original recipient", &record.original.recipient)?;
            match current_recipient_attribution(
                &record.recipient_original,
                &record.recipient_history,
            )
            .map_err(|_| LocalOutputError::InvalidSnapshot)?
            {
                AmendedRecipient::Known(party) => reference(out, "Current recipient", &party)?,
                AmendedRecipient::Unknown => out.field("Current recipient", "Unknown recipient")?,
            }
            actor_time(out, record.original.actor, record.original.recorded_at)?;
            occurrence(out, record.original.occurred)?;
            out.field(
                "Established returned whole packs",
                &effective
                    .get(&record.original.operation)
                    .ok_or(LocalOutputError::InvalidSnapshot)?
                    .value()
                    .to_string(),
            )?;
        }
    }
    if !snapshot.count_return_associations.is_empty() {
        out.heading("Later whole-intake Return associations")?;
        out.paragraph("Returned stock retains its own population and handling. An association does not add stock, revive work or establish an external acknowledgement.")?;
        for association in &snapshot.count_return_associations {
            out.field(
                "Association operation",
                &operation_text(association.operation),
            )?;
            out.field(
                "Return operation",
                &operation_text(association.return_operation),
            )?;
            out.field(
                "Returned population",
                &association.returned_population.to_hex(),
            )?;
            out.field(
                "Explanation operation",
                &operation_text(association.explanation_operation),
            )?;
            out.field(
                "Explanation population",
                &association.explanation_trace.to_hex(),
            )?;
            out.field(
                "Whole returned packs",
                &association.packs.value().to_string(),
            )?;
            out.heading("Explained departure occurrence")?;
            occurrence(out, association.explanation_occurred)?;
            out.heading("Original Return occurrence")?;
            occurrence(out, association.return_occurred)?;
            out.field(
                "Association timing",
                match association.timing {
                    harvestcircle_domain::hub::returns::ReturnTiming::AfterOrSame => {
                        "Consistent with explained departure"
                    }
                    harvestcircle_domain::hub::returns::ReturnTiming::Uncertain => {
                        "Uncertain; needs review"
                    }
                    harvestcircle_domain::hub::returns::ReturnTiming::Contradictory => {
                        "Conflicting; needs review"
                    }
                },
            )?;
            actor_time(out, association.actor, association.recorded_at)?;
        }
    }
    Ok(())
}
