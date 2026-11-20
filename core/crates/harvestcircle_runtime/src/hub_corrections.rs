//! Typed correction ports retain native custody, admission and original recovery.
use super::*;
use harvestcircle_domain::hub::corrections::{
    AdditionalQuantityCommand, CorrectionObservationCommand, CountClassificationCommand,
    CountExplanationCommand, CountReturnAssociationCommand, FoundCorrectionCommand,
    RecipientAttributionCommand, SourceAttributionCommand,
};
use harvestcircle_storage::{
    HubAdditionalQuantityReceipt, HubAdditionalQuantityReview, HubCorrectionFundingEvidence,
    HubCorrectionFundingQuery, HubCorrectionKnownEvidence, HubCorrectionKnownQuery,
    HubCorrectionObservationReceipt, HubCountClassificationReceipt, HubCountClassificationReview,
    HubCountExplanationReceipt, HubCountExplanationReview, HubCountReturnAssociationReceipt,
    HubCountReturnAssociationReview, HubFoundCorrectionReceipt, HubFoundCorrectionReview,
    HubFoundCorrectionReviewQuery, HubRecipientAttributionReceipt, HubRecipientAttributionReview,
    HubSourceAttributionReceipt, HubSourceAttributionReview,
};

pub(super) enum HubCorrectionWrite {
    Observe(Box<CorrectionObservationCommand>),
    Found(Box<FoundCorrectionCommand>),
    Additional(Box<AdditionalQuantityCommand>),
    Source(Box<SourceAttributionCommand>),
    Recipient(Box<RecipientAttributionCommand>),
    Explain(Box<CountExplanationCommand>),
    Classify(Box<CountClassificationCommand>),
    AssociateReturn(Box<CountReturnAssociationCommand>),
}
impl HubCorrectionWrite {
    fn mutation(self) -> HubMutation {
        match self {
            Self::Observe(command) => HubMutation::CorrectionObservation(command),
            Self::Found(command) => HubMutation::FoundCorrection(command),
            Self::Additional(command) => HubMutation::AdditionalQuantity(command),
            Self::Source(command) => HubMutation::SourceAttribution(command),
            Self::Recipient(command) => HubMutation::RecipientAttribution(command),
            Self::Explain(command) => HubMutation::CountExplanation(command),
            Self::Classify(command) => HubMutation::CountClassification(command),
            Self::AssociateReturn(command) => HubMutation::CountReturnAssociation(command),
        }
    }
}
pub(super) enum HubCorrectionOriginal {
    Observation,
    Found,
    Additional,
    Source,
    Recipient,
    Explanation,
    Classification,
    ReturnAssociation,
}
pub(super) enum HubCorrectionCommand {
    Read(HubCorrectionRead),
    Write {
        operation: DurableRequestId,
        command: HubCorrectionWrite,
    },
    Original {
        operation: DurableRequestId,
        kind: HubCorrectionOriginal,
    },
}
pub(super) enum HubCorrectionRead {
    Found(HubFoundCorrectionReviewQuery),
    Additional {
        operation: DurableRequestId,
        command: Box<AdditionalQuantityCommand>,
    },
    Source {
        operation: DurableRequestId,
        command: Box<SourceAttributionCommand>,
    },
    Recipient {
        operation: DurableRequestId,
        command: Box<RecipientAttributionCommand>,
    },
    Explanation {
        operation: DurableRequestId,
        command: Box<CountExplanationCommand>,
    },
    Classification {
        operation: DurableRequestId,
        command: Box<CountClassificationCommand>,
    },
    ReturnAssociation {
        operation: DurableRequestId,
        command: Box<CountReturnAssociationCommand>,
    },
    Known(HubCorrectionKnownQuery),
    Funding(HubCorrectionFundingQuery),
}
pub(super) enum HubCorrectionReadValue {
    Found(Box<HubFoundCorrectionReview>),
    Additional(Box<HubAdditionalQuantityReview>),
    Source(Box<HubSourceAttributionReview>),
    Recipient(Box<HubRecipientAttributionReview>),
    Explanation(Box<HubCountExplanationReview>),
    Classification(Box<HubCountClassificationReview>),
    ReturnAssociation(Box<HubCountReturnAssociationReview>),
    Known(Box<HubCorrectionKnownEvidence>),
    Funding(Box<HubCorrectionFundingEvidence>),
}
impl HubCorrectionCommand {
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(self, Self::Write { .. })
    }
    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}

