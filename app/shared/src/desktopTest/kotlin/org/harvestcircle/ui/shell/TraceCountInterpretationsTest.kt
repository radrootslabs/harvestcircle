package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.TraceFake
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.traceCountSnapshot
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class TraceCountInterpretationsTest {
    @Test
    fun renderedCountPagesKeepZeroUnknownClassificationsAndReturnLinksWithoutMutation() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val original = traceCountSnapshot(65)
            val counts = requireNotNull(original.countInterpretations)
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            var presenter: TracePresenter? = null
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                val active =
                    androidx.compose.runtime.remember {
                        TracePresenter(runtime, scope).also {
                            presenter = it
                            it.attach("hub")
                            it.openReceipt(original.trace.id)
                        }
                    }
                val state by active.state.collectAsState()
                state.snapshot?.let { snapshot ->
                    Column(Modifier.verticalScroll(rememberScrollState())) { TraceCountInterpretations(snapshot, state, active) }
                }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            for ((section, first, last) in listOf(
                Triple(
                    "accounts",
                    "account-${counts.accounts.first().countOperationId.value}",
                    "account-${counts.accounts.last().countOperationId.value}",
                ),
                Triple(
                    "explanations",
                    "explanation-${counts.explanations.first().operationId.value}",
                    "explanation-${counts.explanations.last().operationId.value}",
                ),
                Triple(
                    "classifications",
                    "classification-${counts.accounts.first().classifications.single().operationId.value}",
                    "classification-${counts.accounts.last().classifications.single().operationId.value}",
                ),
                Triple(
                    "returns",
                    "association-${original.countReturnAssociations.first().operationId.value}",
                    "association-${original.countReturnAssociations.last().operationId.value}",
                ),
            )) {
                onNodeWithTag("trace-count-$first").assertExists()
                onNodeWithTag("trace-count-$last").assertDoesNotExist()
                onNodeWithTag("trace-count-$section-previous").performScrollTo().assertIsNotEnabled()
                onNodeWithTag("trace-count-$section-next").performScrollTo().performClick()
                onNodeWithTag("trace-count-$last").assertExists()
                onNodeWithTag("trace-count-$first").assertDoesNotExist()
                onNodeWithTag("trace-count-$section-next").performScrollTo().assertIsNotEnabled()
            }
            onNodeWithText("Original 1 · Current 0 · Established returned 0").assertExists()
            onNodeWithText("Current recipient: Unknown recipient").assertExists()
            onNodeWithText("1 → 0 · Revision 2").assertExists()
            onNodeWithText("Return timing remains uncertain").assertExists()
            runOnIdle { presenter?.close() }
        }

    @Test
    fun incomingReturnTraceWithNoLocalExplanationsStillShowsTheSeparatePopulationLink() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val base = traceCountSnapshot(1)
            val original =
                base.copy(
                    countInterpretations =
                        requireNotNull(
                            base.countInterpretations,
                        ).copy(accounts = emptyList(), explanations = emptyList()),
                )
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            var presenter: TracePresenter? = null
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                val active =
                    androidx.compose.runtime.remember {
                        TracePresenter(runtime, scope).also {
                            presenter = it
                            it.attach("hub")
                            it.openReceipt(original.trace.id)
                        }
                    }
                val state by active.state.collectAsState()
                state.snapshot?.let { snapshot -> Column { TraceCountInterpretations(snapshot, state, active) } }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            onNodeWithTag("trace-count-association-${original.countReturnAssociations.single().operationId.value}").assertExists()
            onNodeWithText("Returned population returned-population-0 · Explanation population ${original.trace.id}").assertExists()
            runOnIdle { presenter?.close() }
        }
}
