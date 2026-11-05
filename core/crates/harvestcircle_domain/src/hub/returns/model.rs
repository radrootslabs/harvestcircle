use super::*;

/// A manual association claim; the native lookup may establish less than this claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnDepartureClaim {
    pub departure: HubRecordId,
    pub group: Option<HubRecordId>,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnDepartureHeader {
    pub departure: HubRecordId,
    pub operation: HubRecordId,
    pub hub: HubId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub identification: HubRecordId,
    pub recipient: ReferenceRecord,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub occurred: Occurrence,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnSourceSnapshot {
    /// Exact historical departure material, never today's stock or renamed labels.
    pub original: DispatchLineSnapshot,
    /// Native established associations at this new intake's capture, not old permission.
    pub previously_established_returns: WholePacks,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnTiming {
    AfterOrSame,
    Uncertain,
    Contradictory,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedReturnLink {
    OriginalGroupUnknown,
    NoMatchingVariant,
    ClaimedGroupMissing,
    VariantConflict,
    QuantityConflict,
    TimeConflict,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnLinkResolution {
    Unknown,
    Unrecorded,
    Unresolved(UnresolvedReturnLink),
    /// Establishes only the selected original scope, not current source/treatment/time.
    EstablishedScope,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnLinkSnapshot {
    pub claim: Option<ReturnDepartureClaim>,
    pub departure: Option<ReturnDepartureHeader>,
    pub claimed_source: Option<ReturnSourceSnapshot>,
    /// Bounded native candidates. Even a single candidate is not automatic attribution.
    pub plausible_sources: Vec<ReturnSourceSnapshot>,
    pub timing: ReturnTiming,
    pub resolution: ReturnLinkResolution,
}
impl ReturnLinkSnapshot {
    #[must_use]
    pub fn established_group(&self) -> Option<HubRecordId> {
        if self.resolution == ReturnLinkResolution::EstablishedScope {
            self.claim.and_then(|claim| claim.group)
        } else {
            None
        }
    }
}

/// Returned custody facts are intentionally distinct from receipt/opening population
/// constructors. Forward persistence will adopt its own explicit population origin.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnIntake {
    pub population: HubRecordId,
    pub product: ReferenceRecord,
    pub variant: ReferenceRecord,
    pub location: ReferenceRecord,
    pub returned_by: Option<ReferenceRecord>,
    pub source: SourceAttribution,
    pub source_issuer: Option<ReferenceRecord>,
    pub packs: WholePacks,
    pub planned_location: Option<ReferenceRecord>,
    pub occurred: Occurrence,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
    pub observation: SourceProvenance,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnPlan {
    pub identities: ReturnIdentities,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub intake: ReturnIntake,
    pub group: PhysicalStockGroup,
    pub intake_work: Option<WorkRequirement>,
    pub acceptance: AcceptanceStatus,
    pub linkage: ReturnLinkSnapshot,
    pub current_allowances: Option<ReturnAllowanceCapture>,
}

impl ReturnPlan {
    /// The generic population projection preserves this distinct intake origin.
    /// Returning party and old departure linkage remain in the return original;
    /// neither becomes a supplier, producer or inherited treatment/work fact.
    #[must_use]
    pub fn population(&self) -> crate::hub::stock::StockPopulation {
        let intake = &self.intake;
        crate::hub::stock::StockPopulation {
            id: intake.population,
            variant: intake.variant.clone(),
            product: intake.product.clone(),
            supplier: None,
            producer: None,
            source: intake.source.clone(),
            source_issuer: intake.source_issuer.clone(),
            original_packs: intake.packs,
            initial_location: intake.location.clone(),
            origin: crate::hub::stock::PopulationOrigin::Returned {
                returned: self.identities.returned,
            },
            planned_location: intake.planned_location.clone(),
            occurred: intake.occurred,
            recorded_at: intake.recorded_at,
            actor: intake.actor,
            observation: intake.observation.clone(),
        }
    }
}
