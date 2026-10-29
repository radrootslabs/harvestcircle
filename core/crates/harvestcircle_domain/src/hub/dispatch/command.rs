use super::*;
use crate::hub::movement::hash_text;

pub struct DispatchContext<'a> {
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog: &'a ReferenceCatalog,
    pub sources: &'a [DispatchSource],
    pub current: Option<&'a DispatchRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchAction {
    Prepare {
        recipient: HubRecordId,
        lines: DispatchSelection,
    },
    Revise {
        preparation: HubRecordId,
        expected_revision: HubRevision,
        recipient: HubRecordId,
        lines: DispatchSelection,
    },
    Issue {
        preparation: HubRecordId,
        expected_revision: HubRevision,
    },
    Replace {
        preparation: HubRecordId,
        expected_revision: HubRevision,
        recipient: HubRecordId,
        lines: DispatchSelection,
    },
    MarkUnused {
        preparation: HubRecordId,
        expected_revision: HubRevision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DispatchCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub action: DispatchAction,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchPlan {
    /// Journal revision only. This plan has no physical stock/work effect.
    pub stock_revision: HubRevision,
    pub record: DispatchRecord,
}

impl DispatchAction {
    #[must_use]
    pub fn target(&self) -> Option<(HubRecordId, HubRevision)> {
        match self {
            Self::Prepare { .. } => None,
            Self::Revise {
                preparation,
                expected_revision,
                ..
            }
            | Self::Issue {
                preparation,
                expected_revision,
            }
            | Self::Replace {
                preparation,
                expected_revision,
                ..
            }
            | Self::MarkUnused {
                preparation,
                expected_revision,
            } => Some((*preparation, *expected_revision)),
        }
    }

    #[must_use]
    pub fn selection(&self) -> Option<&DispatchSelection> {
        match self {
            Self::Prepare { lines, .. }
            | Self::Revise { lines, .. }
            | Self::Replace { lines, .. } => Some(lines),
            Self::Issue { .. } | Self::MarkUnused { .. } => None,
        }
    }
}

impl DispatchCommand {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.action {
            DispatchAction::Prepare { .. } => "dispatch_prepare",
            DispatchAction::Revise { .. } => "dispatch_revise",
            DispatchAction::Issue { .. } => "dispatch_issue",
            DispatchAction::Replace { .. } => "dispatch_replace",
            DispatchAction::MarkUnused { .. } => "dispatch_unused",
        }
    }

    pub fn plan(
        &self,
        context: &DispatchContext<'_>,
        identities: DispatchIdentities,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<DispatchPlan, DispatchError> {
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
        {
            return Err(DispatchError::StaleRevision);
        }
        if identities != DispatchIdentities::from_operation_bytes(identities.operation)? {
            return Err(DispatchError::InvalidSelection);
        }
        let stock_revision = next(context.stock_revision)?;
        let stamp = DispatchStamp {
            operation: identities.operation,
            stock_revision,
            actor,
            recorded_at: now,
            observation: self.observation.clone(),
        };
        let mut record = match (&self.action, context.current) {
            (DispatchAction::Prepare { recipient, lines }, None) => DispatchRecord {
                id: identities.preparation,
                hub: context.hub,
                revision: HubRevision::INITIAL,
                created: stamp.clone(),
                updated: stamp.clone(),
                draft: Some(DispatchMaterial::capture(
                    context,
                    *recipient,
                    lines,
                    self.observation.clone(),
                )?),
                versions: Vec::new(),
            },
            (_, Some(current)) => {
                current.validate()?;
                if self.action.target() != Some((current.id, current.revision))
                    || current.hub != context.hub
                {
                    return Err(DispatchError::StaleRevision);
                }
                let mut record = current.clone();
                record.revision = next(current.revision)?;
                record.updated = stamp.clone();
                record
            }
            _ => return Err(DispatchError::InvalidState),
        };
        match &self.action {
            DispatchAction::Prepare { .. } => {}
            DispatchAction::Revise {
                recipient, lines, ..
            } => {
                if record.draft.is_none() {
                    return Err(DispatchError::InvalidState);
                }
                record.draft = Some(DispatchMaterial::capture(
                    context,
                    *recipient,
                    lines,
                    self.observation.clone(),
                )?);
            }
            DispatchAction::Issue { .. } => {
                let material = record.draft.take().ok_or(DispatchError::InvalidState)?;
                let verified = DispatchMaterial::capture(
                    context,
                    material.recipient.id,
                    &material.selection()?,
                    material.observation.clone(),
                )?;
                if !material.same_material(&verified) {
                    return Err(DispatchError::PreviewChanged);
                }
                record.versions.push(IdentificationVersion {
                    id: identities.identification,
                    number: 1,
                    previous: None,
                    material,
                    issued: stamp.clone(),
                    status: IdentificationStatus::Issued,
                    closed: None,
                });
            }
            DispatchAction::Replace {
                recipient, lines, ..
            } => {
                if record.versions.len() >= HUB_IDENTIFICATION_VERSIONS {
                    return Err(DispatchError::Capacity);
                }
                let previous = record
                    .versions
                    .last_mut()
                    .ok_or(DispatchError::InvalidState)?;
                if previous.status != IdentificationStatus::Issued {
                    return Err(DispatchError::InvalidState);
                }
                let material = DispatchMaterial::capture(
                    context,
                    *recipient,
                    lines,
                    self.observation.clone(),
                )?;
                if material.same_material(&previous.material) {
                    return Err(DispatchError::NoMaterialChange);
                }
                previous.status = IdentificationStatus::Superseded;
                previous.closed = Some(stamp.clone());
                let previous_id = previous.id;
                let number = previous.number + 1;
                record.versions.push(IdentificationVersion {
                    id: identities.identification,
                    number,
                    previous: Some(previous_id),
                    material,
                    issued: stamp.clone(),
                    status: IdentificationStatus::Issued,
                    closed: None,
                });
            }
            DispatchAction::MarkUnused { .. } => {
                let issued = record
                    .versions
                    .last_mut()
                    .ok_or(DispatchError::InvalidState)?;
                if issued.status != IdentificationStatus::Issued {
                    return Err(DispatchError::InvalidState);
                }
                issued.status = IdentificationStatus::Unused;
                issued.closed = Some(stamp);
            }
        }
        record.validate()?;
        Ok(DispatchPlan {
            stock_revision,
            record,
        })
    }

    #[must_use]
    pub fn fingerprint(&self, hub: HubId, actor: PublicKey, ids: DispatchIdentities) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.dispatch.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(ids.operation.as_bytes());
        hash.update(ids.preparation.as_bytes());
        hash.update(ids.identification.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash_text(&mut hash, self.kind());
        if let Some((id, revision)) = self.action.target() {
            hash.update(id.as_bytes());
            hash.update(revision.value().to_be_bytes());
        }
        match &self.action {
            DispatchAction::Prepare { recipient, lines }
            | DispatchAction::Revise {
                recipient, lines, ..
            }
            | DispatchAction::Replace {
                recipient, lines, ..
            } => {
                hash.update(recipient.as_bytes());
                hash.update(
                    u32::try_from(lines.lines().len())
                        .expect("bounded selection")
                        .to_be_bytes(),
                );
                for line in lines.lines() {
                    hash.update(line.group.as_bytes());
                    hash.update(line.expected_group_revision.value().to_be_bytes());
                    hash.update(line.packs.value().to_be_bytes());
                }
            }
            DispatchAction::Issue { .. } | DispatchAction::MarkUnused { .. } => {}
        }
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}
