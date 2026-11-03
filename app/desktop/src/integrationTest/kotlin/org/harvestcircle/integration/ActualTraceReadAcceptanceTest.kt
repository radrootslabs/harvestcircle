package org.harvestcircle.integration

import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubTraceHistoryKind
import org.harvestcircle.application.toModel
import org.harvestcircle.testbridge.ffi.actualTraceEncodingProbe
import org.harvestcircle.testbridge.ffi.actualTraceHistoryEncodingProbe
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

// This is an encoding/read-consumer fixture, not native admission or persisted
// effect acceptance. Real storage worlds qualify those independently.
class ActualTraceReadAcceptanceTest {
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
        assertEquals(3U, history.total)
        assertEquals(
            listOf(
                HubTraceHistoryKind.ActualReported,
                HubTraceHistoryKind.ActualReconciled,
                HubTraceHistoryKind.ActualLinked,
            ),
            history.rows.map {
                it.kind
            },
        )
        assertNotEquals(history.rows[0].actor, history.rows[1].actor)
        assertEquals(history.rows[1].actor, history.rows[2].actor)
    }
}
