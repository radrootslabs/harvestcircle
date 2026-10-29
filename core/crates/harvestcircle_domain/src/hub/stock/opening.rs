//! Present counted stock with explicitly bounded earlier-history knowledge.
use super::*;
use crate::hub::movement::{PhysicalIdentification, PhysicalStockGroup};
use crate::hub::treatment::AcceptanceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpeningError {
    Reference(HubReferenceError),
    Stock(StockError),
    StaleRevision,
    EmptyQuantity,
    CountTimeRequired,
    InvalidIdentity,
    RevisionExhausted,
}
impl From<HubReferenceError> for OpeningError {
    fn from(value: HubReferenceError) -> Self {
        Self::Reference(value)
    }
}
impl From<StockError> for OpeningError {
    fn from(value: StockError) -> Self {
        Self::Stock(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpeningIdentities {
    pub population: HubRecordId,
    pub group: HubRecordId,
    pub source: HubRecordId,
}
impl OpeningIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, OpeningError> {
        let derive = |purpose: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(b"harvestcircle.hub.counted-opening.identity.v1\0");
            hash.update(purpose);
            hash.update(operation.as_bytes());
            let digest = hash.finalize();
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&digest[..16]);
            HubRecordId::from_bytes(bytes).map_err(OpeningError::from)
        };
        Ok(Self {
            population: derive(b"population\0")?,
            group: derive(b"group\0")?,
            source: derive(b"source\0")?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OpeningCommand {
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub variant: HubRecordId,
    pub location: HubRecordId,
    pub packs: WholePacks,
    pub identification: PhysicalIdentification,
    pub source: SourceAttribution,
    pub history: OpeningHistory,
    /// Current count timing is distinct from explicitly unknown earlier intake history.
    pub counted: Occurrence,
    pub observation: SourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpeningPlan {
    pub stock_revision: HubRevision,
    pub population: StockPopulation,
    pub group: PhysicalStockGroup,
    pub acceptance: AcceptanceStatus,
}
impl OpeningCommand {
    pub fn plan(
        &self,
        catalog: &ReferenceCatalog,
        stock_revision: HubRevision,
        identities: OpeningIdentities,
        actor: PublicKey,
        recorded_at: UnixTimestamp,
    ) -> Result<OpeningPlan, OpeningError> {
        if self.expected_stock_revision != stock_revision
            || self.expected_catalog_revision != catalog.revision()
        {
            return Err(OpeningError::StaleRevision);
        }
        if self.packs.value() == 0 {
            return Err(OpeningError::EmptyQuantity);
        }
        self.counted.validate(recorded_at)?;
        let Occurrence::Known(counted_at) = self.counted else {
            return Err(OpeningError::CountTimeRequired);
        };
        if identities.population == identities.group
            || identities.population == identities.source
            || identities.group == identities.source
        {
            return Err(OpeningError::InvalidIdentity);
        }
        let variant = catalog
            .require_kind(self.variant, ReferenceKind::Pack)?
            .clone();
        let ReferenceValue::Pack { product, .. } = variant.value else {
            return Err(OpeningError::InvalidIdentity);
        };
        let product = catalog
            .require_kind(product, ReferenceKind::Product)?
            .clone();
        let location = catalog
            .require_kind(self.location, ReferenceKind::Location)?
            .clone();
        let source_issuer = match &self.source {
            SourceAttribution::Unknown => None,
            SourceAttribution::Referenced { source, .. } => {
                source.validate(catalog)?;
                source
                    .issuer
                    .map(|id| catalog.require_kind(id, ReferenceKind::Party).cloned())
                    .transpose()?
            }
        };
        let stock_revision = stock_revision
            .next()
            .map_err(|_| OpeningError::RevisionExhausted)?;
        Ok(OpeningPlan {
            stock_revision,
            population: StockPopulation {
                id: identities.population,
                variant,
                product,
                supplier: None,
                producer: None,
                source: self.source.clone(),
                source_issuer,
                original_packs: self.packs,
                initial_location: location,
                origin: PopulationOrigin::CountedOpening {
                    history: self.history.clone(),
                },
                planned_location: None,
                occurred: self.counted,
                recorded_at,
                actor,
                observation: self.observation.clone(),
            },
            group: PhysicalStockGroup {
                id: identities.group,
                trace: identities.population,
                variant: self.variant,
                parent: None,
                identification: Some(self.identification.clone()),
                location: self.location,
                packs: self.packs,
                revision: HubRevision::INITIAL,
                causal_floor: Some(counted_at),
                work: Vec::new(),
            },
            acceptance: AcceptanceStatus::ReviewRequired,
        })
    }

    #[must_use]
    pub fn fingerprint(
        &self,
        hub: HubId,
        actor: PublicKey,
        identities: OpeningIdentities,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.counted-opening.command.v1\0");
        hash.update(hub.as_bytes());
        hash.update(actor.as_bytes());
        hash.update(identities.population.as_bytes());
        hash.update(identities.group.as_bytes());
        hash.update(identities.source.as_bytes());
        hash.update(self.expected_stock_revision.value().to_be_bytes());
        hash.update(self.expected_catalog_revision.value().to_be_bytes());
        hash.update(self.variant.as_bytes());
        hash.update(self.location.as_bytes());
        hash.update(self.packs.value().to_be_bytes());
        hash_text(&mut hash, self.identification.label.as_str());
        hash_text(&mut hash, self.identification.observation.as_str());
        hash_source(&mut hash, &self.source);
        match &self.history {
            OpeningHistory::Unknown => hash.update([0]),
            OpeningHistory::ExternalReference(value) => {
                hash.update([1]);
                hash_text(&mut hash, value.as_str());
            }
        }
        crate::hub::movement::hash_occurrence(&mut hash, self.counted);
        hash_text(&mut hash, self.observation.as_str());
        hash.finalize().into()
    }
}

#[cfg(test)]
mod tests;
