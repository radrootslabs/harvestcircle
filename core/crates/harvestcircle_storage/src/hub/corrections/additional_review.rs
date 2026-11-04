//! Current native comparison: preserved factual total and distinct physical delta.
//! This read closes no report, admits no input and changes no stock or work.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{
        ActualPhysicalActivity, ActualPhysicalBirth, ActualReportRecord, ActualSourceContext,
        HistoricalActualSource, HistoricalHoldScopeAssessment, HistoricalHoldScopeContext,
    },
    corrections::{
        AdditionalQuantityCommand, AdditionalQuantityContext, AdditionalQuantityPlan,
        QuantityEffectError,
    },
    movement::PhysicalStockGroup,
    portions::PhysicalLineage,
    reference::ReferenceCatalog,
    treatment::TreatmentState,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubAdditionalQuantityReviewOutcome {
    Candidate(Box<AdditionalQuantityPlan>),
    OperationAlreadyUsed,
    Blocked(QuantityEffectError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubAdditionalQuantityReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub report: ActualReportRecord,
    pub proposed_operation: DurableRequestId,
    pub outcome: HubAdditionalQuantityReviewOutcome,
    pub historical_holds: Vec<HistoricalHoldScopeAssessment>,
}

pub(super) struct Context {
    pub hub: HubId,
    pub revision: HubRevision,
    pub catalog: ReferenceCatalog,
    pub report: ActualReportRecord,
    pub prior_report_review: Option<HubRecordId>,
    pub known: Vec<super::known::HubCorrectionKnownDeparture>,
    pub target_event: usize,
    pub target_line: usize,
    pub group: PhysicalStockGroup,
    pub lineage: PhysicalLineage,
    pub births: Vec<ActualPhysicalBirth>,
    pub treatment: TreatmentState,
    pub source: HistoricalActualSource,
    pub activity: Vec<ActualPhysicalActivity>,
    pub latest_recorded_at: UnixTimestamp,
    pub uncertain_submissions: bool,
}
impl Context {
    pub fn domain(&self) -> AdditionalQuantityContext<'_> {
        let target = &self.known[self.target_event];
        AdditionalQuantityContext {
            hub: self.hub,
            stock_revision: self.revision,
            latest_recorded_at: self.latest_recorded_at,
            catalog: &self.catalog,
            family: target.recipient_original.family,
            original: &target.original,
            target: &target.quantities[self.target_line],
            recipient_original: &target.recipient_original,
            recipient_history: &target.recipient_history,
            report: &self.report,
            prior_report_review: self.prior_report_review,
            historical_source: ActualSourceContext {
                source: &self.source,
                lineage: &self.lineage,
                balancing_group: &self.group,
                subsequent_activity: &self.activity,
            },
            treatment: &self.treatment,
            known_departures: self.known.iter().map(|v| v.as_domain()).collect(),
            uncertain_submissions: self.uncertain_submissions,
        }
    }

    pub fn material(
        &self,
        id: HubRecordId,
    ) -> harvestcircle_application::hub_submission::HubSubmittedAdditionalQuantity {
        use harvestcircle_application::hub_submission::{
            HubSubmittedAdditionalQuantity, HubSubmittedCorrectionKnown,
        };
        HubSubmittedAdditionalQuantity {
            id,
            report: self.report.clone(),
            group: self.group.clone(),
            lineage: self.lineage.clone(),
            births: self.births.clone(),
            treatment: self.treatment.clone(),
            source: self.source.clone(),
            activity: self.activity.clone(),
            known_departures: self
                .known
                .iter()
                .map(|known| HubSubmittedCorrectionKnown {
                    original: known.original.clone(),
                    recipient_original: known.recipient_original.clone(),
                    recipient_history: known.recipient_history.clone(),
                    quantities: known.quantities.clone(),
                    funding: known.funding.clone(),
                })
                .collect(),
            latest_recorded_at: self.latest_recorded_at,
            uncertain_submissions: self.uncertain_submissions,
        }
    }
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    command: &AdditionalQuantityCommand,
    except_pending: Option<&DurableRequestId>,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if state.revision != command.facts.expected_stock_revision
        || catalog.revision() != command.facts.expected_catalog_revision
    {
        return Err(stock::conflict());
    }
    let report = crate::hub::actuals::reports::by_id_on(tx, hub, command.facts.report)
        .await?
        .original;
    let prior: Option<String> = sqlx::query_scalar(
        "SELECT substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=?",
    )
    .bind(report.id.as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let prior_report_review = prior
        .map(|id| {
            let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
            crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())
        })
        .transpose()?;
    let group = handling::read::group_on(tx, hub, command.balancing_group, &catalog).await?;
    let loaded = handling::lineage::load_on(tx, hub, group.trace, &catalog, state.revision).await?;
    let known = Box::pin(super::known::load_on(tx, hub, group.trace, state.revision)).await?;
    let target_event = known
        .iter()
        .position(|v| v.original.operation == command.facts.target_operation)
        .ok_or_else(stock::invalid_input)?;
    let target_line = known[target_event]
        .quantities
        .iter()
        .position(|q| q.original().group == command.facts.target_group)
        .ok_or_else(stock::invalid_input)?;
    let births = handling::lineage::actual_births_on(tx, hub, &loaded).await?;
    let source = handling::actuals::source_on(
        tx,
        hub,
        &loaded.graph,
        &loaded.trace,
        &crate::hub::actuals::common::operation(command.source_operation)?,
        command.source_group,
    )
    .await?;
    let activity = handling::actuals::activity_on(tx, hub, &loaded.graph, &source).await?;
    let treatment = handling::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    let latest: i64 = sqlx::query_scalar(
        "SELECT max(recorded_at_unix_s) FROM hub_stock_operations WHERE hub_id=?",
    )
    .bind(hub.as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    // The selected report is factual input, never a pending-admission exception.
    // Read/capture callers supply None. Only the owned fresh executor may
    // exclude its exact already-admitted operation while consuming that grant.
    let uncertain_submissions = match except_pending {
        None => crate::hub::submission::uncertainty_all_on(tx).await?,
        Some(own) => crate::hub::submission::uncertainty_on(tx, own).await?,
    }
    .uncertain_intent;
    Ok(Context {
        hub,
        revision: state.revision,
        catalog,
        report,
        prior_report_review,
        known,
        target_event,
        target_line,
        group,
        lineage: loaded.graph,
        births,
        treatment,
        source,
        activity,
        latest_recorded_at: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        uncertain_submissions,
    })
}

pub(super) fn error(error: QuantityEffectError) -> SafeError {
    use harvestcircle_domain::hub::corrections::CorrectionError;
    match error {
        QuantityEffectError::InvalidHistory => corrupt_storage(),
        QuantityEffectError::InvalidFacts | QuantityEffectError::InvalidScope => {
            stock::invalid_input()
        }
        QuantityEffectError::StaleRevision
        | QuantityEffectError::UncertainSubmission
        | QuantityEffectError::AlreadyReviewed
        | QuantityEffectError::UnsupportedFamily => stock::conflict(),
        QuantityEffectError::Actual(error) => crate::hub::actuals::common::error(error),
        QuantityEffectError::Accounting(CorrectionError::Capacity) => stock::capacity(),
        QuantityEffectError::Accounting(
            CorrectionError::InvalidHistory | CorrectionError::Accounting,
        ) => corrupt_storage(),
        QuantityEffectError::Accounting(_) => stock::conflict(),
    }
}

impl Database {
    /// A candidate describes current evidence; posting must independently acquire
    /// fresh native admission and recheck all current facts in its own transaction.
    pub async fn hub_additional_quantity_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: AdditionalQuantityCommand,
        now: UnixTimestamp,
    ) -> Result<HubAdditionalQuantityReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let context = Box::pin(load_on(tx, hub.id(), &command, None)).await?;
                    let mut review = HubAdditionalQuantityReview {
                        revision: context.revision,
                        catalog_revision: context.catalog.revision(),
                        report: context.report.clone(),
                        proposed_operation: operation.clone(),
                        outcome: HubAdditionalQuantityReviewOutcome::OperationAlreadyUsed,
                        historical_holds: Vec::new(),
                    };
                    if crate::hub::submission::operation_exists_on(tx, &operation).await? {
                        return Ok(review);
                    }
                    let own = crate::hub::actuals::common::operation_id(&operation)?;
                    review.outcome = match command.plan(&context.domain(), own, actor, now) {
                        Ok(plan) => {
                            super::quantity::ensure_capacity_on(
                                tx,
                                hub.id(),
                                plan.original_line.trace,
                            )
                            .await?;
                            review.historical_holds = context
                                .treatment
                                .holds()
                                .iter()
                                .map(|hold| {
                                    HistoricalHoldScopeContext {
                                        lineage: &context.lineage,
                                        group: context.group.id,
                                        occurred: command.facts.occurred,
                                        births: &context.births,
                                        now,
                                    }
                                    .verify(hold)
                                })
                                .collect::<Result<Vec<_>, _>>()
                                .map_err(crate::hub::actuals::common::error)?;
                            HubAdditionalQuantityReviewOutcome::Candidate(Box::new(plan))
                        }
                        Err(error) => HubAdditionalQuantityReviewOutcome::Blocked(error),
                    };
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

#[cfg(test)]
pub(super) mod tests;
