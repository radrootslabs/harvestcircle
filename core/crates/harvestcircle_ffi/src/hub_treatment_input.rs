//! Closed whole-group treatment input; native runtime supplies actor and record time.
use crate::{
    commands::HarvestCircleError, hub_reference::invalid_input, hub_stock::HubOccurrenceDto,
    hub_treatment::HubAcceptanceStatusDto,
};
use harvestcircle_domain::hub::{
    HubRevision,
    reference::{HubRecordId, SourceProvenance},
    treatment::{
        HUB_TREATMENT_SELECTION_GROUPS, TreatmentAction, TreatmentCommand, TreatmentSelection,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubAcceptanceInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_ids: Vec<String>,
    pub status: HubAcceptanceStatusDto,
    pub occurrence: HubOccurrenceDto,
    pub observation: String,
}
impl HubAcceptanceInputDto {
    pub fn command(self) -> Result<TreatmentCommand, HarvestCircleError> {
        Ok(TreatmentCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            action: TreatmentAction::SetAcceptance {
                selection: selection(self.group_ids)?,
                status: self.status.into(),
            },
            occurred: self.occurrence.decode()?,
            observation: observation(self.observation)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHoldInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub group_ids: Vec<String>,
    pub occurrence: HubOccurrenceDto,
    pub reason: String,
}
impl HubHoldInputDto {
    pub fn command(self) -> Result<TreatmentCommand, HarvestCircleError> {
        Ok(TreatmentCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            action: TreatmentAction::PlaceHold {
                selection: selection(self.group_ids)?,
            },
            occurred: self.occurrence.decode()?,
            observation: observation(self.reason)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubHoldReleaseInputDto {
    pub expected_stock_revision: u64,
    pub expected_catalog_revision: u64,
    pub hold_id: String,
    pub expected_hold_revision: u64,
    pub occurrence: HubOccurrenceDto,
    pub resolution: String,
}
impl HubHoldReleaseInputDto {
    pub fn command(self) -> Result<TreatmentCommand, HarvestCircleError> {
        Ok(TreatmentCommand {
            expected_stock_revision: revision(self.expected_stock_revision)?,
            expected_catalog_revision: revision(self.expected_catalog_revision)?,
            action: TreatmentAction::ReleaseHold {
                hold: id(&self.hold_id)?,
                expected_revision: revision(self.expected_hold_revision)?,
            },
            occurred: self.occurrence.decode()?,
            observation: observation(self.resolution)?,
        })
    }
}

pub(crate) fn revision(value: u64) -> Result<HubRevision, HarvestCircleError> {
    HubRevision::from_value(value).map_err(|_| invalid_input())
}
pub(crate) fn id(value: &str) -> Result<HubRecordId, HarvestCircleError> {
    HubRecordId::from_hex(value).map_err(|_| invalid_input())
}
fn observation(value: String) -> Result<SourceProvenance, HarvestCircleError> {
    SourceProvenance::new(value).map_err(|_| invalid_input())
}
fn selection(values: Vec<String>) -> Result<TreatmentSelection, HarvestCircleError> {
    if values.is_empty() || values.len() > HUB_TREATMENT_SELECTION_GROUPS {
        return Err(invalid_input());
    }
    TreatmentSelection::new(
        values
            .iter()
            .map(|value| id(value))
            .collect::<Result<Vec<_>, _>>()?,
    )
    .map_err(|_| invalid_input())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HubOccurrenceKindDto;

    fn hold(groups: Vec<String>) -> HubHoldInputDto {
        HubHoldInputDto {
            expected_stock_revision: 2,
            expected_catalog_revision: 6,
            group_ids: groups,
            occurrence: HubOccurrenceDto {
                kind: HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            reason: "Observed damage review on these identified groups".into(),
        }
    }

    #[test]
    fn whole_group_treatment_preserves_unknown_time_and_canonical_bounded_selection() {
        let command = hold(vec!["02".repeat(16), "01".repeat(16)])
            .command()
            .unwrap();
        let TreatmentAction::PlaceHold { selection } = command.action else {
            panic!("hold")
        };
        assert_eq!(selection.groups()[0], id(&"01".repeat(16)).unwrap());
        assert_eq!(
            command.occurred,
            harvestcircle_domain::hub::stock::Occurrence::Unknown
        );
        for groups in [
            vec![],
            vec!["bad".into()],
            vec!["01".repeat(16); 2],
            vec!["01".repeat(16); 65],
        ] {
            assert!(hold(groups).command().is_err());
        }
        let mut missing = hold(vec!["01".repeat(16)]);
        missing.reason.clear();
        assert!(missing.command().is_err());
        let mut invalid = hold(vec!["01".repeat(16)]);
        invalid.expected_stock_revision = 0;
        assert!(invalid.command().is_err());
    }

    #[test]
    fn acceptance_and_selected_release_use_distinct_closed_commands() {
        let occurrence = hold(vec![]).occurrence;
        let accept = HubAcceptanceInputDto {
            expected_stock_revision: 2,
            expected_catalog_revision: 6,
            group_ids: vec!["01".repeat(16)],
            status: HubAcceptanceStatusDto::Accepted,
            occurrence: occurrence.clone(),
            observation: "Explicit sample acceptance review".into(),
        }
        .command()
        .unwrap();
        assert!(matches!(
            accept.action,
            TreatmentAction::SetAcceptance {
                status: harvestcircle_domain::hub::treatment::AcceptanceStatus::Accepted,
                ..
            }
        ));
        let input = HubHoldReleaseInputDto {
            expected_stock_revision: 3,
            expected_catalog_revision: 6,
            hold_id: "02".repeat(16),
            expected_hold_revision: 1,
            occurrence,
            resolution: "Reviewed release of only this hold".into(),
        };
        assert!(matches!(
            input.clone().command().unwrap().action,
            TreatmentAction::ReleaseHold { .. }
        ));
        assert!(
            HubHoldReleaseInputDto {
                expected_hold_revision: 0,
                ..input.clone()
            }
            .command()
            .is_err()
        );
        assert!(
            HubHoldReleaseInputDto {
                hold_id: "bad".into(),
                ..input
            }
            .command()
            .is_err()
        );
    }
}
