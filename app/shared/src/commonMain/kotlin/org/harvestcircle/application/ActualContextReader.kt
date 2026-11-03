package org.harvestcircle.application

import kotlin.time.Instant

/** Bounded read choices only; native previews independently validate complete history. */
internal class ActualContextReader(
    private val stock: HarvestCircleStockRuntime?,
    private val handling: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val traces: HarvestCircleTraceRuntime?,
) {
    suspend fun load(fields: ActualFields): ActualContext {
        val page = requireNotNull(stock).stock(HubStockQuery())
        val tracePage =
            requireNotNull(
                traces,
            ).tracePage(HubTraceQuery(expectedStockRevision = page.stockRevision, expectedCatalogRevision = page.catalogRevision))
        val records = referenceChoices(page.catalogRevision)
        val group = fields.groupId?.let { requireNotNull(handling).group(it) }
        check(group == null || group.stockRevision == page.stockRevision && group.catalogRevision == page.catalogRevision)
        var result = ActualContext(references = records, stock = page, traces = tracePage, group = group)
        if (group != null) {
            val treatment =
                requireNotNull(
                    handling,
                ).treatment(HubTreatmentScope(group.group.traceId, page.stockRevision, page.catalogRevision))
            result = result.copy(treatment = treatment.groups.single { it.groupId == group.group.id })
            val lineage = requireNotNull(handling).lineage(HubLineageQuery(group.group.traceId, page.stockRevision, page.catalogRevision))
            val history =
                requireNotNull(
                    traces,
                ).traceHistory(HubTraceHistoryQuery(group.group.traceId, page.stockRevision, page.catalogRevision))
            result = result.copy(lineage = lineage, sourceHistory = history, sources = sourceChoices(lineage, history))
        }
        if (fields.traceId != null) result = result.copy(known = knownChoices(fields.traceId, page.stockRevision, page.catalogRevision))
        return result
    }

    suspend fun next(
        context: ActualContext,
        kind: ActualChoicePage,
    ): ActualContext {
        val stockPage = requireNotNull(context.stock)
        val revision = stockPage.stockRevision
        val catalog = stockPage.catalogRevision
        return when (kind) {
            ActualChoicePage.Stock -> {
                check(stockPage.more)
                context.copy(
                    stock =
                        requireNotNull(stock).stock(
                            HubStockQuery(
                                expectedStockRevision = revision,
                                expectedCatalogRevision = catalog,
                                offset =
                                    stockPage.offset + stockPage.rows.size.toUInt(),
                            ),
                        ),
                )
            }
            ActualChoicePage.Traces -> {
                val page = requireNotNull(context.traces)
                check(page.more)
                context.copy(
                    traces =
                        requireNotNull(traces).tracePage(
                            HubTraceQuery(
                                expectedStockRevision = revision,
                                expectedCatalogRevision = catalog,
                                offset =
                                    page.offset + page.rows.size.toUInt(),
                            ),
                        ),
                )
            }
            ActualChoicePage.Lineage -> {
                val page = requireNotNull(context.lineage)
                check(page.more)
                val lineage =
                    requireNotNull(handling).lineage(
                        HubLineageQuery(
                            page.trace.id,
                            revision,
                            catalog,
                            page.offset + page.rows.size.toUInt(),
                        ),
                    )
                context.copy(lineage = lineage, sources = sourceChoices(lineage, requireNotNull(context.sourceHistory)))
            }
            ActualChoicePage.History -> {
                val page = requireNotNull(context.sourceHistory)
                check(page.more)
                val group = requireNotNull(context.group)
                val history =
                    requireNotNull(traces).traceHistory(
                        HubTraceHistoryQuery(
                            group.group.traceId,
                            revision,
                            catalog,
                            page.offset + page.rows.size.toUInt(),
                        ),
                    )
                context.copy(sourceHistory = history, sources = sourceChoices(requireNotNull(context.lineage), history))
            }
        }
    }

    private suspend fun sourceChoices(
        lineage: HubLineagePage,
        history: HubTraceHistoryPage,
    ): List<ActualSourceChoice> {
        val choices =
            lineage.rows
                .map {
                    ActualSourceChoice(
                        it.createdOperationId,
                        it.id,
                        "${it.createdKind} • ${it.id} • recorded ${Instant.fromEpochSeconds(it.createdAt.value)}",
                    )
                }.toMutableList()
        for (entry in history.rows.filter { it.kind == HubTraceHistoryKind.Move }) {
            val original = requireNotNull(handling).handlingOperation(entry.operationId) ?: continue
            check(original.operationId == entry.operationId)
            val moved = original.result as? HubHandlingResult.Moved ?: continue
            choices +=
                ActualSourceChoice(
                    original.operationId,
                    moved.movedGroupId,
                    "Arrival • ${moved.to.value.label} • recorded ${Instant.fromEpochSeconds(original.recordedAt.value)}",
                )
        }
        return choices.distinctBy { it.key }
    }

    private suspend fun knownChoices(
        traceId: String,
        revision: ULong,
        catalog: ULong,
    ): List<ActualKnownChoice> {
        val original = requireNotNull(traces).traceSnapshot(HubTraceSnapshotQuery(traceId, revision, catalog))
        check(original.trace.id == traceId)
        val variant = original.trace.variant
        return (
            original.exits.map {
                ActualKnownChoice(
                    it.operationId,
                    traceId,
                    variant.id,
                    "Departure • ${it.recipient.value.label} • ${it.operationId.value}",
                )
            } +
                original.actualDepartures.map {
                    ActualKnownChoice(
                        it.operationId,
                        traceId,
                        it.variant.id,
                        "Reconciled actual • ${it.recipient.value.label} • ${it.operationId.value}",
                    )
                }
        ).distinctBy { it.key }
    }

    private suspend fun referenceChoices(revision: ULong): List<HubReferenceRecord> {
        val result = mutableListOf<HubReferenceRecord>()
        var offset = 0U
        repeat(64) {
            val page = requireNotNull(references).references(HubReferenceQuery(expectedCatalogRevision = revision, offset = offset))
            check(page.catalogRevision == revision && page.records.size <= 64)
            result += page.records
            if (!page.more) return result
            check(page.records.isNotEmpty())
            offset += page.records.size.toUInt()
        }
        error("Reference choices exceed bounded catalog")
    }
}

enum class ActualChoicePage { Stock, Traces, Lineage, History }
