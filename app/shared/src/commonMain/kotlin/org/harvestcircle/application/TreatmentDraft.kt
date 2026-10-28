package org.harvestcircle.application

enum class TreatmentEditorAction { Acceptance, PlaceHold, ReleaseHold }

internal fun treatmentDraft(
    state: HandlingViewState,
    action: HandlingAction,
): HandlingDraft.Treatment? {
    val snapshot = state.treatment ?: return null
    val kind =
        when (action) {
            HandlingAction.SetAcceptance -> TreatmentEditorAction.Acceptance
            HandlingAction.PlaceHold -> TreatmentEditorAction.PlaceHold
            HandlingAction.ReleaseHold -> TreatmentEditorAction.ReleaseHold
            else -> return null
        }
    return HandlingDraft.Treatment(
        snapshot,
        kind,
        HandlingFields(treatmentGroups = state.group?.let { listOf(it.group.id) } ?: emptyList()),
        choices = state.treatmentChoices,
    )
}

internal fun HandlingDraft.Treatment.treatmentInput(): HubHandlingInput {
    if (action != TreatmentEditorAction.ReleaseHold) require(fields.treatmentGroups.isNotEmpty())
    return when (action) {
        TreatmentEditorAction.Acceptance ->
            HubHandlingInput.SetAcceptance(
                versions.stock,
                versions.catalog,
                fields.treatmentGroups,
                requireNotNull(fields.acceptanceStatus),
                fields.occurrence(),
                fields.observation,
            )
        TreatmentEditorAction.PlaceHold ->
            HubHandlingInput.PlaceHold(
                versions.stock,
                versions.catalog,
                fields.treatmentGroups,
                fields.occurrence(),
                fields.observation,
            )
        TreatmentEditorAction.ReleaseHold -> {
            val hold = requireNotNull(selected.holds.find { it.hold.stamp.id == fields.holdId }).hold
            HubHandlingInput.ReleaseHold(
                versions.stock,
                versions.catalog,
                hold.stamp.id,
                hold.revision,
                fields.occurrence(),
                fields.observation,
            )
        }
    }
}

internal suspend fun treatmentHistoryPage(
    runtime: HarvestCircleHandlingRuntime,
    query: HubTreatmentQuery,
): HubTreatmentPage {
    val page = runtime.treatmentHistory(query)
    check(page.offset == query.offset && page.total <= 512U && page.rows.size <= 64)
    check(page.rows.all { it.result is HubHandlingResult.Treatment })
    check(query.expectedStockRevision == null || query.expectedStockRevision == page.stockRevision)
    check(query.expectedCatalogRevision == null || query.expectedCatalogRevision == page.catalogRevision)
    return page
}
