package org.harvestcircle.integration

import org.harvestcircle.application.HubActualReportResolutionKind
import org.harvestcircle.application.HubCorrectionImpactKind
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubTraceHistoryKind
import org.harvestcircle.application.HubTraceQuantityAmendmentKind
import org.harvestcircle.application.toModel
import org.harvestcircle.testbridge.ffi.actualLinkInterpretationEncodingProbe
import org.harvestcircle.testbridge.ffi.actualReportResolutionEncodingProbe
import org.harvestcircle.testbridge.ffi.actualTraceEncodingProbe
import org.harvestcircle.testbridge.ffi.actualTraceHistoryEncodingProbe
import org.harvestcircle.testbridge.ffi.correctedTraceEncodingProbe
import org.harvestcircle.testbridge.ffi.recipientTraceEncodingProbe
import org.harvestcircle.testbridge.ffi.sourceTraceEncodingProbe
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

// This is an encoding/read-consumer fixture, not native admission or persisted
// effect acceptance. Real storage worlds qualify those independently.
class ActualTraceReadAcceptanceTest {
    @Test
    fun generatedLinkMeaningRetainsOriginalTwoEffectiveZeroAndUnknownWithoutRecipientFallback() {
        val native = actualLinkInterpretationEncodingProbe()
        val model = native.toModel()
        val meaning = assertNotNull(model.interpretation)
        assertEquals(2U, model.known.wholePacks)
        assertEquals(2U, meaning.originalWholePacks)
        assertEquals(0U, meaning.effectiveWholePacks)
        assertEquals(null, meaning.recipient)
        assertEquals(model.known.recipient, meaning.recipientHistory.single().originalRecipient)
        assertEquals(null, meaning.recipientHistory.single().after)
        assertEquals(0U, meaning.quantities.single().effectiveWholePacks)
        assertEquals(2U, meaning.quantities.single().originalWholePacks)
        assertEquals(native.interpretation?.stockRevision, meaning.stockRevision)
    }

    @Test
    fun generatedRecipientHistoryKeepsOriginalPartyUnknownMeaningAndManualScopeDistinct() {
        val original = actualTraceEncodingProbe().toModel()
        val native = recipientTraceEncodingProbe()
        val model = native.toModel()
        assertEquals(original.trace, model.trace)
        assertEquals(original.onHand, model.onHand)
        assertEquals(original.actualDepartures, model.actualDepartures)
        val correction = model.recipientCorrections.single()
        assertEquals(original.actualDepartures.single().recipient, correction.originalRecipient)
        assertEquals(correction.originalRecipient, correction.before)
        assertEquals(null, correction.after)
        assertEquals(original.actualDepartures.single().operationId, correction.originalDepartureOperationId)
        assertEquals(listOf(original.trace.id), correction.originalPopulationIds)
        assertEquals(emptyList(), correction.originalIssuedVersionIds)
        assertEquals(native.recipientCorrections.single().operationId, correction.operationId.value)
        assertEquals(model.stockRevision, correction.stockRevision)
        assertTrue(correction.impact.any { it.kind == HubCorrectionImpactKind.Recipient && it.targetId == correction.originalRecipient.id })
        assertTrue(
            correction.impact.any {
                it.kind == HubCorrectionImpactKind.UnresolvedRecipient &&
                    it.targetId == correction.originalDepartureOperationId.value.replace("-", "")
            },
        )
    }

    @Test
    fun generatedSourceHistoryKeepsOriginalPossibleInterpretationAndManualImpactDistinct() {
        val original = actualTraceEncodingProbe().toModel()
        val native = sourceTraceEncodingProbe()
        val model = native.toModel()
        assertEquals(original.trace, model.trace)
        assertEquals(original.onHand, model.onHand)
        assertEquals(original.actualDepartures, model.actualDepartures)
        val correction = model.sourceCorrections.single()
        assertEquals(HubSourceCertainty.Unknown, correction.before.certainty)
        assertEquals(HubSourceCertainty.Possible, correction.after.certainty)
        assertEquals("ENCODING-ONLY-POSSIBLE-SOURCE", correction.after.code)
        assertEquals(null, correction.after.issuer)
        assertEquals(model.stockRevision, correction.stockRevision)
        assertEquals(native.sourceCorrections.single().operationId, correction.operationId.value)
        assertTrue(correction.impact.any { it.kind == HubCorrectionImpactKind.UnresolvedSource && it.targetId == model.trace.id })
        assertTrue(correction.impact.any { it.kind == HubCorrectionImpactKind.Departure })
        assertTrue(correction.impact.any { it.kind == HubCorrectionImpactKind.Recipient })
    }

