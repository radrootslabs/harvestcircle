use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscrepancyAction {
    Open {
        trace: HubRecordId,
        group: Option<HubRecordId>,
        observation: SourceProvenance,
    },
    Resolve {
        discrepancy: HubRecordId,
        expected_revision: HubRevision,
        resolution: SourceProvenance,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscrepancyCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub action: DiscrepancyAction,
}

impl DiscrepancyCommand {
    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, opened_id: HubRecordId) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.discrepancy.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(opened_id.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        match &self.action {
            DiscrepancyAction::Open {
                trace,
                group,
                observation,
            } => {
                hash.update([0]);
                hash.update(trace.as_bytes());
                hash.update([u8::from(group.is_some())]);
                if let Some(group) = group {
                    hash.update(group.as_bytes());
                }
                hash_text(&mut hash, observation.as_str());
            }
            DiscrepancyAction::Resolve {
                discrepancy,
                expected_revision,
                resolution,
            } => {
                hash.update([1]);
                hash.update(discrepancy.as_bytes());
                hash.update(expected_revision.value().to_be_bytes());
                hash_text(&mut hash, resolution.as_str());
            }
        }
        hash.finalize().into()
    }
}

/// Closed actual handling commands, using the existing inventory operation ID.
/// Scope validation and the corresponding pure plan precede any native writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandlingCommand {
    Move(MovementCommand),
    Split(crate::hub::portions::SplitCommand),
    Treatment(crate::hub::treatment::TreatmentCommand),
    Replan(ReplanCommand),
    NewWork(NewWorkCommand),
    Discrepancy(DiscrepancyCommand),
}

impl HandlingCommand {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Move(_) => "move",
            Self::Split(_) => "split",
            Self::Treatment(command) => command.kind(),
            Self::Replan(_) => "work_replan",
            Self::NewWork(_) => "work_new",
            Self::Discrepancy(command) => match command.action {
                DiscrepancyAction::Open { .. } => "discrepancy_open",
                DiscrepancyAction::Resolve { .. } => "discrepancy_resolve",
            },
        }
    }

    #[must_use]
    pub fn expected_versions(&self) -> (HubRevision, HubRevision) {
        match self {
            Self::Move(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
            Self::Treatment(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
            Self::Split(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
            Self::Replan(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
            Self::NewWork(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
            Self::Discrepancy(command) => (
                command.expected_stock_revision,
                command.expected_catalog_revision,
            ),
        }
    }

    pub fn validate_shape(&self) -> Result<(), MovementError> {
        if let Self::NewWork(command) = self {
            command.validate_shape()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, ids: HandlingIdentities) -> [u8; 32] {
        match self {
            Self::Move(command) => command.fingerprint(hub, actor, ids.group),
            Self::Split(command) => command.fingerprint(hub, actor, ids.group),
            Self::Treatment(command) => command.fingerprint(hub, actor, ids.treatment),
            Self::Replan(command) => command.fingerprint(hub, actor, ids.requirement),
            Self::NewWork(command) => command.fingerprint(hub, actor, ids.requirement),
            Self::Discrepancy(command) => command.fingerprint(hub, actor, ids.discrepancy),
        }
    }
}
