package org.harvestcircle.integration

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.application.CountPresenter
import org.harvestcircle.application.CountResult
import org.harvestcircle.application.CountSubmission
import org.harvestcircle.application.HarvestCircleHandlingRuntime
import org.harvestcircle.application.HubGroupView
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.ui.shell.CountWorkspace
import org.harvestcircle.ui.shell.HarvestCircleTheme
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

@OptIn(ExperimentalTestApi::class)
class CountFormAcceptanceTest {
    @Test
    fun isolatedFormSavesObservationThenExplicitlyPostsNativeShortage() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-count-form-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CountPresenter
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                var failFirstRead = true
                val handling =
                    object : HarvestCircleHandlingRuntime by runtime {
                        override suspend fun group(id: String): HubGroupView {
                            if (failFirstRead) {
                                failFirstRead = false
                                error("Initial context temporarily unavailable")
                            }
                            return runtime.group(id)
                        }
                    }
                setContent {
                    val scope = rememberCoroutineScope()
                    presenter =
                        remember {
                            CountPresenter(
                                runtime,
                                runtime,
                                handling,
                                runtime,
                                scope,
                                OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) },
                                { "2023-11-14T22:13:20Z" },
                            )
                        }
                    val state by presenter.state.collectAsState()
                    LaunchedEffect(Unit) {
                        presenter.attach("isolated-count-form")
                        presenter.beginObservation(hub.receipt.initialPosition.groupId)
                    }
                    HarvestCircleTheme(AppearanceState()) {
                        CountWorkspace(state, presenter) { presenter.submit(runtime.currentSnapshot().revision) }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.message != null && !presenter.state.value.loading }
                onNodeWithTag("count-retry-context").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag("count-submit").assertIsNotEnabled()
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("18")
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Counted eighteen fictional original packs")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val observed = assertIs<CountResult.Observed>((presenter.state.value.submission as CountSubmission.Recorded).result)
                val saved = runBlocking { runtime.stock(HubStockQuery()) }.rows.single()
                assertEquals(20U, saved.position.wholePacks)
                assertEquals(20U, saved.putAway?.remaining)
                onNodeWithTag("count-review-post").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null && presenter.state.value.submission == null }
                onNodeWithTag("count-submit").assertIsNotEnabled()
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Reviewed current scope and original count")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val posted = assertIs<CountResult.Posted>((presenter.state.value.submission as CountSubmission.Recorded).result)
                assertEquals(observed.observation, posted.receipt.observation)
                assertEquals(18U, posted.receipt.onHandAfter)
                val work =
                    posted.receipt.workChanges
                        .single()
                        .after.quantity
                assertEquals(listOf(0U, 2U, 18U), listOf(work.completed, work.noLongerRequired, work.remaining))
                assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
            } finally {
                runOnIdle { presenter.close() }
                runBlocking { runtime.close() }
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}
