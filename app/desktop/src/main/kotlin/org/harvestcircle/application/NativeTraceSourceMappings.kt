package org.harvestcircle.application

import org.harvestcircle.ffi.HubCorrectionImpactDto
import org.harvestcircle.ffi.HubCorrectionImpactKindDto
import org.harvestcircle.ffi.HubTraceSourceCorrectionDto

internal fun HubTraceSourceCorrectionDto.toModel() =
    HubTraceSourceCorrection(
        OperationId.from(operationId),
        previousOperationId?.let(OperationId::from),
        stockRevision,
        catalogRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        before.toModel(),
        after.toModel(),
        basis,
        impact.map { it.toModel() },
    )

internal fun HubCorrectionImpactDto.toModel() =
    HubCorrectionImpact(
        when (kind) {
            HubCorrectionImpactKindDto.POPULATION -> HubCorrectionImpactKind.Population
            HubCorrectionImpactKindDto.SOURCE -> HubCorrectionImpactKind.Source
            HubCorrectionImpactKindDto.DEPARTURE -> HubCorrectionImpactKind.Departure
            HubCorrectionImpactKindDto.ISSUED_VERSION -> HubCorrectionImpactKind.IssuedVersion
            HubCorrectionImpactKindDto.RECIPIENT -> HubCorrectionImpactKind.Recipient
            HubCorrectionImpactKindDto.UNRESOLVED_SOURCE -> HubCorrectionImpactKind.UnresolvedSource
            HubCorrectionImpactKindDto.UNRESOLVED_RECIPIENT -> HubCorrectionImpactKind.UnresolvedRecipient
        },
        targetId,
    )
