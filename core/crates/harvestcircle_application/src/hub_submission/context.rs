//! Captured public semantics are read-only evidence, never an execution permit.
use super::*;
use crate::{DurableRequestId, SessionGeneration};
use harvestcircle_domain::hub::{
    HubRevision,
    counts::CountObservation,
    dispatch::{DispatchMaterial, IdentificationVersion},
    movement::{PhysicalIdentification, PhysicalStockGroup, WorkProgress, WorkRequirement},
    reference::{HubRecordId, ReferenceRecord, WholePacks},
    stock::StockPopulation,
};
use harvestcircle_domain::{PublicKey, UnixTimestamp};

pub const HUB_SUBMISSION_ARCHIVE_SCHEMA: u8 = 1;
pub const HUB_SUBMISSION_ARCHIVE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const HUB_SUBMISSION_MATERIAL_CAPACITY: usize =
    harvestcircle_domain::hub::movement::HUB_WORK_CAPACITY
        + harvestcircle_domain::hub::reference::HUB_REFERENCE_CAPACITY
        + 2 * harvestcircle_domain::hub::portions::HUB_PHYSICAL_SCOPES
        + 4;

/// Native-generated runtime identity. A restored generation alone is insufficient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubRuntimeIncarnation([u8; 16]);
impl HubRuntimeIncarnation {
    pub fn from_bytes(value: [u8; 16]) -> Result<Self, HubRequestArchiveError> {
        if value == [0; 16] {
            return Err(HubRequestArchiveError::InvalidContext);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
impl Serialize for HubRuntimeIncarnation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = [0; 32];
        for (n, byte) in self.0.iter().copied().enumerate() {
            bytes[n * 2] = HEX[usize::from(byte >> 4)];
            bytes[n * 2 + 1] = HEX[usize::from(byte & 15)];
        }
        serializer.serialize_str(std::str::from_utf8(&bytes).expect("hex ASCII"))
    }
}

/// Correlation data only. Native storage must verify the live session separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubSubmissionScope {
    pub runtime: HubRuntimeIncarnation,
    pub generation: SessionGeneration,
    pub actor: PublicKey,
}
impl Serialize for HubSubmissionScope {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Scope {
            runtime: HubRuntimeIncarnation,
            generation: u64,
            actor: PublicKey,
        }
        Scope {
            runtime: self.runtime,
            generation: self.generation.value(),
            actor: self.actor,
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedWorkScope {
    pub work: HubRecordId,
    pub progress: WorkProgress,
    pub packs: WholePacks,
}

/// Physical meaning without duplicating each shared work definition per group.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedGroup {
    pub id: HubRecordId,
    pub trace: HubRecordId,
    pub variant: HubRecordId,
    pub parent: Option<HubRecordId>,
    pub identification: Option<PhysicalIdentification>,
    pub location: HubRecordId,
    pub packs: WholePacks,
    pub revision: HubRevision,
    pub causal_floor: Option<UnixTimestamp>,
    pub work: Vec<HubSubmittedWorkScope>,
}
impl From<&PhysicalStockGroup> for HubSubmittedGroup {
    fn from(group: &PhysicalStockGroup) -> Self {
        let mut work: Vec<_> = group
            .work
            .iter()
            .map(|allocation| HubSubmittedWorkScope {
                work: allocation.requirement.id,
                progress: allocation.progress,
                packs: group.packs,
            })
            .collect();
        work.sort_by_key(|scope| scope.work);
        Self {
            id: group.id,
            trace: group.trace,
            variant: group.variant,
            parent: group.parent,
            identification: group.identification.clone(),
            location: group.location,
            packs: group.packs,
            revision: group.revision,
            causal_floor: group.causal_floor,
            work,
        }
    }
}

/// Only the selected draft/issued material belongs in an intent, not all paper history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HubSubmittedDispatch {
    pub preparation: HubRecordId,
    pub revision: HubRevision,
    pub draft: Option<DispatchMaterial>,
    pub identification: Option<IdentificationVersion>,
}

