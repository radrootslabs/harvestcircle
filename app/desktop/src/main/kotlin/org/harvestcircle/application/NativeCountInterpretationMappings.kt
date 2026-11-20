package org.harvestcircle.application

import org.harvestcircle.ffi.HubCountInterpretationsDto
import org.harvestcircle.ffi.HubCountReturnAssociationDto

internal fun HubCountInterpretationsDto.toModel(): HubCountInterpretations {
    require(schema == 2U) { "Unsupported count interpretation schema" }
    return HubCountInterpretations(
        stockRevision,
        catalogRevision,
        UnixSeconds(latestRecordedAtUnixS),
        accounts.map { account ->
            HubCountInterpretationAccount(
                OperationId.from(account.countOperationId),
                account.reconciliationId,
                account.observationId,
                account.traceId,
                account.variantId,
                account.anchorGroupId,
                account.stockRevision,
                account.catalogRevision,
                IdentityId.fromPublicKeyHex(account.actorPublicKey),
                UnixSeconds(account.recordedAtUnixS),
                UnixSeconds(account.confirmedOnHandAtUnixS),
                account.grossShortageWholePacks,
                account.explainedWholePacks,
                account.unexplainedWholePacks,
                account.classifications.map { classification ->
                    HubCountClassificationMeaning(
                        OperationId.from(classification.operationId),
                        classification.stockRevision,
                        OperationId.from(classification.explanationOperationId),
                        classification.beforeWholePacks,
                        classification.afterWholePacks,
                    )
                },
            )
        },
        explanations.map { explanation ->
            HubCountExplanationMeaning(
                OperationId.from(explanation.operationId),
                OperationId.from(explanation.countOperationId),
                explanation.stockRevision,
                explanation.catalogRevision,
                IdentityId.fromPublicKeyHex(explanation.actorPublicKey),
                UnixSeconds(explanation.recordedAtUnixS),
                explanation.occurrence.toModel(),
                explanation.originalRecipient.toModel(),
                explanation.interpretation.toModel(),
                explanation.establishedReturnWholePacks,
            )
        },
    )
}

internal fun HubCountReturnAssociationDto.toModel() =
    HubCountReturnAssociation(
        id,
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        OperationId.from(returnOperationId),
        returnStockRevision,
        returnedId,
        returnedPopulationId,
        returnedGroupId,
        OperationId.from(explanationOperationId),
        explanationStockRevision,
        explanationTraceId,
        OperationId.from(countOperationId),
        countStockRevision,
        anchorGroupId,
        variantId,
        wholePacks,
        returnOccurrence.toModel(),
        explanationOccurrence.toModel(),
        timing.toModel(),
    )
