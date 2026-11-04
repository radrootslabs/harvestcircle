//! Current comparison of one named recorded found observation. It creates no
//! admission, operation, group or future execution permission.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        CorrectionObservedFacts, FoundStockCommand, FoundStockContext, FoundStockPlan,
        PhysicalCorrectionError,
    },
    movement::{HUB_GROUP_CAPACITY, PhysicalIdentification},
    reference::{HubLabel, SourceProvenance},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubFoundCorrectionReviewQuery {
    pub observation_operation: DurableRequestId,
    pub proposed_operation: DurableRequestId,
    pub expected_stock_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubFoundCorrectionReviewBlocker {
    ObservationNoLongerCurrent,
    OperationAlreadyUsed,
    TargetNotQualified,
    Policy(PhysicalCorrectionError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubFoundCorrectionReviewOutcome {
    Candidate(Box<FoundStockPlan>),
    Blocked(HubFoundCorrectionReviewBlocker),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubFoundCorrectionReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub observation: HubCorrectionObservationReceipt,
    pub proposed_operation: DurableRequestId,
    pub outcome: HubFoundCorrectionReviewOutcome,
}

impl Database {
    /// Recomputes current evidence for one exact recorded observation. Even a
    /// candidate is read-only data; posting must reacquire all live native facts.
    pub async fn hub_found_correction_review(
        &self,
        actor: PublicKey,
        query: HubFoundCorrectionReviewQuery,
        now: UnixTimestamp,
    ) -> Result<HubFoundCorrectionReview, SafeError> {
        if query.expected_stock_revision.is_some() != query.expected_catalog_revision.is_some() {
            return Err(stock::invalid_input());
        }
        self.host().transaction(|tx|Box::pin(async move {
            let hub = reference::authorize_on(tx, actor).await?;
            reference::validate_basis_on(tx, hub.id()).await?;
            let state = stock::read::state_on(tx, hub.id()).await?;
            let catalog = reference::load_catalog_on(tx, hub.id()).await?;
            if query.expected_stock_revision.is_some_and(|r| r != state.revision)
                || query.expected_catalog_revision.is_some_and(|r| r != catalog.revision()) {
                return Err(stock::conflict());
            }
            let observed = stock::read::operation_on(tx, hub.id(), &query.observation_operation).await?.ok_or_else(stock::invalid_input)?;
            let observation = super::original_on(tx, hub.id(), observed).await?;
            let CorrectionObservedFacts::Found {target_operation, target_group, allocations, basis} = observation.original.observation.facts.clone() else {return Err(stock::invalid_input());};
            let mut review = HubFoundCorrectionReview {
                revision:state.revision,catalog_revision:catalog.revision(),observation,
                proposed_operation:query.proposed_operation.clone(),
                outcome:HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::TargetNotQualified),
            };
            let current:i64=sqlx::query_scalar(concat!(
                "SELECT NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=?) ",
                "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=?)"
            )).bind(query.observation_operation.as_str()).bind(query.observation_operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if current != 1 {
                review.outcome=HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::ObservationNoLongerCurrent);return Ok(review);
            }
            let used:i64=sqlx::query_scalar(concat!(
                "SELECT EXISTS(SELECT 1 FROM hub_submissions WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_stock_operations WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_reference_versions WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_history WHERE operation_id=?)"
            )).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if used != 0 {
                review.outcome=HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::OperationAlreadyUsed);return Ok(review);
            }
            // Observed identifiers are untrusted facts. A non-operation UUID is
            // an unqualified target, not damage to its canonical retained fact.
            let target = match crate::hub::actuals::common::operation(target_operation) {
                Ok(target) => target,
                Err(_) => return Ok(review),
            };
            let funding = match super::target_funding::load_on(tx,hub.id(),&target,target_group,state.revision).await {
                Ok(value)=>value,
                Err(error) if error.code()==harvestcircle_domain::SafeErrorCode::HubInvalidInput=>return Ok(review),
                Err(error)=>return Err(error),
            };
            let trace=funding.quantity.original().trace;
            let lineage=handling::lineage::load_on(tx,hub.id(),trace,&catalog,state.revision).await?;
            let treatment=handling::treatment::read::state_on(tx,hub.id(),&lineage.graph).await?;
            let (groups,identifications)=identifications_on(tx).await?;
            if groups.checked_add(allocations.len()).is_none_or(|count|count>HUB_GROUP_CAPACITY) {
                review.outcome=HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::Policy(PhysicalCorrectionError::Capacity));return Ok(review);
            }
            let latest:i64=sqlx::query_scalar("SELECT COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=?").bind(hub.id().as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            let latest=UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?;
            let uncertain=crate::hub::submission::uncertainty_all_on(tx).await?;
            let actual:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE review_operation_id IS NULL").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            let open=super::reservations_on(tx).await?;
            if open.operations==0 || actual<0 {return Err(corrupt_storage());}
            // Only this canonical current unused observation is being reviewed.
            // Every pending intent, other observation and unresolved report stays.
            let blocked=uncertain.uncertain_intent || actual!=0 || open.operations>1;
            let command=FoundStockCommand {target_operation,target_group,expected_stock_revision:state.revision,expected_catalog_revision:catalog.revision(),allocations,basis};
            let context=FoundStockContext {family:funding.family,hub:hub.id(),stock_revision:state.revision,catalog:&catalog,latest_recorded_at:latest,target:&funding.quantity,funding:&funding.debits,lineage:&lineage.graph,treatment:&treatment,active_identifications:&identifications,uncertain_submissions:blocked};
            let operation=crate::hub::actuals::common::operation_id(&query.proposed_operation).map_err(|_|stock::invalid_input())?;
            review.outcome=match command.plan(&context,operation,actor,now) {
                Ok(plan)=>HubFoundCorrectionReviewOutcome::Candidate(Box::new(plan)),
                Err(error)=>HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::Policy(error)),
            };
            Ok(review)
        })).await.map_err(map_transaction_error)
    }
}

async fn identifications_on(
    tx: &mut ServiceSqliteTransaction<'_>,
) -> Result<(usize, Vec<PhysicalIdentification>), SafeError> {
    let groups: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    let groups = usize::try_from(groups).map_err(|_| corrupt_storage())?;
    if groups > HUB_GROUP_CAPACITY {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query(concat!(
        "SELECT substr(CAST(i.physical_label AS BLOB),1,121) AS label,substr(CAST(i.observation AS BLOB),1,513) AS observation ",
        "FROM hub_group_identification i WHERE i.physical_label IS NOT NULL AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0) ORDER BY i.group_id LIMIT ?"
    )).bind(i64::try_from(HUB_GROUP_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > groups {
        return Err(corrupt_storage());
    }
    let result = rows
        .into_iter()
        .map(|row| {
            Ok(PhysicalIdentification {
                label: HubLabel::new(stock::read::text(&row, "label")?)
                    .map_err(|_| corrupt_storage())?,
                observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
                    .map_err(|_| corrupt_storage())?,
            })
        })
        .collect::<Result<Vec<_>, SafeError>>()?;
    Ok((groups, result))
}
