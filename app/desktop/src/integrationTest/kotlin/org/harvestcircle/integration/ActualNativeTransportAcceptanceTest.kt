package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubActualClockAssessment
import org.harvestcircle.application.HubActualDepartureInput
import org.harvestcircle.application.HubActualHoldAssessment
import org.harvestcircle.application.HubActualHoldTiming
import org.harvestcircle.application.HubActualHoldUncertainty
import org.harvestcircle.application.HubActualLinkInput
import org.harvestcircle.application.HubActualReportInput
import org.harvestcircle.application.HubActualReportQuery
import org.harvestcircle.application.HubActualReviewOriginal
import org.harvestcircle.application.HubActualSourceClaim
import org.harvestcircle.application.HubActualSourceKind
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTreatmentScope
import org.harvestcircle.application.toModel
import org.harvestcircle.ffi.HubActualClockAssessmentDto
import org.harvestcircle.ffi.HubActualDepartureInputDto
import org.harvestcircle.ffi.HubActualHoldAssessmentDto
import org.harvestcircle.ffi.HubActualHoldTimingDto
import org.harvestcircle.ffi.HubActualHoldUncertaintyDto
import org.harvestcircle.ffi.HubActualLinkInputDto
import org.harvestcircle.ffi.HubActualReportFilterDto
import org.harvestcircle.ffi.HubActualReportInputDto
import org.harvestcircle.ffi.HubActualReportQueryDto
import org.harvestcircle.ffi.HubActualReviewOriginalDto
import org.harvestcircle.ffi.HubActualSourceKindDto
import org.harvestcircle.ffi.HubOccurrenceDto
import org.harvestcircle.ffi.HubOccurrenceKindDto
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

