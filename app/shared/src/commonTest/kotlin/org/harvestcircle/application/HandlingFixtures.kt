package org.harvestcircle.application

internal object HandlingFixtures {
    val requirement = HubWorkRequirement(StockFixtures.trace.id, StockFixtures.trace.id, StockFixtures.pack.id, StockFixtures.work)
    val work =
        HubWorkRow(
            requirement,
            StockFixtures.product,
            StockFixtures.pack,
            true,
            1U,
            HubWorkOrigin.Intake,
            "Counted twenty original crates",
            StockFixtures.actor,
            UnixSeconds(100),
            StockFixtures.operation,
        )
    val page = HubWorkPage(2UL, 5UL, 0U, 1U, false, listOf(work))
    val group =
        HubGroupView(
            2UL,
            5UL,
            HubPhysicalGroup(
                StockFixtures.position.groupId,
                StockFixtures.trace.id,
                StockFixtures.pack.id,
                null,
                null,
                StockFixtures.receiving.id,
                20U,
                1UL,
                null,
                listOf(HubGroupWork(requirement, HubWorkProgress.Pending)),
            ),
            StockFixtures.receiving,
            StockFixtures.product,
            StockFixtures.pack,
            StockFixtures.trace,
        )
    val fields =
        HandlingFields(
            targetId = StockFixtures.planned.id,
            scope = MovementScope.IdentifiedPortion,
            quantity = "8",
            physicalLabel = "Blue stack",
            physicalObservation = "Eight separately tagged crates",
            observation = "Actual arrival observed",
        )

    fun receipt(operation: OperationId) =
        HubHandlingReceipt(
            operation,
            HubHandlingKind.Move,
            3UL,
            5UL,
            StockFixtures.trace.id,
            StockFixtures.actor,
            UnixSeconds(100),
            HubHandlingResult.Moved(
                StockFixtures.position.groupId,
                1UL,
                20U,
                StockFixtures.receiving,
                StockFixtures.planned,
                8U,
                "07".repeat(16),
                12U,
                HubPhysicalIdentification("Blue stack", "Eight separately tagged crates"),
                HubOccurrence(HubOccurrenceKind.Unknown),
                fields.observation,
                emptyList(),
            ),
        )
}

internal open class HandlingFake :
    HarvestCircleHandlingRuntime,
    HarvestCircleReferenceRuntime {
    var groupView = HandlingFixtures.group
    var workPage = HandlingFixtures.page
    var stored: HubHandlingReceipt? = null
    var failure: Exception? = null
    var loseReply = true
    val writes = mutableListOf<Pair<RequestContext, HubHandlingInput>>()
    val lookups = mutableListOf<OperationId>()
    val queries = mutableListOf<HubHandlingQuery>()

    override suspend fun group(id: String) = groupView

    override suspend fun work(query: HubHandlingQuery): HubWorkPage {
        queries += query
        return workPage
    }

    override suspend fun discrepancies(query: HubHandlingQuery) = HubDiscrepancyPage(2UL, 5UL, 0U, 0U, false, emptyList())

    override suspend fun applyHandling(
        context: RequestContext,
        input: HubHandlingInput,
    ): HubHandlingReceipt {
        writes += context to input
        failure?.let { throw it }
        val receipt = HandlingFixtures.receipt(context.operationId)
        stored = receipt
        if (loseReply) error("Reply lost after native commit")
        return receipt
    }

    override suspend fun handlingOperation(operation: OperationId): HubHandlingReceipt? {
        lookups += operation
        return stored
    }

    override suspend fun references(query: HubReferenceQuery) =
        HubReferencePage(
            StockFixtures.basis,
            requireNotNull(query.expectedCatalogRevision),
            0U,
            4U,
            false,
            StockFixtures.references,
        )

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt = error("Unexpected edit")

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? = error("Unexpected lookup")

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> = error("Unexpected history")
}
