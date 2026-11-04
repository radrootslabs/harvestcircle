package org.harvestcircle.application

import org.harvestcircle.ffi.HubTraceRecipientCorrectionDto

internal fun HubTraceRecipientCorrectionDto.toModel() =
    HubTraceRecipientCorrection(
        OperationId.from(operationId),
        previousOperationId?.let(OperationId::from),
        stockRevision,
        catalogRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        OperationId.from(originalDepartureOperationId),
        originalRecipient.toModel(),
        originalPopulationIds,
        originalIssuedVersionIds,
        before?.toModel(),
        after?.toModel(),
        basis,
        impact.map { it.toModel() },
    )
