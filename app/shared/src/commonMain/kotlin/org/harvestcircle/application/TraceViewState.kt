package org.harvestcircle.application

enum class TraceSection { Overview, History }

data class TraceViewState(
    val available: Boolean = false,
    val open: Boolean = false,
    val loading: Boolean = false,
    val selection: HubTraceSelection = HubTraceSelection(),
    val queryText: String = "",
    val page: HubTracePage? = null,
    val targetId: String? = null,
    val snapshot: HubTraceSnapshot? = null,
    val history: HubTraceHistoryPage? = null,
    val section: TraceSection = TraceSection.Overview,
    val positionsOffset: Int = 0,
    val exitsOffset: Int = 0,
    val countLossesOffset: Int = 0,
    val needsRefresh: Boolean = false,
    val message: String? = null,
)