// Only the closed typed variants above can enter this route. No arbitrary
// HubMutation, serialized request or saved Pending value is a runtime command.
macro_rules! correction_write_port {
    ($method:ident, $command:ty, $write:ident, $result:ident, $receipt:ty) => {
        impl RuntimeActorHandle {
            pub async fn $method(
                &self,
                operation: DurableRequestId,
                command: $command,
                expected: SnapshotRevision,
                timeout: Duration,
            ) -> Result<$receipt, SafeError> {
                match self
                    .dispatch_durable(
                        RuntimeCommand::HubCorrection {
                            generation: self.session_generation(),
                            command: HubCorrectionCommand::Write {
                                operation,
                                command: HubCorrectionWrite::$write(Box::new(command)),
                            },
                        },
                        expected,
                        timeout,
                    )
                    .await?
                {
                    RuntimeCommandValue::HubCorrection(Some(HubMutationResult::$result(value))) => {
                        Ok(*value)
                    }
                    _ => Err(hub_authority::mutation_error(invalid_actor_response())),
                }
            }
        }
    };
}
correction_write_port!(
    observe_hub_correction,
    CorrectionObservationCommand,
    Observe,
    CorrectionObserved,
    HubCorrectionObservationReceipt
);
correction_write_port!(
    post_hub_found_correction,
    FoundCorrectionCommand,
    Found,
    FoundCorrected,
    HubFoundCorrectionReceipt
);
correction_write_port!(
    post_hub_additional_quantity,
    AdditionalQuantityCommand,
    Additional,
    AdditionalCorrected,
    HubAdditionalQuantityReceipt
);
correction_write_port!(
    post_hub_source_attribution,
    SourceAttributionCommand,
    Source,
    SourceAttributed,
    HubSourceAttributionReceipt
);
correction_write_port!(
    post_hub_recipient_attribution,
    RecipientAttributionCommand,
    Recipient,
    RecipientAttributed,
    HubRecipientAttributionReceipt
);
correction_write_port!(
    explain_hub_count_loss,
    CountExplanationCommand,
    Explain,
    CountExplained,
    HubCountExplanationReceipt
);
correction_write_port!(
    classify_hub_count_loss,
    CountClassificationCommand,
    Classify,
    CountClassified,
    HubCountClassificationReceipt
);
correction_write_port!(
    associate_hub_count_return,
    CountReturnAssociationCommand,
    AssociateReturn,
    CountReturnAssociated,
    HubCountReturnAssociationReceipt
);

macro_rules! correction_original_port {
    ($method:ident, $kind:ident, $result:ident, $receipt:ty) => {
        impl RuntimeActorHandle {
            pub async fn $method(
                &self,
                operation: DurableRequestId,
            ) -> Result<Option<$receipt>, SafeError> {
                match self
                    .dispatch_correction_original(operation, HubCorrectionOriginal::$kind)
                    .await?
                {
                    None => Ok(None),
                    Some(HubMutationResult::$result(value)) => Ok(Some(*value)),
                    _ => Err(invalid_actor_response()),
                }
            }
        }
    };
}
correction_original_port!(
    hub_found_correction_original,
    Found,
    FoundCorrected,
    HubFoundCorrectionReceipt
);
correction_original_port!(
    hub_additional_quantity_original,
    Additional,
    AdditionalCorrected,
    HubAdditionalQuantityReceipt
);
correction_original_port!(
    hub_source_attribution_original,
    Source,
    SourceAttributed,
    HubSourceAttributionReceipt
);
correction_original_port!(
    hub_recipient_attribution_original,
    Recipient,
    RecipientAttributed,
    HubRecipientAttributionReceipt
);
correction_original_port!(
    hub_count_explanation_operation,
    Explanation,
    CountExplained,
    HubCountExplanationReceipt
);
correction_original_port!(
    hub_count_classification_operation,
    Classification,
    CountClassified,
    HubCountClassificationReceipt
);
correction_original_port!(
    hub_count_return_association_operation,
    ReturnAssociation,
    CountReturnAssociated,
    HubCountReturnAssociationReceipt
);

