//! Bounded stored-data inspection; never deserialize an executable domain command.
use super::*;
use crate::{DurableRequestId, SessionGeneration};
use harvestcircle_domain::{
    PublicKey,
    hub::{HubId, HubRevision},
};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubSubmissionArchiveMetadata {
    pub operation: DurableRequestId,
    pub kind: String,
    pub hub: HubOwnership,
    pub scope: HubSubmissionScope,
    pub catalog_revision: Option<HubRevision>,
    pub stock_revision: Option<HubRevision>,
    pub input_digest: [u8; 32],
    pub capture_digest: [u8; 32],
}

/// Checks format, canonical public identifiers and consistency without granting
/// permission or turning raw input/material into a mutation command.
pub fn inspect_hub_submission(
    bytes: &[u8],
) -> Result<HubSubmissionArchiveMetadata, HubRequestArchiveError> {
    if bytes.is_empty() || bytes.len() > HUB_SUBMISSION_ARCHIVE_MAX_BYTES {
        return Err(HubRequestArchiveError::Capacity);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Scope {
        runtime: String,
        generation: u64,
        actor: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Hub {
        id: String,
        owner: String,
        revision: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope<'a> {
        schema: u8,
        kind: String,
        operation: String,
        scope: Scope,
        hub: Hub,
        catalog_revision: Option<u64>,
        stock_revision: Option<u64>,
        #[serde(borrow)]
        input: &'a RawValue,
        #[serde(borrow)]
        material: &'a RawValue,
    }
    let invalid = || HubRequestArchiveError::InvalidContext;
    let value: Envelope<'_> = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value.schema != HUB_SUBMISSION_ARCHIVE_SCHEMA
        || !known_kind(&value.kind)
        || !value.material.get().starts_with('[')
    {
        return Err(invalid());
    }
    let hexadecimal = |value: &str| -> Result<[u8; 16], HubRequestArchiveError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        Ok(u128::from_str_radix(value, 16)
            .map_err(|_| invalid())?
            .to_be_bytes())
    };
    let public_key = |text: &str| {
        let key = PublicKey::from_hex(text).map_err(|_| invalid())?;
        if key.to_hex() != text {
            return Err(invalid());
        }
        Ok(key)
    };
    let hub = HubOwnership::restore(
        HubId::from_bytes(hexadecimal(&value.hub.id)?).map_err(|_| invalid())?,
        public_key(&value.hub.owner)?,
        HubRevision::from_value(value.hub.revision).map_err(|_| invalid())?,
    );
    let scope = HubSubmissionScope {
        runtime: HubRuntimeIncarnation::from_bytes(hexadecimal(&value.scope.runtime)?)?,
        generation: SessionGeneration::from_value(value.scope.generation),
        actor: public_key(&value.scope.actor)?,
    };
    if hub.owner() != scope.actor {
        return Err(invalid());
    }
    #[derive(Serialize)]
    struct RawInput<'a> {
        schema: u8,
        kind: &'a str,
        input: &'a RawValue,
    }
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit: HUB_SUBMITTED_REQUEST_MAX_BYTES,
    };
    serde_json::to_writer(
        &mut writer,
        &RawInput {
            schema: HUB_SUBMITTED_REQUEST_SCHEMA,
            kind: &value.kind,
            input: value.input,
        },
    )
    .map_err(|_| HubRequestArchiveError::Capacity)?;
    let input_digest = archive_digest(&writer.bytes);
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.submission.capture.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .expect("bounded archive")
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(HubSubmissionArchiveMetadata {
        operation: DurableRequestId::parse(value.operation).map_err(|_| invalid())?,
        kind: value.kind,
        hub,
        scope,
        catalog_revision: value
            .catalog_revision
            .map(HubRevision::from_value)
            .transpose()
            .map_err(|_| invalid())?,
        stock_revision: value
            .stock_revision
            .map(HubRevision::from_value)
            .transpose()
            .map_err(|_| invalid())?,
        input_digest,
        capture_digest: hash.finalize().into(),
    })
}

#[must_use]
pub fn known_hub_submission_kind(kind: &str) -> bool {
    known_kind(kind)
}
fn known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "initialize"
            | "reference"
            | "receipt"
            | "opening"
            | "move"
            | "split"
            | "acceptance"
            | "hold_place"
            | "hold_release"
            | "work_new"
            | "work_replan"
            | "discrepancy_open"
            | "discrepancy_resolve"
            | "dispatch_prepare"
            | "dispatch_revise"
            | "dispatch_issue"
            | "dispatch_replace"
            | "dispatch_unused"
            | "departure"
            | "count_observe"
            | "count_post"
    )
}
