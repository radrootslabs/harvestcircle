package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.DisposableEffect
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
import org.harvestcircle.application.ActualAttempt
import org.harvestcircle.application.ActualDraft
import org.harvestcircle.application.ActualFake
import org.harvestcircle.application.ActualFields
import org.harvestcircle.application.ActualPresenter
import org.harvestcircle.application.ActualRequest
import org.harvestcircle.application.ActualResult
import org.harvestcircle.application.ActualSubmission
import org.harvestcircle.application.ActualTarget
import org.harvestcircle.application.ActualViewState
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.HandlingAction
import org.harvestcircle.application.HandlingFake
import org.harvestcircle.application.HandlingFixtures
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingSubmission
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HubActualReportInput
import org.harvestcircle.application.NativeSessionBinding
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.StockFixtures
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.navigation.AppRoute
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import org.harvestcircle.designsystem.theme.HarvestCircleTheme as Tokens

@OptIn(ExperimentalTestApi::class)
class ActualWorkspaceTest {
    @Test
    fun minimumWindowKeepsUnknownFactsKeyboardAndDraftAcrossWorkStock() = renderWorkspace(1100, 720)

    @Test
    fun initialWindowKeepsLargeTextAndThemeAcrossWorkStock() = renderWorkspace(1280, 800)

    @Test
    fun loadingPermissionConflictAndUnknownKeepTheOriginalAndBlockSubmission() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val draft = ActualDraft(ActualTarget.Report, fields = ActualFields(observation = "Unverified facts"))
            val attempt =
                ActualAttempt(StockFixtures.operation, ActualRequest.Report(HubActualReportInput(observation = "Unverified facts")))
            var shown by mutableStateOf(ActualViewState(available = true, open = true, loading = true, draft = draft))
            var submissions = 0
            setContent {
                val scope = rememberCoroutineScope()
                val presenter =
                    remember { ActualPresenter(null, null, null, null, null, scope, OperationIdSource { error("No operation") }) }
                HarvestCircleTheme(AppearanceState(ThemePreference.Dark, TextSizePreference.VeryLarge)) {
                    ActualWorkspace(shown, presenter) { submissions++ }
                }
            }
            onNodeWithTag("actual-confirm").assertIsNotEnabled()
            onNodeWithTag("actual-submit").assertIsNotEnabled()
            runOnIdle { shown = shown.copy(loading = false, submission = ActualSubmission.Pending(attempt)) }
            onNodeWithTag("actual-submit").assertDoesNotExist()
            onNodeWithTag("actual-check-original").assertDoesNotExist()
            onNodeWithTag("actual-operation").assertExists()
            for ((code, text) in listOf(
                ApplicationErrorCode.HubPermissionDenied to "Current owner required",
                ApplicationErrorCode.HubConflict to "Current context changed",
            )) {
                runOnIdle {
                    shown =
                        shown.copy(
                            submission =
                                ActualSubmission.Rejected(
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
                onNodeWithTag("actual-review-rejected").performScrollTo().assertIsEnabled()
                onNodeWithTag("actual-submit").assertDoesNotExist()
            }
            runOnIdle { shown = shown.copy(submission = ActualSubmission.Uncertain(attempt, "Outcome still unknown")) }
            onNodeWithTag("actual-check-original").performScrollTo().assertIsEnabled()
            onNodeWithTag("actual-new-report").assertDoesNotExist()
            onNodeWithTag("actual-submit").assertDoesNotExist()
            assertEquals(0, submissions)
        }

    @Test
    fun unrelatedUnknownHandlingCannotBlockSavingAnUnverifiedReportOrLoseItsAttempt() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val runtime = ActualFake()
            val other = HandlingFake()
            lateinit var presenter: ActualPresenter
            lateinit var handling: HandlingPresenter
            setContent {
                val scope = rememberCoroutineScope()
                presenter =
                    remember {
                        ActualPresenter(
                            runtime,
                            null,
                            null,
                            null,
                            null,
                            scope,
                            OperationIdSource {
                                org.harvestcircle.application.OperationId
                                    .from("01900000-0000-7000-8000-000000000032")
                            },
                            onFinished = { handling.refresh() },
                        )
                    }
                val stock = remember { StockPresenter(null, null, scope, OperationIdSource { error("No receipt") }) }
                handling = remember { HandlingPresenter(other, other, scope, OperationIdSource { StockFixtures.operation }) }
                DisposableEffect(Unit) {
                    onDispose {
                        presenter.close()
                        handling.close()
                        stock.close()
                    }
                }
                LaunchedEffect(Unit) {
                    presenter.attach("hub", NativeSessionBinding("01".repeat(16), 1UL))
                    handling.attach("hub")
                }
                val actual by presenter.state.collectAsState()
                val handlingState by handling.state.collectAsState()
                HarvestCircleTheme(AppearanceState()) {
                    OperationalWorkspace(AppRoute.HubWork, StockViewState(available = true, page = StockFixtures.received), stock, {
                    }, handlingState, handling, {
                    }, {}, {}, actualPresenter = presenter, actualState = actual, submitActual = {
                        presenter.submit(
                            SnapshotRevision(7UL),
                        )
                    })
                }
            }
            waitUntil { handling.state.value.work != null }
            runOnIdle { handling.inspectGroup(HandlingFixtures.group.group.id) }
            waitUntil { handling.state.value.group != null && !handling.state.value.loading }
            runOnIdle {
                handling.begin(HandlingAction.Split)
                handling.edit(HandlingFixtures.fields.copy(targetId = null))
                handling.submit(SnapshotRevision(7UL))
            }
            waitUntil { handling.state.value.submission is HandlingSubmission.Uncertain }
            val retained = assertIs<HandlingSubmission.Uncertain>(handling.state.value.submission)
            onNodeWithTag("operations-actual-reports").assertIsEnabled().performClick()
            waitUntil { presenter.state.value.page != null && !presenter.state.value.loading }
            onNodeWithTag("actual-new-report").performScrollTo().performClick()
            runOnIdle { presenter.edit { it.copy(observation = "Unverified incoming report while an unrelated result is unknown") } }
            onNodeWithTag("actual-confirm").performScrollTo().performClick()
            onNodeWithTag("actual-submit").performScrollTo().performClick()
            waitUntil { presenter.state.value.submission is ActualSubmission.Recorded }
            val saved = assertIs<ActualResult.Reported>(assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).result).value
            assertEquals(null, saved.reported.wholePacks)
            assertTrue(saved.operationId != retained.attempt.operation)
            assertEquals(retained, handling.state.value.submission)
            assertEquals(1, other.writes.size)
            assertEquals(1, runtime.writes)
            onNodeWithTag("actual-done").performScrollTo().performClick()
            waitUntil { !handling.state.value.loading && presenter.state.value.page != null }
            assertEquals(retained, handling.state.value.submission)
            assertEquals(1, other.writes.size)
        }