// Generated DTOs cross the real native actor/SQLite boundary here. Production
// exported methods have separate Rust tests; product forms are qualified later.
class ActualNativeTransportAcceptanceTest {
    @Test
    fun generatedActualTransportPreservesRawFactsNativeHoldScopeAndOriginalsAcrossRestart() =
        runBlocking {
            for (separateCohort in listOf(false, true)) {
                val root = Files.createTempDirectory("harvestcircle-actual-native-transport-")
                val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
                try {
                    val hub = HandlingFixture.open(runtime, HubOccurrence(HubOccurrenceKind.Known, 1_699_999_940L))
                    val groupId = hub.receipt.initialPosition.groupId
                    if (separateCohort) {
                        val group = runtime.group(groupId)
                        runtime.applyHandling(
                            hub.context(),
                            HubHandlingInput.Split(
                                group.stockRevision,
                                group.catalogRevision,
                                groupId,
                                group.group.revision,
                                6U,
                                HubPhysicalIdentification("Separate six", "Six fictional crates separated before the hold"),
                                HubOccurrence(HubOccurrenceKind.Known, 1_699_999_950L),
                                "Recorded physical separation",
                            ),
                        )
                    }
                    val catalog = runtime.references(HubReferenceQuery())
                    val recipient =
                        runtime
                            .editReference(
                                hub.context(),
                                HubReferenceEdit(
                                    catalog.catalogRevision,
                                    value = HubReferenceValue(HubReferenceKind.Party, "Fictional actual recipient"),
                                ),
                            ).current
                    val groupBeforeHold = runtime.group(groupId)
                    runtime.applyHandling(
                        hub.context(),
                        HubHandlingInput.PlaceHold(
                            groupBeforeHold.stockRevision,
                            groupBeforeHold.catalogRevision,
                            listOf(groupId),
                            HubOccurrence(HubOccurrenceKind.Known, 1_699_999_970L),
                            "Current restriction stays in force",
                        ),
                    )
                    val heldBefore = runtime.treatment(HubTreatmentScope(hub.receipt.trace.id))
                    val raw =
                        HubActualReportInputDto(
                            null,
                            null,
                            null,
                            HubActualSourceKindDto.UNKNOWN,
                            emptyList(),
                            unknown(),
                            "Already-taken report with fields still unverified",
                        )
                    val reportContext = hub.context()
                    val saved = runtime.reportActualNative(reportContext, raw)
                    assertEquals(raw, saved.reported)
                    assertEquals(HubActualClockAssessmentDto.UNKNOWN, saved.clockAssessment)
                    assertEquals(1U, runtime.actualReportsNative(reports()).total)
                    val before = runtime.stock(HubStockQuery())
                    assertEquals(20U, before.rows.sumOf { it.position.wholePacks })
                    val current = runtime.group(groupId)
                    val input =
                        HubActualDepartureInputDto(
                            saved.id,
                            current.stockRevision,
                            current.catalogRevision,
                            groupId,
                            current.group.revision,
                            2U,
                            recipient.id,
                            known(1_699_999_990L),
                            hub.receipt.operationId.value,
                            groupId,
                            "Original cohort verified",
                            "Two whole crates verified",
                            "Actual recipient confirmed",
                            "Physical occurrence verified separately",
                            "Complete native departure history reviewed",
                        )
                    val reviewContext = hub.context()
                    val preview = runtime.actualDeparturePreviewNative(reviewContext.operationId, input)
                    val remaining = if (separateCohort) 12U else 18U
                    assertEquals(saved, preview.report)
                    assertNull(preview.report.reported.wholePacks)
                    assertNull(preview.report.reported.recipientId)
                    assertEquals(2U, preview.proposed.wholePacks)
                    assertEquals(recipient.id, preview.proposed.recipient.id)
                    assertEquals(remaining, preview.proposed.onHandAfterWholePacks)
                    val model = preview.toModel()
                    assertEquals(saved.toModel(), model.report)
                    assertEquals(hub.receipt.operationId, model.proposed.sourceOperationId)
                    if (separateCohort) {
                        val unresolved = assertIs<HubActualHoldAssessment.Unresolved>(model.historicalHolds.single())
                        assertEquals(HubActualHoldTiming.EstablishedBreach, unresolved.conditionalTiming)
                        assertEquals(HubActualHoldUncertainty.AnchorCohortUnverified, unresolved.reason)
                    } else {
                        assertEquals(
                            HubActualHoldTiming.EstablishedBreach,
                            assertIs<HubActualHoldAssessment.Verified>(model.historicalHolds.single()).timing,
                        )
                    }
                    val finding = preview.historicalHolds.single()
                    if (separateCohort) {
                        val unresolved = assertIs<HubActualHoldAssessmentDto.Unresolved>(finding)
                        assertEquals(HubActualHoldTimingDto.ESTABLISHED_BREACH, unresolved.conditionalTiming)
                        assertEquals(HubActualHoldUncertaintyDto.ANCHOR_COHORT_UNVERIFIED, unresolved.reason)
                    } else {
                        assertEquals(
                            HubActualHoldTimingDto.ESTABLISHED_BREACH,
                            assertIs<HubActualHoldAssessmentDto.Verified>(finding).timing,
                        )
                    }
                    assertEquals(before, runtime.stock(HubStockQuery()))
                    assertNull(runtime.actualReviewOperationNative(reviewContext.operationId))
                    val committed = runtime.reconcileActualNative(reviewContext, input)
                    val physical = assertIs<HubActualReviewOriginalDto.Departure>(committed.original)
                    assertEquals(saved, physical.report)
                    assertEquals(preview.historicalHolds, physical.historicalHolds)
                    assertEquals(remaining, physical.original.onHandAfterWholePacks)
                    assertEquals(
                        2U,
                        physical.original.workChanges
                            .single()
                            .retiredWholePacks,
                    )
                    val replayContext = reviewContext.copy(expectedRevision = runtime.currentSnapshot().revision)
                    assertEquals(committed, runtime.reconcileActualNative(replayContext, input))
                    val after = runtime.stock(HubStockQuery())
                    assertEquals(18U, after.rows.sumOf { it.position.wholePacks })
                    val work =
                        runtime
                            .work(HubHandlingQuery(openOnly = false))
                            .rows
                            .single()
                            .work.quantity
                    assertEquals(0U, work.completed)
                    assertEquals(2U, work.noLongerRequired)
                    assertEquals(18U, work.remaining)
                    val heldAfter = runtime.treatment(HubTreatmentScope(hub.receipt.trace.id))
                    assertEquals(heldBefore.holds.single().hold, heldAfter.holds.single().hold)
                    assertEquals(remaining, heldAfter.holds.single().currentWholePacks)
                    assertEquals(
                        1,
                        heldAfter.groups
                            .single { it.groupId == groupId }
                            .activeHoldIds.size,
                    )
                    if (separateCohort) {
                        val sibling = heldAfter.groups.single { it.groupId != groupId }
                        assertEquals(6U, sibling.wholePacks)
                        assertTrue(sibling.activeHoldIds.isEmpty())
                    }
                    val duplicate = runtime.reportActualNative(hub.context(), raw)
                    val linkContext = hub.context()
                    val linkInput =
                        HubActualLinkInputDto(
                            duplicate.id,
                            reviewContext.operationId.value,
                            hub.receipt.trace.id,
                            current.variant.id,
                            "Explicitly the same already-recorded two-crate event",
                        )
                    val linkPreview = runtime.actualLinkPreviewNative(linkContext.operationId, linkInput)
                    assertEquals(duplicate, linkPreview.proposed.report)
                    assertEquals(2U, linkPreview.proposed.known.wholePacks)
                    val meaning = assertNotNull(linkPreview.proposed.interpretation)
                    assertEquals(2U, meaning.originalWholePacks)
                    assertEquals(2U, meaning.effectiveWholePacks)
                    assertEquals(linkPreview.proposed.known.recipient, meaning.recipient)
                    assertEquals(1, meaning.quantities.size)
                    assertTrue(meaning.recipientHistory.isEmpty())
                    val linked = runtime.linkActualNative(linkContext, linkInput)
                    assertIs<HubActualReviewOriginalDto.Duplicate>(linked.original)
                    assertEquals(after.rows, runtime.stock(HubStockQuery()).rows)
                    assertEquals(0U, runtime.actualReportsNative(reports()).total)
                    val futureContext = hub.context()
                    val futureInput = raw.copy(occurrence = known(1_700_003_600L))
                    val future = runtime.reportActualNative(futureContext, futureInput)
                    assertEquals(HubActualClockAssessmentDto.RECORDING_CLOCK_CONFLICT, future.clockAssessment)
                    assertEquals(futureInput, future.reported)
                    assertEquals(HubActualClockAssessment.RecordingClockConflict, future.toModel().clockAssessment)
                    assertEquals(after.rows, runtime.stock(HubStockQuery()).rows)
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    assertEquals(saved, runtime.actualReportOperationNative(reportContext.operationId))
                    assertEquals(committed, runtime.actualReviewOperationNative(reviewContext.operationId))
                    assertEquals(linked, runtime.actualReviewOperationNative(linkContext.operationId))
                    assertEquals(future, runtime.actualReportOperationNative(futureContext.operationId))
                    assertEquals(
                        future,
                        runtime
                            .actualReportsNative(reports())
                            .rows
                            .single()
                            .report,
                    )
                    assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
                    runtime.execute(ApplicationCommand.SignOut)
                    assertEquals(
                        ApplicationErrorCode.HubPermissionDenied,
                        assertFailsWith<ApplicationFailure> {
                            runtime.actualReportsNative(reports())
                        }.problem.code,
                    )
                    assertEquals(
                        ApplicationErrorCode.HubPermissionDenied,
                        assertFailsWith<ApplicationFailure> {
                            runtime.actualReviewOperationNative(reviewContext.operationId)
                        }.problem.code,
                    )
                } finally {
                    runtime.close()
                    Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
                }
            }
        }

