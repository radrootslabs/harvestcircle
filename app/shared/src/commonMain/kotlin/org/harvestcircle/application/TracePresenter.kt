package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Read-only coordination. Native policy owns scope, accounting and historical facts. */
class TracePresenter(
    private val runtime: HarvestCircleTraceRuntime?,
    private val scope: CoroutineScope,
    outputPort: LocalOutputPort? = null,
) {
    val output = LocalOutputPresenter(runtime, outputPort, scope)
    private val mutableState = MutableStateFlow(TraceViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null

    fun attach(id: String) {
        if (closed || runtime == null || hub == id) return
        generation += 1
        job?.cancel()
        hub = id
        output.attach(id)
        mutableState.value = TraceViewState(available = true)
    }

    fun browse(
        scope: HubTraceScope = HubTraceScope.All,
        sourceId: String? = null,
    ) {
        if (!canRead()) return
        val selection = HubTraceSelection(scope, sourceId = sourceId)
        mutableState.value =
            mutableState.value.copy(open = true, selection = selection, queryText = "", targetId = null, snapshot = null, history = null)
        loadPage(HubTraceQuery(selection))
    }

    fun search(text: String) {
        val current = mutableState.value
        if (text.length <= 128 && current.selection.scope in setOf(HubTraceScope.All, HubTraceScope.Unresolved)) {
            mutableState.value = current.copy(queryText = text)
        }
    }

    fun refresh() {
        val current = mutableState.value
        current.targetId?.let {
            openReceipt(it)
            return
        }
        loadPage(HubTraceQuery(current.selection.copy(text = current.queryText)))
    }

    fun nextPage() {
        val current = mutableState.value
        val page = current.page?.takeIf { it.more } ?: return
        if (current.loading) return
        loadPage(HubTraceQuery(page.selection, page.stockRevision, page.catalogRevision, page.offset + page.rows.size.toUInt()))
    }

    private fun loadPage(query: HubTraceQuery) {
        mutableState.value = mutableState.value.copy(page = null, targetId = null, snapshot = null, history = null)
        read {
            val page = requireNotNull(runtime).tracePage(query)
            check(page.selection == query.selection && page.offset == query.offset && page.rows.size <= 64 && page.total <= 4096U)
            check(query.expectedStockRevision == null || query.expectedStockRevision == page.stockRevision)
            check(query.expectedCatalogRevision == null || query.expectedCatalogRevision == page.catalogRevision)
            val update: (TraceViewState) -> TraceViewState = { it.copy(page = page, selection = page.selection, needsRefresh = false) }
            update
        }
    }

    fun openReceipt(id: String) {
        if (!canRead()) return
        mutableState.value =
            mutableState.value.copy(
                open = true,
                targetId = id,
                snapshot = null,
                history = null,
                section = TraceSection.Overview,
                positionsOffset = 0,
                exitsOffset = 0,
            )
        read {
            val snapshot = requireNotNull(runtime).traceSnapshot(HubTraceSnapshotQuery(id))
            check(snapshot.trace.id == id)
            val history = requireNotNull(runtime).traceHistory(HubTraceHistoryQuery(id, snapshot.stockRevision, snapshot.catalogRevision))
            checkHistory(snapshot, history)
            check(history.offset == 0U)
            val update: (TraceViewState) -> TraceViewState = { it.copy(snapshot = snapshot, history = history, needsRefresh = false) }
            update
        }
    }

    fun closeRecord() {
        if (mutableState.value.loading) return
        val current = mutableState.value
        mutableState.value = current.copy(targetId = null, snapshot = null, history = null, needsRefresh = false, message = null)
        if (current.page == null) browse()
    }

    fun historyNext() {
        val current = mutableState.value
        val history = current.history?.takeIf { it.more } ?: return
        if (!current.loading) historyPage(history.offset + history.rows.size.toUInt())
    }

    fun historyFirst() = historyPage(0U)

    fun chooseSection(section: TraceSection) {
        if (mutableState.value.snapshot != null) mutableState.value = mutableState.value.copy(section = section)
    }

    private fun historyPage(offset: UInt) {
        val current = mutableState.value
        val snapshot = current.snapshot ?: return
        if (current.loading || current.needsRefresh) return
        mutableState.value = current.copy(history = null)
        read {
            val history =
                requireNotNull(
                    runtime,
                ).traceHistory(HubTraceHistoryQuery(snapshot.trace.id, snapshot.stockRevision, snapshot.catalogRevision, offset))
            checkHistory(snapshot, history)
            check(history.offset == offset)
            val update: (TraceViewState) -> TraceViewState = { it.copy(history = history) }
            update
        }
    }

    fun positionsPage(delta: Int) {
        val current = mutableState.value
        val count = current.snapshot?.onHand?.size ?: return
        val offset = current.positionsOffset + delta * 64
        if (delta in -1..1 && offset >= 0 && offset < count) mutableState.value = current.copy(positionsOffset = offset)
    }

    fun exitsPage(delta: Int) {
        val current = mutableState.value
        val count = current.snapshot?.exits?.size ?: return
        val offset = current.exitsOffset + delta * 64
        if (delta in -1..1 && offset >= 0 && offset < count) mutableState.value = current.copy(exitsOffset = offset)
    }

    fun park() {
        output.dismiss()
        generation += 1
        job?.cancel()
        mutableState.value = mutableState.value.copy(open = false, loading = false)
    }

    /** Inspector dismissal precedes route history; an existing modal keeps Escape. */
    fun intercept(
        intent: HarvestCircleShellIntent,
        overlay: FoundationOverlay?,
    ): Boolean {
        val back = intent is HarvestCircleShellIntent.Navigation && intent.intent == org.harvestcircle.navigation.NavigationIntent.Back
        val escape = intent is HarvestCircleShellIntent.Overlay && intent.intent is OverlayIntent.Escape && overlay == null
        if (output.state.value.open && (back || escape)) return output.dismiss()
        if (mutableState.value.open && (back || escape)) {
            park()
            return true
        }
        if (intent is HarvestCircleShellIntent.Navigate || intent is HarvestCircleShellIntent.Navigation) {
            if (output.state.value.open) {
                output.dismiss()
                if (output.state.value.open) return true
            }
            park()
        }
        return false
    }

    private fun checkHistory(
        snapshot: HubTraceSnapshot,
        history: HubTraceHistoryPage,
    ) {
        check(
            history.traceId == snapshot.trace.id &&
                history.stockRevision == snapshot.stockRevision &&
                history.catalogRevision == snapshot.catalogRevision,
        )
        check(history.rows.size <= 64 && history.total <= 32768U)
    }

    private fun read(block: suspend () -> (TraceViewState) -> TraceViewState) {
        if (!canRead()) return
        generation += 1
        val token = generation
        job?.cancel()
        mutableState.value = mutableState.value.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val update = block()
                    if (isCurrent(token)) mutableState.value = update(mutableState.value).copy(loading = false)
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                loading = false,
                                needsRefresh = true,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "This trace snapshot could not be read. Refresh the same population or selection.",
                            )
                    }
                }
            }
    }

    private fun canRead() = !closed && hub != null && runtime != null

    private fun isCurrent(token: Long) = !closed && token == generation

    fun close() {
        output.close()
        closed = true
        generation += 1
        job?.cancel()
        hub = null
        mutableState.value = TraceViewState()
    }
}
