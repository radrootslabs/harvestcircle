//! Canonical committed observation data. This codec never reads a submission
//! archive or produces admission/session/effect authority.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{CorrectionObservedFacts, FoundStockInput, PhysicalDebitKey},
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceProvenance, WholePacks},
};
use serde::{Deserialize, Serialize};

pub(super) const MAX_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: u8,
    previous: Option<String>,
    facts: Facts,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum Facts {
    Found {
        target_operation: String,
        target_group: String,
        allocations: Vec<Allocation>,
        basis: String,
    },
    CountClassification {
        explanation_operation: String,
        corrected_packs: u32,
        report: Option<String>,
        basis: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allocation {
    debit_operation: String,
    debit_group: String,
    packs: u32,
    location: String,
    label: String,
    observation: String,
    cohort_evidence: String,
}

pub(super) fn encode(command: &CorrectionObservationCommand) -> Result<Vec<u8>, SafeError> {
    command.validate().map_err(error)?;
    let facts = match &command.facts {
        CorrectionObservedFacts::Found {
            target_operation,
            target_group,
            allocations,
            basis,
        } => Facts::Found {
            target_operation: target_operation.to_hex(),
            target_group: target_group.to_hex(),
            allocations: allocations
                .iter()
                .map(|a| Allocation {
                    debit_operation: a.debit.operation.to_hex(),
                    debit_group: a.debit.group.to_hex(),
                    packs: a.packs.value(),
                    location: a.location.to_hex(),
                    label: a.identification.label.as_str().into(),
                    observation: a.identification.observation.as_str().into(),
                    cohort_evidence: a.cohort_evidence.as_str().into(),
                })
                .collect(),
            basis: basis.as_str().into(),
        },
        CorrectionObservedFacts::CountClassification {
            explanation_operation,
            corrected_packs,
            report,
            basis,
        } => Facts::CountClassification {
            explanation_operation: explanation_operation.to_hex(),
            corrected_packs: corrected_packs.value(),
            report: report.map(HubRecordId::to_hex),
            basis: basis.as_str().into(),
        },
    };
    let bytes = serde_json::to_vec(&Payload {
        schema: 1,
        previous: command.previous.map(HubRecordId::to_hex),
        facts,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(stock::capacity());
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> Result<CorrectionObservationCommand, SafeError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(corrupt_storage());
    }
    let payload: Payload = serde_json::from_slice(bytes).map_err(|_| corrupt_storage())?;
    if payload.schema != 1 {
        return Err(corrupt_storage());
    }
    let id = |text: &str| HubRecordId::from_hex(text).map_err(|_| corrupt_storage());
    let note = |text: String| SourceProvenance::new(text).map_err(|_| corrupt_storage());
    let facts = match payload.facts {
        Facts::Found {
            target_operation,
            target_group,
            allocations,
            basis,
        } => {
            if allocations.is_empty()
                || allocations.len() > harvestcircle_domain::hub::corrections::HUB_FOUND_ALLOCATIONS
            {
                return Err(corrupt_storage());
            }
            let mut inputs = Vec::with_capacity(allocations.len());
            for a in allocations {
                inputs.push(FoundStockInput {
                    debit: PhysicalDebitKey {
                        operation: id(&a.debit_operation)?,
                        group: id(&a.debit_group)?,
                    },
                    packs: WholePacks::from_value(a.packs).map_err(|_| corrupt_storage())?,
                    location: id(&a.location)?,
                    identification: PhysicalIdentification {
                        label: HubLabel::new(a.label).map_err(|_| corrupt_storage())?,
                        observation: note(a.observation)?,
                    },
                    cohort_evidence: note(a.cohort_evidence)?,
                });
            }
            CorrectionObservedFacts::Found {
                target_operation: id(&target_operation)?,
                target_group: id(&target_group)?,
                allocations: inputs,
                basis: note(basis)?,
            }
        }
        Facts::CountClassification {
            explanation_operation,
            corrected_packs,
            report,
            basis,
        } => CorrectionObservedFacts::CountClassification {
            explanation_operation: id(&explanation_operation)?,
            corrected_packs: WholePacks::from_value(corrected_packs)
                .map_err(|_| corrupt_storage())?,
            report: report.map(|text| id(&text)).transpose()?,
            basis: note(basis)?,
        },
    };
    let command = CorrectionObservationCommand {
        previous: payload.previous.map(|text| id(&text)).transpose()?,
        facts,
    };
    command.validate().map_err(|_| corrupt_storage())?;
    // Reject duplicate/unknown fields, noncanonical spellings, whitespace and
    // schema drift rather than accepting a second representation of an original.
    if encode(&command)?.as_slice() != bytes {
        return Err(corrupt_storage());
    }
    Ok(command)
}
