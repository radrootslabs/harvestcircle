package org.harvestcircle.application

/** Copy bounded, version-pinned native labels for physical group selection. */
internal suspend fun completeTreatmentChoices(
    runtime: HarvestCircleHandlingRuntime,
    first: HubLineagePage,
): List<HubLineageRow> {
    check(first.offset == 0U && first.total <= 256U)
    val rows = mutableListOf<HubLineageRow>()
    var page = first
    repeat(4) {
        check(page.rows.size <= 64)
        rows += page.rows
        check(rows.size <= 256 && rows.map { it.id }.distinct().size == rows.size)
        if (!page.more) {
            check(rows.size.toUInt() == first.total)
            return rows.toList()
        }
        check(page.rows.isNotEmpty())
        val offset = page.offset + page.rows.size.toUInt()
        page = runtime.lineage(HubLineageQuery(first.trace.id, first.stockRevision, first.catalogRevision, offset))
        check(page.stockRevision == first.stockRevision && page.catalogRevision == first.catalogRevision)
        check(page.trace.id == first.trace.id && page.total == first.total && page.offset == offset)
    }
    error("Physical identification choices exceed the native bound")
}

internal suspend fun refreshTreatmentDraft(
    runtime: HarvestCircleHandlingRuntime,
    draft: HandlingDraft.Treatment,
): HandlingDraft.Treatment {
    val current = runtime.treatment(HubTreatmentScope(draft.selected.trace.id))
    val first = runtime.lineage(HubLineageQuery(current.trace.id, current.stockRevision, current.catalogRevision))
    return draft.copy(selected = current, choices = completeTreatmentChoices(runtime, first))
}
