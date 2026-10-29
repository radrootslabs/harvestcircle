//! A positive count creates separately identified stock with explicitly unknown origin.
use super::*;
use crate::hub::{
    reference::SourceAttribution,
    stock::{PopulationOrigin, StockPopulation},
    treatment::AcceptanceStatus,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountSurplusIdentities {
    pub population: HubRecordId,
    pub group: HubRecordId,
}

impl CountSurplusIdentities {
    pub fn from_operation(operation: HubRecordId) -> Result<Self, CountError> {
        Ok(Self {
            population: derived_id(
                b"harvestcircle.hub.count.surplus.population.identity.v1\0",
                operation,
            )?,
            group: derived_id(
                b"harvestcircle.hub.count.surplus.group.identity.v1\0",
                operation,
            )?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountSurplusPlan {
    pub population: StockPopulation,
    pub group: PhysicalStockGroup,
    pub acceptance: AcceptanceStatus,
}

impl CountPostPlan {
    /// Derives the exact new population from a validated posting original.
    /// Native storage still checks current authority and active label uniqueness.
    pub fn surplus(&self) -> Result<Option<CountSurplusPlan>, CountError> {
        self.validate()?;
        let CountEffect::Surplus {
            packs,
            identification,
            ..
        } = &self.effect
        else {
            return Ok(None);
        };
        let ids = CountSurplusIdentities::from_operation(self.operation)?;
        let prior = [self.id, self.count.id, self.before.id, self.before.trace];
        if ids.population == ids.group
            || prior.contains(&ids.population)
            || prior.contains(&ids.group)
        {
            return Err(CountError::InvalidHistory);
        }
        Ok(Some(CountSurplusPlan {
            population: StockPopulation {
                id: ids.population,
                variant: self.references.variant.clone(),
                product: self.references.product.clone(),
                supplier: None,
                producer: None,
                source: SourceAttribution::Unknown,
                source_issuer: None,
                original_packs: *packs,
                initial_location: self.references.location.clone(),
                origin: PopulationOrigin::CountedSurplus {
                    count: self.count.id,
                    related_population: self.before.trace,
                },
                planned_location: None,
                occurred: Occurrence::Known(self.confirmed_on_hand_at),
                recorded_at: self.recorded_at,
                actor: self.actor,
                observation: self.reason.clone(),
            },
            group: PhysicalStockGroup {
                id: ids.group,
                trace: ids.population,
                variant: self.references.variant.id,
                parent: None,
                identification: Some(identification.clone()),
                location: self.references.location.id,
                packs: *packs,
                revision: HubRevision::INITIAL,
                causal_floor: Some(self.confirmed_on_hand_at),
                work: Vec::new(),
            },
            acceptance: AcceptanceStatus::ReviewRequired,
        }))
    }
}
