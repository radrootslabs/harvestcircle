package org.harvestcircle.application

internal fun traceSourceRows(
    count: Int,
    impacts: Int = 65,
): List<HubTraceSourceCorrection> {
    var before = StockFixtures.trace.source
    return List(count) { index ->
        val after =
            HubSource(
                (index + 1).toString(16).padStart(32, '0'),
                null,
                HubSourceKind.Other,
                "Possible source ${index + 1}",
                "Independent possible source",
                HubSourceCertainty.Possible,
            )
        HubTraceSourceCorrection(
            OperationId.from("01900000-0000-7000-8000-${(index + 1).toString().padStart(12, '0')}"),
            if (index == 0) null else OperationId.from("01900000-0000-7000-8000-${index.toString().padStart(12, '0')}"),
            (index + 3).toULong(),
            5UL,
            StockFixtures.actor,
            UnixSeconds(200L + index),
            before,
            after,
            "Compared original and amended source ${index + 1}",
            List(impacts) { HubCorrectionImpact(HubCorrectionImpactKind.Population, (it + 1).toString(16).padStart(32, '0')) },
        ).also { before = after }
    }
}
