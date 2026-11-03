package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.CountFields
import org.harvestcircle.application.CustodyAttempt
import org.harvestcircle.application.CustodyContext
import org.harvestcircle.application.CustodyDraft
import org.harvestcircle.application.CustodyFields
import org.harvestcircle.application.CustodyPresenter
import org.harvestcircle.application.CustodyRequest
import org.harvestcircle.application.CustodySubmission
import org.harvestcircle.application.CustodyTarget
import org.harvestcircle.application.CustodyViewState
import org.harvestcircle.application.HandlingFake
import org.harvestcircle.application.HarvestCircleCustodyRuntime
import org.harvestcircle.application.HarvestCircleStockRuntime
import org.harvestcircle.application.HubDispositionInput
import org.harvestcircle.application.HubDispositionReceipt
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubReceiptInput
import org.harvestcircle.application.HubReturnInput
import org.harvestcircle.application.HubReturnReceipt
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubStockReceipt
import org.harvestcircle.application.NativeSessionBinding
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.StockFixtures
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import org.harvestcircle.designsystem.theme.HarvestCircleTheme as Tokens

@OptIn(ExperimentalTestApi::class)
class CustodyWorkspaceTest {
    @Test
    fun largeTextThemesKeepLongUnknownFactsAndKeyboardFocus() =
        runDesktopComposeUiTest(width = 1280, height = 800) {
            val runtime = CustodyUiFake()
            lateinit var presenter: CustodyPresenter
            var theme by mutableStateOf(ThemePreference.Light)
            setContent {
                val scope = rememberCoroutineScope()
                presenter =
                    remember {
                        CustodyPresenter(
                            runtime,
                            runtime,
                            runtime,
                            runtime,
                            null,
                            scope,
                            OperationIdSource { error("No submission") },
                        )
                    }
                LaunchedEffect(Unit) {
                    presenter.attach("hub", NativeSessionBinding("01".repeat(16), 1UL))
                    presenter.beginReturn()
                }
                val state by presenter.state.collectAsState()
                HarvestCircleTheme(AppearanceState(theme, TextSizePreference.VeryLarge)) {
                    Box(Modifier.fillMaxSize().background(Tokens.foundation.colors.surface.canvas)) {
                        CustodyWorkspace(state, presenter) { error("Keyboard confirmation must not submit") }
                    }
                }
            }
            waitUntil { presenter.state.value.draft != null }
            runOnIdle {
                presenter.edit {
                    it.copy(
                        common =
                            it.common.copy(
                                quantity = "2",
                                physicalLabel = "R".repeat(100),
                                physicalObservation = "Distinct observed returned crates ".repeat(10),
                                observation = "Physical facts ".repeat(25),
                            ),
                    )
                }
            }
            onNodeWithTag("custody-quantity").performScrollTo().performClick().performKeyInput {
                keyDown(Key.Tab)
                keyUp(Key.Tab)
            }
            onNodeWithTag("custody-label").assertIsFocused()
            onNodeWithTag("custody-observation").performScrollTo().performClick().performKeyInput {
                keyDown(Key.Tab)
                keyUp(Key.Tab)
            }
            onNodeWithTag("custody-confirm").assertIsFocused().performKeyInput {
                keyDown(Key.Enter)
                keyUp(Key.Enter)
            }
            onNodeWithTag("custody-submit").assertIsEnabled()
            val light = onNodeWithTag("custody-workspace").captureToImage().toPixelMap()[0, 0].toArgb()
            runOnIdle { theme = ThemePreference.Dark }
            onNodeWithText("Unknown actual time remains unknown. Recording time is separate.").performScrollTo().assertIsDisplayed()
            val dark = onNodeWithTag("custody-workspace").captureToImage().toPixelMap()[0, 0].toArgb()
            assertNotEquals(light, dark)
            assertEquals(
                "R".repeat(100),
                presenter.state.value.draft
                    ?.fields
                    ?.common
                    ?.physicalLabel,
            )
            assertEquals(0, runtime.effects)
            runOnIdle { presenter.park() }
            assertFalse(requireNotNull(presenter.state.value.draft).fields.common.confirmed)
            runOnIdle { presenter.close() }
        }