    private fun renderWorkspace(
        width: Int,
        height: Int,
    ) = runDesktopComposeUiTest(width = width, height = height) {
        val runtime = ActualFake()
        lateinit var presenter: ActualPresenter
        var theme by mutableStateOf(ThemePreference.Light)
        var route by mutableStateOf<AppRoute>(AppRoute.HubStock)
        val note = "Unverified report; source, quantity and recipient unknown. ".repeat(12)
        setContent {
            val scope = rememberCoroutineScope()
            val ids = OperationIdSource { error("No effect is authorized by keyboard confirmation") }
            presenter = remember { ActualPresenter(runtime, null, null, null, null, scope, ids) }
            val stock = remember { StockPresenter(null, null, scope, ids) }
            val handling = remember { HandlingPresenter(null, null, scope, ids) }
            DisposableEffect(Unit) {
                onDispose {
                    presenter.close()
                    stock.close()
                    handling.close()
                }
            }
            LaunchedEffect(Unit) { presenter.attach("hub", NativeSessionBinding("01".repeat(16), 1UL)) }
            val actual by presenter.state.collectAsState()
            HarvestCircleTheme(AppearanceState(theme, TextSizePreference.VeryLarge)) {
                Box(Modifier.fillMaxSize().background(Tokens.foundation.colors.surface.canvas)) {
                    OperationalWorkspace(
                        route,
                        StockViewState(available = true, page = StockFixtures.received),
                        stock,
                        {},
                        HandlingViewState(available = true),
                        handling,
                        {},
                        {},
                        {},
                        actualPresenter = presenter,
                        actualState = actual,
                        submitActual = { error("Confirmation must not submit") },
                    )
                }
            }
        }
        waitUntil { presenter.state.value.available }
        onNodeWithTag("operations-actual-reports").performClick()
        waitUntil { presenter.state.value.page != null && !presenter.state.value.loading }
        onNodeWithTag("actual-new-report").performScrollTo().performClick()
        runOnIdle { presenter.edit { it.copy(observation = note) } }
        onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
        onNodeWithTag("actual-observation").performScrollTo().performClick().performKeyInput {
            keyDown(Key.Tab)
            keyUp(Key.Tab)
        }
        onNodeWithTag("actual-confirm").assertIsFocused().performKeyInput {
            keyDown(Key.Enter)
            keyUp(Key.Enter)
        }
        onNodeWithTag("actual-submit").assertIsEnabled()
        val light = onNodeWithTag("actual-workspace").captureToImage().toPixelMap()[0, 0].toArgb()
        runOnIdle { theme = ThemePreference.Dark }
        onNodeWithText(
            "Recording time is separate. Unknown time stays unknown; a future report claim is retained as a clock conflict.",
        ).performScrollTo().assertIsDisplayed()
        val dark = onNodeWithTag("actual-workspace").captureToImage().toPixelMap()[0, 0].toArgb()
        assertNotEquals(light, dark)
        val bounds = onNodeWithTag("actual-workspace").fetchSemanticsNode().boundsInRoot
        assertTrue(bounds.right <= width.toFloat() && bounds.bottom <= height.toFloat())
        onNodeWithTag("actual-park").performScrollTo().performClick()
        assertFalse(presenter.state.value.open)
        assertFalse(assertNotNull(presenter.state.value.draft).fields.confirmed)
        runOnIdle { route = AppRoute.HubWork }
        onNodeWithTag("operations-actual-reports").performClick()
        waitUntil { presenter.state.value.open }
        assertEquals(
            note,
            presenter.state.value.draft
                ?.fields
                ?.observation,
        )
        onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
        assertEquals(0, runtime.writes)
    }
}
