package org.harvestcircle.application

internal fun traceRecipientRows(
    count: Int,
    impacts: Int = 65,
): List<HubTraceRecipientCorrection> {
    val original = HubReferenceRecord("a".repeat(32), 1UL, HubReferenceValue(HubReferenceKind.Party, "Original recipient A"))
    var before: HubReferenceRecord? = original
    val event = OperationId.from("01900000-0000-7000-8000-000000000900")
    return List(count) { index ->
        val after =
            if (index ==
                0
            ) {
                null
            } else {
                HubReferenceRecord(
                    (index + 1).toString(16).padStart(32, '0'),
                    1UL,
                    HubReferenceValue(HubReferenceKind.Party, "Amended recipient ${index + 1}"),
                )
            }
        HubTraceRecipientCorrection(
            OperationId.from("01900000-0000-7000-8000-${(index + 1).toString().padStart(12, '0')}"),
            if (index == 0) null else OperationId.from("01900000-0000-7000-8000-${index.toString().padStart(12, '0')}"),
            (index + 3).toULong(),
            5UL,
            StockFixtures.actor,
            UnixSeconds(200L + index),
            event,
            original,
            listOf(StockFixtures.trace.id),
            listOf("b".repeat(32)),
            before,
            after,
            "Compared the complete original and amended recipient ${index + 1}",
            List(impacts) { HubCorrectionImpact(HubCorrectionImpactKind.Population, (it + 1).toString(16).padStart(32, '0')) },
        ).also { before = after }
    }
}