    @Test
    fun minimumWindowKeepsBlockedStatesNonSubmitting() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val draft =
                CustodyDraft(CustodyContext(2UL, 5UL, StockFixtures.references), CustodyFields(common = CountFields(quantity = "2")))
            var shown by mutableStateOf(
                CustodyViewState(
                    available = true,
                    open = true,
                    target = CustodyTarget.Return,
                    draft = draft,
                    needsReview = true,
                    loading = true,
                    message = "Current context is loading",
                ),
            )
            var submissions = 0
            setContent {
                val scope = rememberCoroutineScope()
                val presenter =
                    remember { CustodyPresenter(null, null, null, null, null, scope, OperationIdSource { error("No operation") }) }
                HarvestCircleTheme(AppearanceState(ThemePreference.Dark, TextSizePreference.VeryLarge)) {
                    CustodyWorkspace(shown, presenter) { submissions++ }
                }
            }
            onNodeWithTag("custody-submit").assertIsNotEnabled()
            onNodeWithTag("custody-confirm").assertIsNotEnabled()
            runOnIdle { shown = shown.copy(draft = null, loading = false, message = "Current native context is unavailable") }
            onNodeWithTag("custody-retry-context").performScrollTo().assertIsDisplayed()
            onNodeWithTag("custody-submit").assertDoesNotExist()
            val attempt =
                CustodyAttempt(
                    StockFixtures.operation,
                    CustodyRequest.Return(
                        HubReturnInput(
                            2UL,
                            5UL,
                            StockFixtures.pack.id,
                            StockFixtures.receiving.id,
                            null,
                            HubSourceInput(),
                            2U,
                            HubPhysicalIdentification("Two", "Observed separate crates"),
                            null,
                            null,
                            HubOccurrence(HubOccurrenceKind.Unknown),
                            "Actually returned",
                        ),
                    ),
                )
            for ((code, text) in listOf(
                ApplicationErrorCode.HubPermissionDenied to "Current hub ownership is required",
                ApplicationErrorCode.HubConflict to "Current stock changed; review your input",
            )) {
                runOnIdle {
                    shown =
                        shown.copy(
                            draft = draft,
                            submission =
                                CustodySubmission.Rejected(
                                    attempt,
                                    ApplicationProblem(
                                        code,
                                        ApplicationErrorCategory.Conflict,
                                        false,
                                        RecoveryAction.Authenticate,
                                        attempt.operation,
                                        text,
                                    ),
                                ),
                        )
                }
                onNodeWithText(text).performScrollTo().assertIsDisplayed()
                onNodeWithTag("custody-review-rejected").performScrollTo().assertIsEnabled()
                onNodeWithTag("custody-submit").assertDoesNotExist()
            }
            runOnIdle { shown = shown.copy(submission = CustodySubmission.Uncertain(attempt, "Original outcome remains unknown")) }
            onNodeWithTag("custody-check-original").performScrollTo().assertIsEnabled()
            onNodeWithTag("custody-discard").assertDoesNotExist()
            onNodeWithTag("custody-submit").assertDoesNotExist()
            assertEquals(0, submissions)
        }
}

private class CustodyUiFake :
    HandlingFake(),
    HarvestCircleStockRuntime,
    HarvestCircleCustodyRuntime {
    var effects = 0

    override suspend fun stock(query: HubStockQuery) = StockFixtures.received

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt = error("No receipt")

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? = null

    override suspend fun applyReturn(
        context: RequestContext,
        input: HubReturnInput,
    ): HubReturnReceipt {
        effects++
        error("No native effect")
    }

    override suspend fun returnOperation(operation: OperationId): HubReturnReceipt? = null

    override suspend fun applyDisposition(
        context: RequestContext,
        input: HubDispositionInput,
    ): HubDispositionReceipt {
        effects++
        error("No native effect")
    }

    override suspend fun dispositionOperation(operation: OperationId): HubDispositionReceipt? = null
}
