package org.harvestcircle.application

// Presentation-only row fixtures. Native policy and persisted effects are
// qualified separately; these values exercise bounded read coordination.
internal fun traceCountSnapshot(rows: Int): HubTraceSnapshot {
    val base = TraceFake().snapshot
    val party = HubReferenceRecord("44".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Party, "Original fictional recipient"))

    fun operation(n: Int) = OperationId.from("01900000-0000-7000-8000-${n.toString().padStart(12, '0')}")
    val accounts =
        (0 until rows).map { index ->
            HubCountInterpretationAccount(
                operation(10000 + index),
                "count-$index",
                "observation-$index",
                base.trace.id,
                base.trace.variant.id,
                "anchor-$index",
                2UL,
                base.catalogRevision,
                StockFixtures.actor,
                UnixSeconds(110),
                UnixSeconds(80),
                2U,
                0U,
                2U,
                listOf(HubCountClassificationMeaning(operation(20000 + index), 2UL, operation(30000 + index), 1U, 0U)),
            )
        }
    val explanations =
        (0 until rows).map { index ->
            HubCountExplanationMeaning(
                operation(30000 + index),
                operation(10000 + index),
                2UL,
                base.catalogRevision,
                StockFixtures.actor,
                UnixSeconds(150),
                HubOccurrence(HubOccurrenceKind.Known, 70, 70),
                party,
                HubActualKnownInterpretation(
                    base.stockRevision,
                    base.catalogRevision,
                    UnixSeconds(200),
                    1U,
                    0U,
                    null,
                    emptyList(),
                    emptyList(),
                ),
                0U,
            )
        }
    val associations =
        (0 until rows).map { index ->
            HubCountReturnAssociation(
                "association-$index",
                operation(40000 + index),
                2UL,
                base.catalogRevision,
                StockFixtures.actor,
                UnixSeconds(200),
                operation(50000 + index),
                2UL,
                "return-$index",
                "returned-population-$index",
                "returned-group-$index",
                operation(30000 + index),
                2UL,
                base.trace.id,
                operation(10000 + index),
                2UL,
                "anchor-$index",
                base.trace.variant.id,
                1U,
                HubOccurrence(HubOccurrenceKind.Unknown),
                HubOccurrence(HubOccurrenceKind.Known, 70, 70),
                HubReturnTiming.Uncertain,
            )
        }
    return base.copy(
        countInterpretations = HubCountInterpretations(base.stockRevision, base.catalogRevision, UnixSeconds(200), accounts, explanations),
        countReturnAssociations = associations,
        countReturnUncertain = true,
    )
}