macro_rules! correction_review_port {
    ($method:ident, $command:ty, $variant:ident, $value:ty) => {
        impl RuntimeActorHandle {
            pub async fn $method(
                &self,
                operation: DurableRequestId,
                command: $command,
            ) -> Result<$value, SafeError> {
                match self
                    .dispatch_correction_read(HubCorrectionRead::$variant {
                        operation,
                        command: Box::new(command),
                    })
                    .await?
                {
                    HubCorrectionReadValue::$variant(value) => Ok(*value),
                    _ => Err(invalid_actor_response()),
                }
            }
        }
    };
}
correction_review_port!(
    hub_additional_quantity_review,
    AdditionalQuantityCommand,
    Additional,
    HubAdditionalQuantityReview
);
correction_review_port!(
    hub_source_attribution_review,
    SourceAttributionCommand,
    Source,
    HubSourceAttributionReview
);
correction_review_port!(
    hub_recipient_attribution_review,
    RecipientAttributionCommand,
    Recipient,
    HubRecipientAttributionReview
);
correction_review_port!(
    hub_count_explanation_review,
    CountExplanationCommand,
    Explanation,
    HubCountExplanationReview
);
correction_review_port!(
    hub_count_classification_review,
    CountClassificationCommand,
    Classification,
    HubCountClassificationReview
);
correction_review_port!(
    hub_count_return_association_review,
    CountReturnAssociationCommand,
    ReturnAssociation,
    HubCountReturnAssociationReview
);

macro_rules! correction_query_port {
    ($method:ident, $query:ty, $variant:ident, $value:ty) => {
        impl RuntimeActorHandle {
            pub async fn $method(&self, query: $query) -> Result<$value, SafeError> {
                match self
                    .dispatch_correction_read(HubCorrectionRead::$variant(query))
                    .await?
                {
                    HubCorrectionReadValue::$variant(value) => Ok(*value),
                    _ => Err(invalid_actor_response()),
                }
            }
        }
    };
}
correction_query_port!(
    hub_found_correction_review,
    HubFoundCorrectionReviewQuery,
    Found,
    HubFoundCorrectionReview
);
correction_query_port!(
    hub_correction_known_departures,
    HubCorrectionKnownQuery,
    Known,
    HubCorrectionKnownEvidence
);
correction_query_port!(
    hub_correction_target_funding,
    HubCorrectionFundingQuery,
    Funding,
    HubCorrectionFundingEvidence
);