/// Native adapters capture only referenced/affected records at the admission cut.
/// A record's presence does not establish current authority or safe reconciliation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HubSubmittedMaterial {
    Reference(Box<ReferenceRecord>),
    Group(Box<HubSubmittedGroup>),
    Population(Box<StockPopulation>),
    Work(Box<WorkRequirement>),
    Dispatch(Box<HubSubmittedDispatch>),
    Count(Box<CountObservation>),
    Hold(Box<harvestcircle_domain::hub::treatment::Hold>),
    Discrepancy(Box<harvestcircle_domain::hub::movement::Discrepancy>),
}
impl HubSubmittedMaterial {
    fn identity(&self) -> (u8, HubRecordId, u64) {
        match self {
            Self::Reference(v) => (0, v.id, v.revision.value()),
            Self::Group(v) => (1, v.id, 0),
            Self::Population(v) => (2, v.id, 0),
            Self::Work(v) => (3, v.id, 0),
            Self::Dispatch(v) => (4, v.preparation, 0),
            Self::Count(v) => (5, v.id, 0),
            Self::Hold(v) => (6, v.stamp.id, 0),
            Self::Discrepancy(v) => (7, v.id, 0),
        }
    }
}

/// Captured context is supplied by native reads, never by Kotlin clear/safety flags.
#[derive(Serialize)]
pub struct HubSubmissionContext<'a> {
    pub hub: HubOwnership,
    pub catalog_revision: Option<HubRevision>,
    pub stock_revision: Option<HubRevision>,
    pub material: &'a [HubSubmittedMaterial],
}

impl HubSubmittedRequest<'_> {
    /// Encodes exact intent and resolved material; performs no storage or effects.
    pub fn capture(
        &self,
        operation: &DurableRequestId,
        scope: HubSubmissionScope,
        context: &HubSubmissionContext<'_>,
    ) -> Result<ArchivedHubSubmission, HubRequestArchiveError> {
        // Preserve the independent raw-request bound; a large material allowance
        // must not turn an oversized raw command into an admissible request.
        let input = self.archive()?;
        if context.hub.owner() != scope.actor {
            return Err(HubRequestArchiveError::InvalidContext);
        }
        if context.material.len() > HUB_SUBMISSION_MATERIAL_CAPACITY {
            return Err(HubRequestArchiveError::Capacity);
        }
        let mut material: Vec<_> = context.material.iter().collect();
        material.sort_by_key(|item| item.identity());
        if material
            .windows(2)
            .any(|pair| pair[0].identity() == pair[1].identity())
        {
            return Err(HubRequestArchiveError::InvalidContext);
        }
        #[derive(Serialize)]
        struct Envelope<'a, 'b, 'c> {
            schema: u8,
            kind: &'static str,
            operation: &'a str,
            scope: HubSubmissionScope,
            input: &'a HubSubmittedRequest<'b>,
            hub: HubOwnership,
            catalog_revision: Option<HubRevision>,
            stock_revision: Option<HubRevision>,
            material: Vec<&'c HubSubmittedMaterial>,
        }
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            limit: HUB_SUBMISSION_ARCHIVE_MAX_BYTES,
        };
        serde_json::to_writer(
            &mut writer,
            &Envelope {
                schema: HUB_SUBMISSION_ARCHIVE_SCHEMA,
                kind: self.kind(),
                operation: operation.as_str(),
                scope,
                input: self,
                hub: context.hub,
                catalog_revision: context.catalog_revision,
                stock_revision: context.stock_revision,
                material,
            },
        )
        .map_err(|_| HubRequestArchiveError::Capacity)?;
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.submission.capture.v1\0");
        hash.update(
            u64::try_from(writer.bytes.len())
                .expect("bounded archive")
                .to_be_bytes(),
        );
        hash.update(&writer.bytes);
        Ok(ArchivedHubSubmission {
            kind: self.kind(),
            bytes: writer.bytes,
            digest: hash.finalize().into(),
            input_digest: *input.digest(),
        })
    }
}

/// No decoder or constructor from stored bytes can turn this archive into a command.
#[derive(Clone, Eq, PartialEq)]
pub struct ArchivedHubSubmission {
    kind: &'static str,
    bytes: Vec<u8>,
    digest: [u8; 32],
    input_digest: [u8; 32],
}
impl ArchivedHubSubmission {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    #[must_use]
    pub const fn input_digest(&self) -> &[u8; 32] {
        &self.input_digest
    }
}
impl fmt::Debug for ArchivedHubSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchivedHubSubmission")
            .field("kind", &self.kind)
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
