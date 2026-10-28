package org.harvestcircle.ui.shell

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HarvestCircleReferenceRuntime
import org.harvestcircle.application.HubHandlingBasis
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferencePage
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceReceipt
import org.harvestcircle.application.HubReferenceRecord
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.ReferenceAttempt
import org.harvestcircle.application.ReferencePresenter
import org.harvestcircle.application.ReferenceSubmission
import org.harvestcircle.application.ReferenceViewState
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.UnixSeconds
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class ReferenceWorkspaceTest {
    @Test
    fun boundedEditorsSubmitEachKindThroughTheRuntimePort() {
        HubReferenceKind.entries.forEach { kind ->
            runComposeUiTest {
                val runtime = EditorRuntime()
                lateinit var presenter: ReferencePresenter
                setHarvestCircleContent {
                    val scope = rememberCoroutineScope()
                    presenter =
                        androidx.compose.runtime.remember { ReferencePresenter(runtime, scope, OperationIdSource { editorOperation }) }
                    val state by presenter.state.collectAsState()
                    LaunchedEffect(Unit) { presenter.attach("hub") }
                    ReferenceWorkspace(state, presenter, { presenter.submit(SnapshotRevision(7UL)) }, {})
                }
                waitUntil { presenter.state.value.page != null }
                onNodeWithTag("reference-kind-${kind.name}").performClick()
                waitUntil { !presenter.state.value.loading }
                onNodeWithTag("reference-add").performClick()
                onNodeWithTag("reference-label").performTextInput("Fictional ${kind.name}")
                if (kind == HubReferenceKind.Pack) {
                    waitUntil { presenter.state.value.products != null }
                    onNodeWithTag("reference-product-${editorProduct.id}").performScrollTo().performClick()
                    onNodeWithTag("reference-grams").performScrollTo().performTextInput("1200")
                }
                onNodeWithTag("reference-save").performScrollTo().assertIsEnabled().performClick()
                waitUntil { presenter.state.value.submission is ReferenceSubmission.Recorded }
                onNodeWithTag("reference-recorded").assertIsDisplayed()
                assertEquals(
                    kind,
                    runtime.edits
                        .single()
                        .value.kind,
                )
                assertEquals(
                    "Fictional ${kind.name}",
                    runtime.edits
                        .single()
                        .value.label,
                )
                if (kind == HubReferenceKind.Pack) {
                    assertEquals(
                        editorProduct.id,
                        runtime.edits
                            .single()
                            .value.productId,
                    )
                    assertEquals(
                        1200U,
                        runtime.edits
                            .single()
                            .value.nominalGrams,
                    )
                }
                runOnIdle { presenter.close() }
            }
        }
    }

    @Test
    fun uncertainSubmissionOffersOriginalResultLookupAndNoNewSave() =
        runComposeUiTest {
            val attempt = ReferenceAttempt(editorOperation, HubReferenceEdit(1UL, value = editorProduct.value))
            setHarvestCircleContent {
                ReferenceWorkspace(
                    ReferenceViewState(
                        available = true,
                        submission = ReferenceSubmission.Uncertain(attempt, "Outcome unconfirmed"),
                    ),
                    ReferencePresenter(null, rememberCoroutineScope(), OperationIdSource { editorOperation }),
                    {},
                    {},
                )
            }
            onNodeWithText("Original operation: ${editorOperation.value}").assertExists()
            onNodeWithTag("reference-check-original").assertIsEnabled()
            onNodeWithTag("reference-save").assertDoesNotExist()
            onNodeWithTag("reference-add").assertDoesNotExist()
        }
}

private class EditorRuntime : HarvestCircleReferenceRuntime {
    val edits = mutableListOf<HubReferenceEdit>()

    override suspend fun references(query: HubReferenceQuery) =
        HubReferencePage(
            HubHandlingBasis("whole-pack-demo-v1", 1U, "Whole-pack demo", "UTC"),
            1UL,
            0U,
            1U,
            false,
            if (query.kind == HubReferenceKind.Product) listOf(editorProduct) else emptyList(),
        )

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt {
        edits += edit
        return HubReferenceReceipt(
            context.operationId,
            IdentityId.fromPublicKeyHex("01".repeat(32)),
            UnixSeconds(100L),
            2UL,
            null,
            editorProduct.copy(value = edit.value),
        )
    }

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? = null

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> = emptyList()
}

private val editorOperation = OperationId.from("01900000-0000-7000-8000-000000000022")
private val editorProduct = HubReferenceRecord("01".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Product, "Fictional carrots"))