    @Test
    fun sharedActualAdapterPreservesReportedClaimsAndCopiesNativeEffectsWithoutClientMath() =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-actual-model-adapter-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime, HubOccurrence(HubOccurrenceKind.Known, 1_699_999_940L))
                val catalog = runtime.references(HubReferenceQuery())
                val recipient =
                    runtime
                        .editReference(
                            hub.context(),
                            HubReferenceEdit(
                                catalog.catalogRevision,
                                value = HubReferenceValue(HubReferenceKind.Party, "Fictional recipient"),
                            ),
                        ).current
                val claim = HubActualSourceClaim(hub.receipt.trace.id, hub.receipt.initialPosition.groupId, hub.receiving)
                val input =
                    HubActualReportInput(
                        sourceKind = HubActualSourceKind.Reported,
                        sourceClaims = listOf(claim),
                        observation = "A reported source is a claim requiring independent review",
                    )
                val reportContext = hub.context()
                val saved = runtime.reportActual(reportContext, input)
                assertEquals(input, saved.reported)
                assertEquals(
                    saved,
                    runtime
                        .actualReports(HubActualReportQuery())
                        .rows
                        .single()
                        .report,
                )
                val group = runtime.group(hub.receipt.initialPosition.groupId)
                val reviewInput =
                    HubActualDepartureInput(
                        saved.id,
                        group.stockRevision,
                        group.catalogRevision,
                        group.group.id,
                        group.group.revision,
                        2U,
                        recipient.id,
                        HubOccurrence(HubOccurrenceKind.Known, 1_699_999_990L),
                        hub.receipt.operationId,
                        group.group.id,
                        "Original source cohort verified",
                        "Two packs verified",
                        "Recipient independently verified",
                        "Occurrence independently verified",
                        "Complete native history checked",
                    )
                val reviewContext = hub.context()
                val preview = runtime.actualDeparturePreview(reviewContext.operationId, reviewInput)
                assertEquals(saved, preview.report)
                assertEquals(18U, preview.proposed.onHandAfterWholePacks)
                assertEquals(20U, runtime.group(group.group.id).group.wholePacks)
                val committed = runtime.reconcileActual(reviewContext, reviewInput)
                val original = assertIs<HubActualReviewOriginal.Departure>(committed.original)
                assertEquals(saved, original.report)
                assertEquals(input.sourceClaims, original.report.reported.sourceClaims)
                assertEquals(18U, original.original.onHandAfterWholePacks)
                assertEquals(
                    2U,
                    original.original.workChanges
                        .single()
                        .retiredWholePacks,
                )
                val duplicate = runtime.reportActual(hub.context(), input)
                val linkContext = hub.context()
                val link =
                    HubActualLinkInput(
                        duplicate.id,
                        reviewContext.operationId,
                        hub.receipt.trace.id,
                        group.variant.id,
                        "Same native original departure independently verified",
                    )
                val linkedPreview = runtime.actualLinkPreview(linkContext.operationId, link)
                assertEquals(2U, linkedPreview.proposed.known.wholePacks)
                val meaning = assertNotNull(linkedPreview.proposed.interpretation)
                assertEquals(2U, meaning.originalWholePacks)
                assertEquals(2U, meaning.effectiveWholePacks)
                assertEquals(linkedPreview.proposed.known.recipient, meaning.recipient)
                assertEquals(1, meaning.quantities.size)
                val linked = runtime.linkActual(linkContext, link)
                assertEquals(linkedPreview.proposed, assertIs<HubActualReviewOriginal.Duplicate>(linked.original).original)
                assertEquals(18U, runtime.group(group.group.id).group.wholePacks)
                assertEquals(0U, runtime.actualReports(HubActualReportQuery()).total)
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(saved, runtime.actualReportOperation(reportContext.operationId))
                assertEquals(committed, runtime.actualReviewOperation(reviewContext.operationId))
                assertEquals(linked, runtime.actualReviewOperation(linkContext.operationId))
                assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    private fun unknown() = HubOccurrenceDto(HubOccurrenceKindDto.UNKNOWN, null, null)

    private fun known(at: Long) = HubOccurrenceDto(HubOccurrenceKindDto.KNOWN, at, null)

    private fun reports() = HubActualReportQueryDto(null, null, 0U, 64U, HubActualReportFilterDto.UNRESOLVED)
}
