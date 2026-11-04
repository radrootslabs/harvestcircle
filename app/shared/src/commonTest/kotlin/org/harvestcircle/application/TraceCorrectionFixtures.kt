package org.harvestcircle.application

internal fun traceCorrectionRows(count: Int): List<HubTraceQuantityCorrection> =
    listOf(
        HubTraceQuantityCorrection(
            StockFixtures.operation,
            StockFixtures.position.groupId,
            100U,
            35U,
            2U,
            List(count) { index ->
                HubTraceQuantityAmendment(
                    OperationId.from("01900000-0000-7000-8000-${(index + 1).toString().padStart(12, '0')}"),
                    (index + 3).toULong(),
                    100U - index.toUInt(),
                    99U - index.toUInt(),
                    HubTraceQuantityAmendmentKind.IdentifiedFoundRemainder,
                    1U,
                )
            },
        ),
    )
