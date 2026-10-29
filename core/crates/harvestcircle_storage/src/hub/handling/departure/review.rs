//! Read-only readiness shares the exact new-departure preparation and budget.
use super::*;
use crate::Database;
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::reference::SourceProvenance;

impl Database {
    pub async fn hub_departure_review(
        &self,
        actor: PublicKey,
        request: HubDepartureReviewInput,
        now: UnixTimestamp,
    ) -> Result<HubDepartureReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let context = plan::load_on(tx, hub.id(), request.preparation).await?;
                    let command = DepartureCommand {
                        expected_stock_revision: context.state.revision,
                        expected_catalog_revision: context.catalog.revision(),
                        preparation: request.preparation,
                        expected_preparation_revision: request.expected_preparation_revision,
                        identification: request.identification,
                        occurred: Occurrence::Unknown,
                        observation: SourceProvenance::new(
                            "Read-only current departure context review",
                        )
                        .map_err(|_| corrupt_storage())?,
                    };
                    // These internal plan identities never leave the review or enter a journal.
                    let transient = DurableRequestId::new_v7();
                    let result = plan::prepare_on(
                        tx,
                        hub.id(),
                        actor,
                        identities(&transient)?,
                        &command,
                        &context,
                        now,
                    )
                    .await;
                    let (block, proposed_work_changes) = match result {
                        Ok(plan) => (None, plan.work_changes),
                        Err(error)
                            if matches!(
                                error.code(),
                                SafeErrorCode::HubConflict
                                    | SafeErrorCode::HubCapacity
                                    | SafeErrorCode::HubInvalidInput
                            ) =>
                        {
                            (Some(error), Vec::new())
                        }
                        Err(error) => return Err(error),
                    };
                    let view = HubDispatchView {
                        revision: context.state.revision,
                        catalog_revision: context.catalog.revision(),
                        state: super::super::dispatch::current_state(
                            &context.paper,
                            context.departure.is_some(),
                        )?,
                        departure: context.departure,
                        record: context.paper,
                    };
                    Ok(HubDepartureReview {
                        request,
                        view,
                        block,
                        proposed_work_changes,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