    @Test
    fun generatedReportResolutionKeepsUnresolvedActualReviewAndAdditionalCorrectionDistinct() {
        val native = actualReportResolutionEncodingProbe()
        val page = native.toModel()
        assertEquals(3U, page.total)
        assertEquals(
            listOf(
                null,
                HubActualReportResolutionKind.ActualReview,
                HubActualReportResolutionKind.AdditionalQuantity,
            ),
            page.rows.map {
                it.resolutionKind
            },
        )
        assertEquals(null, page.rows[0].reviewOperationId)
        assertEquals(native.rows[1].reviewOperationId, page.rows[1].reviewOperationId?.value)
        assertEquals(native.rows[2].reviewOperationId, page.rows[2].reviewOperationId?.value)
        assertNotEquals(page.rows[1].reviewOperationId, page.rows[2].reviewOperationId)
        assertEquals(page.rows[0].report, page.rows[2].report)
    }

    @Test
    fun generatedCorrectedTracePreservesGrossActualAndNativeEffectiveQuantities() {
        val original = actualTraceEncodingProbe().toModel()
        val native = correctedTraceEncodingProbe()
        val model = native.toModel()
        assertEquals(19U, model.onHandWholePacks)
        assertEquals(1U, model.actualDepartedWholePacks)
        assertEquals(original.actualDepartures, model.actualDepartures)
        val correction = model.quantityCorrections.single()
        assertEquals(model.actualDepartures.single().operationId, correction.originalOperationId)
        assertEquals(2U, correction.originalWholePacks)
        assertEquals(1U, correction.effectiveWholePacks)
        assertEquals(0U, correction.establishedReturnWholePacks)
        val amendment = correction.amendments.single()
        assertEquals(HubTraceQuantityAmendmentKind.IdentifiedFoundRemainder, amendment.kind)
        assertEquals(2U, amendment.beforeWholePacks)
        assertEquals(1U, amendment.afterWholePacks)
        assertEquals(1U, amendment.deltaWholePacks)
        assertEquals(native.stockRevision, amendment.stockRevision)
        assertNotEquals(correction.originalOperationId, amendment.operationId)
    }

    @Test
    fun generatedNativeActualPreservesOriginalAndReviewedRolesWithoutClientStockMath() {
        val native = actualTraceEncodingProbe()
        val model = native.toModel()
        assertEquals(20U, model.trace.originalWholePacks)
        assertEquals(18U, model.onHandWholePacks)
        assertEquals(0U, model.departedWholePacks)
        assertEquals(2U, model.actualDepartedWholePacks)
        assertTrue(model.exits.isEmpty())
        val actual = model.actualDepartures.single()
        assertEquals(HubOccurrenceKind.Interval, actual.reportedOccurrence.kind)
        assertEquals(140L, actual.reportedOccurrence.startUnixSeconds)
        assertEquals(160L, actual.reportedOccurrence.endUnixSeconds)
        assertEquals(150L, actual.reportRecordedAt.value)
        assertTrue(actual.reportedClockConflict)
        assertEquals(HubOccurrenceKind.Known, actual.occurrence.kind)
        assertEquals(145L, actual.occurrence.startUnixSeconds)
        assertEquals(190L, actual.recordedAt.value)
        assertNotEquals(actual.reportActor, actual.actor)
        assertEquals("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", actual.reportActor.value)
        assertEquals("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5", actual.actor.value)
        assertEquals("Historical A1", actual.historicalLocation.value.label)
        assertEquals("Current B2", actual.balancingLocation.value.label)
        assertNotEquals(actual.historicalLocation.id, actual.balancingLocation.id)
        assertEquals(20U, actual.before.wholePacks)
        assertEquals(18U, actual.onHandAfterWholePacks)
        assertEquals(155L, actual.afterCausalFloor?.value)
        val delta = actual.workChanges.single()
        assertEquals(20U, delta.before.quantity.remaining)
        assertEquals(18U, delta.after.quantity.remaining)
        assertEquals(2U, delta.retiredWholePacks)
        assertEquals(2U, delta.after.quantity.noLongerRequired)
        assertEquals("Original crossing-clock claim remains unchanged", actual.originalObservation)
        assertEquals("Verified recipient", actual.recipient.value.label)
        assertEquals("Historical cohort A verified", actual.sourceEvidence)
    }

    @Test
    fun generatedNativeHistoryKeepsReportReconciliationAndZeroEffectLinkDistinct() {
        val history = actualTraceHistoryEncodingProbe().toModel()
        assertEquals(5U, history.total)
        assertEquals(
            listOf(
                HubTraceHistoryKind.ActualReported,
                HubTraceHistoryKind.ActualReconciled,
                HubTraceHistoryKind.ActualLinked,
                HubTraceHistoryKind.FoundCorrected,
                HubTraceHistoryKind.AdditionalCorrected,
            ),
            history.rows.map {
                it.kind
            },
        )
        assertNotEquals(history.rows[0].actor, history.rows[1].actor)
        assertEquals(history.rows[1].actor, history.rows[2].actor)
    }
}
