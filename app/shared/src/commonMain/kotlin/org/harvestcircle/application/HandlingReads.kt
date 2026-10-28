package org.harvestcircle.application

internal suspend fun handlingReferenceChoices(
    runtime: HarvestCircleReferenceRuntime,
    revision: ULong,
): List<HubReferenceRecord> {
    val records = mutableListOf<HubReferenceRecord>()
    var offset = 0U
    repeat(8) {
        val page = runtime.references(HubReferenceQuery(expectedCatalogRevision = revision, offset = offset))
        check(page.catalogRevision == revision && page.offset == offset && page.total <= 512U)
        records += page.records
        check(records.size <= 512)
        if (!page.more) {
            check(records.size.toUInt() == page.total)
            return records.toList()
        }
        check(page.records.isNotEmpty())
        offset += page.records.size.toUInt()
    }
    error("Reference choices exceed the bounded catalog")
}

internal suspend fun refreshHandlingDraft(
    runtime: HarvestCircleHandlingRuntime,
    draft: HandlingDraft,
): HandlingDraft =
    when (draft) {
        is HandlingDraft.Treatment -> refreshTreatmentDraft(runtime, draft)
        is HandlingDraft.Split -> draft.copy(selected = runtime.group(draft.selected.group.id))
        is HandlingDraft.Move -> draft.copy(selected = runtime.group(draft.selected.group.id))
        is HandlingDraft.NewWork -> draft.copy(selected = runtime.group(draft.selected.group.id))
        is HandlingDraft.OpenDiscrepancy -> draft.copy(selected = runtime.group(draft.selected.group.id))
        is HandlingDraft.Replan -> {
            val (row, versions) = findHandlingWork(runtime, draft.selected.work.id, draft.selected.work.traceId)
            draft.copy(selected = row, versions = versions)
        }
        is HandlingDraft.ResolveDiscrepancy -> {
            val (row, versions) = findHandlingDiscrepancy(runtime, draft.selected.value.id, draft.selected.value.traceId)
            draft.copy(selected = row, versions = versions)
        }
    }

private suspend fun findHandlingWork(
    runtime: HarvestCircleHandlingRuntime,
    id: String,
    trace: String,
): Pair<HubWorkRow, HandlingVersions> {
    var query = HubHandlingQuery(traceId = trace, openOnly = false)
    repeat(256) {
        val page = runtime.work(query)
        check(page.offset == query.offset && page.total <= 16_384U && page.rows.size <= 64)
        val versions = HandlingVersions(page.stockRevision, page.catalogRevision)
        page.rows.find { it.work.id == id }?.let { return it to versions }
        if (!page.more) error("The selected requirement is unavailable")
        check(page.rows.isNotEmpty())
        query =
            query.copy(
                expectedStockRevision = page.stockRevision,
                expectedCatalogRevision = page.catalogRevision,
                offset = page.offset + page.rows.size.toUInt(),
            )
    }
    error("Work retrieval exceeds its native bound")
}

private suspend fun findHandlingDiscrepancy(
    runtime: HarvestCircleHandlingRuntime,
    id: String,
    trace: String,
): Pair<HubDiscrepancyRow, HandlingVersions> {
    var query = HubHandlingQuery(traceId = trace, openOnly = false)
    repeat(64) {
        val page = runtime.discrepancies(query)
        check(page.offset == query.offset && page.total <= 4096U && page.rows.size <= 64)
        val versions = HandlingVersions(page.stockRevision, page.catalogRevision)
        page.rows.find { it.value.id == id }?.let { return it to versions }
        if (!page.more) error("The selected discrepancy is unavailable")
        check(page.rows.isNotEmpty())
        query =
            query.copy(
                expectedStockRevision = page.stockRevision,
                expectedCatalogRevision = page.catalogRevision,
                offset = page.offset + page.rows.size.toUInt(),
            )
    }
    error("Discrepancy retrieval exceeds its native bound")
}