impl RuntimeActorHandle {
    async fn dispatch_correction_read(
        &self,
        command: HubCorrectionRead,
    ) -> Result<HubCorrectionReadValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubCorrection {
                    generation: self.session_generation(),
                    command: HubCorrectionCommand::Read(command),
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubCorrectionRead(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_correction_observation_original(
        &self,
        operation: DurableRequestId,
    ) -> Result<HubCorrectionObservationReceipt, SafeError> {
        match self
            .dispatch_correction_original(operation, HubCorrectionOriginal::Observation)
            .await?
        {
            Some(HubMutationResult::CorrectionObserved(value)) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    async fn dispatch_correction_original(
        &self,
        operation: DurableRequestId,
        kind: HubCorrectionOriginal,
    ) -> Result<Option<HubMutationResult>, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubCorrection {
                    generation: self.session_generation(),
                    command: HubCorrectionCommand::Original { operation, kind },
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubCorrection(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActor {
    pub(super) async fn execute_hub_correction(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubCorrectionCommand,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let scope = self
            .hub_submission_scope(generation)
            .map_err(|_| hub_authority::denied())?;
        let actor = scope.actor;
        let mutating = command.is_mutating();
        let result = self
            .run_async(
                context.deadline(),
                move |adapter, secrets, clock| async move {
                    let database = adapter.database();
                    let value = match command {
                        HubCorrectionCommand::Read(command) => {
                            return correction_read_on(database, actor, command, clock.now())
                                .await
                                .map(RuntimeCommandValue::HubCorrectionRead);
                        }
                        HubCorrectionCommand::Write { operation, command } => Some(
                            hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                command.mutation(),
                            )
                            .await?,
                        ),
                        HubCorrectionCommand::Original { operation, kind } => match kind {
                            HubCorrectionOriginal::Observation => {
                                Some(HubMutationResult::CorrectionObserved(Box::new(
                                    database
                                        .hub_correction_observation_original(actor, operation)
                                        .await?,
                                )))
                            }
                            HubCorrectionOriginal::Found => database
                                .hub_found_correction_original(actor, operation)
                                .await?
                                .map(|value| HubMutationResult::FoundCorrected(Box::new(value))),
                            HubCorrectionOriginal::Additional => database
                                .hub_additional_quantity_original(actor, operation)
                                .await?
                                .map(|value| {
                                    HubMutationResult::AdditionalCorrected(Box::new(value))
                                }),
                            HubCorrectionOriginal::Source => database
                                .hub_source_attribution_original(actor, operation)
                                .await?
                                .map(|value| HubMutationResult::SourceAttributed(Box::new(value))),
                            HubCorrectionOriginal::Recipient => database
                                .hub_recipient_attribution_original(actor, operation)
                                .await?
                                .map(|value| {
                                    HubMutationResult::RecipientAttributed(Box::new(value))
                                }),
                            HubCorrectionOriginal::Explanation => database
                                .hub_count_explanation_operation(actor, operation)
                                .await?
                                .map(|value| HubMutationResult::CountExplained(Box::new(value))),
                            HubCorrectionOriginal::Classification => database
                                .hub_count_classification_operation(actor, operation)
                                .await?
                                .map(|value| HubMutationResult::CountClassified(Box::new(value))),
                            HubCorrectionOriginal::ReturnAssociation => database
                                .hub_count_return_association_operation(actor, operation)
                                .await?
                                .map(|value| {
                                    HubMutationResult::CountReturnAssociated(Box::new(value))
                                }),
                        },
                    };
                    Ok(RuntimeCommandValue::HubCorrection(value))
                },
            )
            .await;
        result.map_err(|error| {
            if mutating {
                hub_authority::mutation_error(error)
            } else {
                error
            }
        })
    }
}

async fn correction_read_on(
    database: &harvestcircle_storage::Database,
    actor: PublicKey,
    command: HubCorrectionRead,
    now: harvestcircle_domain::UnixTimestamp,
) -> Result<HubCorrectionReadValue, SafeError> {
    Ok(match command {
        HubCorrectionRead::Found(query) => HubCorrectionReadValue::Found(Box::new(
            database
                .hub_found_correction_review(actor, query, now)
                .await?,
        )),
        HubCorrectionRead::Additional { operation, command } => {
            HubCorrectionReadValue::Additional(Box::new(
                database
                    .hub_additional_quantity_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::Source { operation, command } => {
            HubCorrectionReadValue::Source(Box::new(
                database
                    .hub_source_attribution_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::Recipient { operation, command } => {
            HubCorrectionReadValue::Recipient(Box::new(
                database
                    .hub_recipient_attribution_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::Explanation { operation, command } => {
            HubCorrectionReadValue::Explanation(Box::new(
                database
                    .hub_count_explanation_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::Classification { operation, command } => {
            HubCorrectionReadValue::Classification(Box::new(
                database
                    .hub_count_classification_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::ReturnAssociation { operation, command } => {
            HubCorrectionReadValue::ReturnAssociation(Box::new(
                database
                    .hub_count_return_association_review(actor, operation, *command, now)
                    .await?,
            ))
        }
        HubCorrectionRead::Known(query) => HubCorrectionReadValue::Known(Box::new(
            database
                .hub_correction_known_departures(actor, query)
                .await?,
        )),
        HubCorrectionRead::Funding(query) => HubCorrectionReadValue::Funding(Box::new(
            database.hub_correction_target_funding(actor, query).await?,
        )),
    })
}
