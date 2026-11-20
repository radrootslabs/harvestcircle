//! Bounded read projections. Shared accounts are encoded once, including zero
//! interpretations. These records contain no native admission or effect grant.
use crate::{
    commands::HarvestCircleError,
    hub_actuals::HubActualKnownInterpretationDto,
    hub_reference::{HubReferenceRecordDto, invalid_input},
    hub_returns::HubReturnTimingDto,
    hub_stock::HubOccurrenceDto,
    hub_trace::operation_text,
};
use harvestcircle_domain::hub::{
    actuals::ActualKnownCountInventory, corrections::CountReturnAssociation,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountInterpretationsDto {
    pub schema: u32,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub latest_recorded_at_unix_s: i64,
    pub accounts: Vec<HubCountInterpretationAccountDto>,
    pub explanations: Vec<HubCountExplanationMeaningDto>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountInterpretationAccountDto {
    pub count_operation_id: String,
    pub reconciliation_id: String,
    pub observation_id: String,
    pub trace_id: String,
    pub variant_id: String,
    pub anchor_group_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub confirmed_on_hand_at_unix_s: i64,
    pub gross_shortage_whole_packs: u32,
    pub explained_whole_packs: u32,
    pub unexplained_whole_packs: u32,
    pub classifications: Vec<HubCountClassificationMeaningDto>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountClassificationMeaningDto {
    pub operation_id: String,
    pub stock_revision: u64,
    pub explanation_operation_id: String,
    pub before_whole_packs: u32,
    pub after_whole_packs: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountExplanationMeaningDto {
    pub operation_id: String,
    pub count_operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub occurrence: HubOccurrenceDto,
    pub original_recipient: HubReferenceRecordDto,
    pub interpretation: HubActualKnownInterpretationDto,
    pub established_return_whole_packs: u32,
}

impl TryFrom<&ActualKnownCountInventory> for HubCountInterpretationsDto {
    type Error = HarvestCircleError;
    fn try_from(value: &ActualKnownCountInventory) -> Result<Self, Self::Error> {
        let facts = value.facts();
        let mut shared = BTreeMap::new();
        for record in &facts.records {
            let count = record
                .count_explanation
                .as_ref()
                .ok_or_else(invalid_input)?;
            shared
                .entry(count.count.operation)
                .or_insert(count.as_ref());
        }
        let mut effective = BTreeMap::new();
        let mut accounts = Vec::with_capacity(shared.len());
        for value in shared.values() {
            let count = &value.count;
            let account = &value.account;
            for item in account.effective_explanations() {
                if effective
                    .insert(item.original.operation, item.established_returns)
                    .is_some()
                {
                    return Err(invalid_input());
                }
            }
            accounts.push(HubCountInterpretationAccountDto {
                count_operation_id: operation_text(count.operation),
                reconciliation_id: count.id.to_hex(),
                observation_id: count.count.id.to_hex(),
                trace_id: count.before.trace.to_hex(),
                variant_id: count.before.variant.to_hex(),
                anchor_group_id: count.before.id.to_hex(),
                stock_revision: count.stock_revision.value(),
                catalog_revision: count.catalog_revision.value(),
                actor_public_key: count.actor.to_hex(),
                recorded_at_unix_s: count.recorded_at.as_seconds(),
                confirmed_on_hand_at_unix_s: count.confirmed_on_hand_at.as_seconds(),
                gross_shortage_whole_packs: account.original().packs.value(),
                explained_whole_packs: account.explained().value(),
                unexplained_whole_packs: account.unexplained().value(),
                classifications: account
                    .amendments()
                    .iter()
                    .map(|item| HubCountClassificationMeaningDto {
                        operation_id: operation_text(item.operation),
                        stock_revision: item.stock_revision.value(),
                        explanation_operation_id: operation_text(item.explanation),
                        before_whole_packs: item.before.value(),
                        after_whole_packs: item.after.value(),
                    })
                    .collect(),
            });
        }
        accounts
            .sort_by_key(|account| (account.stock_revision, account.count_operation_id.clone()));
        let explanations = facts
            .records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                let count = record
                    .count_explanation
                    .as_ref()
                    .ok_or_else(invalid_input)?;
                Ok(HubCountExplanationMeaningDto {
                    operation_id: operation_text(record.original.operation),
                    count_operation_id: operation_text(count.count.operation),
                    stock_revision: record.original.stock_revision.value(),
                    catalog_revision: record.original.catalog_revision.value(),
                    actor_public_key: record.original.actor.to_hex(),
                    recorded_at_unix_s: record.original.recorded_at.as_seconds(),
                    occurrence: record.original.occurred.into(),
                    original_recipient: (&record.original.recipient).into(),
                    interpretation: HubActualKnownInterpretationDto::from_inventory(value, index)?,
                    established_return_whole_packs: effective
                        .get(&record.original.operation)
                        .ok_or_else(invalid_input)?
                        .value(),
                })
            })
            .collect::<Result<Vec<_>, HarvestCircleError>>()?;
        Ok(Self {
            schema: u32::from(
                harvestcircle_domain::hub::actuals::ACTUAL_KNOWN_COUNT_INVENTORY_SCHEMA,
            ),
            stock_revision: facts.stock_revision.value(),
            catalog_revision: facts.catalog_revision.value(),
            latest_recorded_at_unix_s: facts.latest_recorded_at.as_seconds(),
            accounts,
            explanations,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubCountReturnAssociationDto {
    pub id: String,
    pub operation_id: String,
    pub stock_revision: u64,
    pub catalog_revision: u64,
    pub actor_public_key: String,
    pub recorded_at_unix_s: i64,
    pub return_operation_id: String,
    pub return_stock_revision: u64,
    pub returned_id: String,
    pub returned_population_id: String,
    pub returned_group_id: String,
    pub explanation_operation_id: String,
    pub explanation_stock_revision: u64,
    pub explanation_trace_id: String,
    pub count_operation_id: String,
    pub count_stock_revision: u64,
    pub anchor_group_id: String,
    pub variant_id: String,
    pub whole_packs: u32,
    pub return_occurrence: HubOccurrenceDto,
    pub explanation_occurrence: HubOccurrenceDto,
    pub timing: HubReturnTimingDto,
}
impl From<&CountReturnAssociation> for HubCountReturnAssociationDto {
    fn from(value: &CountReturnAssociation) -> Self {
        Self {
            id: value.id.to_hex(),
            operation_id: operation_text(value.operation),
            stock_revision: value.stock_revision.value(),
            catalog_revision: value.catalog_revision.value(),
            actor_public_key: value.actor.to_hex(),
            recorded_at_unix_s: value.recorded_at.as_seconds(),
            return_operation_id: operation_text(value.return_operation),
            return_stock_revision: value.return_stock_revision.value(),
            returned_id: value.returned.to_hex(),
            returned_population_id: value.returned_population.to_hex(),
            returned_group_id: value.returned_group.to_hex(),
            explanation_operation_id: operation_text(value.explanation_operation),
            explanation_stock_revision: value.explanation_stock_revision.value(),
            explanation_trace_id: value.explanation_trace.to_hex(),
            count_operation_id: operation_text(value.count_operation),
            count_stock_revision: value.count_stock_revision.value(),
            anchor_group_id: value.anchor_group.to_hex(),
            variant_id: value.variant.to_hex(),
            whole_packs: value.packs.value(),
            return_occurrence: value.return_occurred.into(),
            explanation_occurrence: value.explanation_occurred.into(),
            timing: value.timing.into(),
        }
    }
}
