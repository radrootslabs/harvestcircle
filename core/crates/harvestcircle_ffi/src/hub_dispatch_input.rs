//! Closed preparation inputs; native authority supplies actor, IDs and clocks.
use crate::commands::HarvestCircleError;
use crate::hub_reference::invalid_input;
use crate::hub_treatment_input::{id, revision};
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection,
};
use harvestcircle_domain::hub::reference::{SourceProvenance, WholePacks};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchLineInputDto {
    pub group_id: String,
    pub expected_group_revision: u64,
    pub whole_packs: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchMaterialInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub recipient_id: String,
    pub lines: Vec<HubDispatchLineInputDto>,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchEditInputDto {
    pub preparation_id: String,
    pub expected_preparation_revision: u64,
    pub material: HubDispatchMaterialInputDto,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubDispatchActionInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub preparation_id: String,
    pub expected_preparation_revision: u64,
    pub observation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Enum))]
pub enum HubDispatchInputDto {
    Prepare { value: HubDispatchMaterialInputDto },
    Revise { value: HubDispatchEditInputDto },
    Issue { value: HubDispatchActionInputDto },
    Replace { value: HubDispatchEditInputDto },
    MarkUnused { value: HubDispatchActionInputDto },
}
impl HubDispatchInputDto {
    pub fn command(self) -> Result<DispatchCommand, HarvestCircleError> {
        let replace = matches!(&self, Self::Replace { .. });
        let unused = matches!(&self, Self::MarkUnused { .. });
        match self {
            Self::Prepare { value } => material_command(value, None),
            Self::Revise { value } | Self::Replace { value } => material_command(
                value.material,
                Some((
                    id(&value.preparation_id)?,
                    revision(value.expected_preparation_revision)?,
                    replace,
                )),
            ),
            Self::Issue { value } | Self::MarkUnused { value } => {
                let preparation = id(&value.preparation_id)?;
                let expected_revision = revision(value.expected_preparation_revision)?;
                Ok(DispatchCommand {
                    expected_stock_revision: revision(value.expected_stock_revision)?,
                    expected_catalog_revision: revision(value.expected_catalog_revision)?,
                    action: if unused {
                        DispatchAction::MarkUnused {
                            preparation,
                            expected_revision,
                        }
                    } else {
                        DispatchAction::Issue {
                            preparation,
                            expected_revision,
                        }
                    },
                    observation: SourceProvenance::new(value.observation)
                        .map_err(|_| invalid_input())?,
                })
            }
        }
    }
}
fn material_command(
    value: HubDispatchMaterialInputDto,
    target: Option<(
        harvestcircle_domain::hub::reference::HubRecordId,
        harvestcircle_domain::hub::HubRevision,
        bool,
    )>,
) -> Result<DispatchCommand, HarvestCircleError> {
    if value.lines.is_empty() || value.lines.len() > 64 {
        return Err(invalid_input());
    }
    let lines = DispatchSelection::new(
        value
            .lines
            .into_iter()
            .map(|line| {
                Ok(DispatchLineInput {
                    group: id(&line.group_id)?,
                    expected_group_revision: revision(line.expected_group_revision)?,
                    packs: WholePacks::from_value(line.whole_packs).map_err(|_| invalid_input())?,
                })
            })
            .collect::<Result<Vec<_>, HarvestCircleError>>()?,
    )
    .map_err(|_| invalid_input())?;
    let recipient = id(&value.recipient_id)?;
    let action = match target {
        None => DispatchAction::Prepare { recipient, lines },
        Some((preparation, expected_revision, true)) => DispatchAction::Replace {
            preparation,
            expected_revision,
            recipient,
            lines,
        },
        Some((preparation, expected_revision, false)) => DispatchAction::Revise {
            preparation,
            expected_revision,
            recipient,
            lines,
        },
    };
    Ok(DispatchCommand {
        expected_stock_revision: revision(value.expected_stock_revision)?,
        expected_catalog_revision: revision(value.expected_catalog_revision)?,
        action,
        observation: SourceProvenance::new(value.observation).map_err(|_| invalid_input())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> HubDispatchMaterialInputDto {
        HubDispatchMaterialInputDto {
            expected_stock_revision: 2,
            expected_catalog_revision: 6,
            recipient_id: "03".repeat(16),
            lines: vec![HubDispatchLineInputDto {
                group_id: "02".repeat(16),
                expected_group_revision: 1,
                whole_packs: 20,
            }],
            observation: "Reviewed sample recipient and original crates".into(),
        }
    }
    #[test]
    fn hub_dispatch_codec_rejects_unbounded_duplicate_invalid_and_zero_lines() {
        let good = material();
        assert!(
            HubDispatchInputDto::Prepare {
                value: good.clone()
            }
            .command()
            .is_ok()
        );
        for lines in [
            vec![],
            vec![good.lines[0].clone(); 2],
            vec![good.lines[0].clone(); 65],
            vec![HubDispatchLineInputDto {
                whole_packs: 0,
                ..good.lines[0].clone()
            }],
            vec![HubDispatchLineInputDto {
                expected_group_revision: 0,
                ..good.lines[0].clone()
            }],
            vec![HubDispatchLineInputDto {
                group_id: "bad".into(),
                ..good.lines[0].clone()
            }],
        ] {
            assert!(
                HubDispatchInputDto::Prepare {
                    value: HubDispatchMaterialInputDto {
                        lines,
                        ..good.clone()
                    }
                }
                .command()
                .is_err()
            );
        }
        for value in [
            HubDispatchMaterialInputDto {
                recipient_id: "bad".into(),
                ..good.clone()
            },
            HubDispatchMaterialInputDto {
                observation: String::new(),
                ..good.clone()
            },
            HubDispatchMaterialInputDto {
                expected_stock_revision: 0,
                ..good
            },
        ] {
            assert!(HubDispatchInputDto::Prepare { value }.command().is_err());
        }
    }
    #[test]
    fn hub_dispatch_codec_keeps_issue_replace_and_unused_distinct() {
        let action = HubDispatchActionInputDto {
            expected_stock_revision: 2,
            expected_catalog_revision: 6,
            preparation_id: "01".repeat(16),
            expected_preparation_revision: 1,
            observation: "Explicit review".into(),
        };
        assert!(matches!(
            HubDispatchInputDto::Issue {
                value: action.clone()
            }
            .command()
            .unwrap()
            .action,
            DispatchAction::Issue { .. }
        ));
        assert!(matches!(
            HubDispatchInputDto::MarkUnused {
                value: action.clone()
            }
            .command()
            .unwrap()
            .action,
            DispatchAction::MarkUnused { .. }
        ));
        assert!(
            HubDispatchInputDto::Issue {
                value: HubDispatchActionInputDto {
                    expected_preparation_revision: 0,
                    ..action
                }
            }
            .command()
            .is_err()
        );
        let edit = HubDispatchEditInputDto {
            preparation_id: "01".repeat(16),
            expected_preparation_revision: 1,
            material: material(),
        };
        assert!(matches!(
            HubDispatchInputDto::Revise {
                value: edit.clone()
            }
            .command()
            .unwrap()
            .action,
            DispatchAction::Revise { .. }
        ));
        assert!(matches!(
            HubDispatchInputDto::Replace { value: edit }
                .command()
                .unwrap()
                .action,
            DispatchAction::Replace { .. }
        ));
    }
}
