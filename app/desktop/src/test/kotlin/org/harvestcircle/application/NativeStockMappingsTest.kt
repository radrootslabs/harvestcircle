package org.harvestcircle.application

import org.harvestcircle.ffi.HubHandlingBasisDto
import org.harvestcircle.ffi.HubLocationPurposeDto
import org.harvestcircle.ffi.HubOccurrenceDto
import org.harvestcircle.ffi.HubOccurrenceKindDto
import org.harvestcircle.ffi.HubQuantityWorkDto
import org.harvestcircle.ffi.HubReceiptTraceDto
import org.harvestcircle.ffi.HubReferenceInputDto
import org.harvestcircle.ffi.HubReferenceKindDto
import org.harvestcircle.ffi.HubReferenceRecordDto
import org.harvestcircle.ffi.HubSourceCertaintyDto
import org.harvestcircle.ffi.HubSourceDto
import org.harvestcircle.ffi.HubSourceKindDto
import org.harvestcircle.ffi.HubStockPageDto
import org.harvestcircle.ffi.HubStockPositionDto
import org.harvestcircle.ffi.HubStockReceiptDto
import org.harvestcircle.ffi.HubStockRowDto
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class NativeStockMappingsTest {
    @Test
    fun receiptInputAndQueryPreserveOriginalIntentAndBothCursorVersions() {
        val input =
            HubReceiptInput(
                7UL,
                11UL,
                "01".repeat(16),
                null,
                "02".repeat(16),
                HubSourceInput(
                    "03".repeat(16),
                    "04".repeat(16),
                    HubSourceKind.ProducerBatch,
                    "BATCH-01",
                    "Original label",
                    HubSourceCertainty.Possible,
                ),
                20U,
                "05".repeat(16),
                "06".repeat(16),
                HubOccurrence(HubOccurrenceKind.Interval, 80L, 90L),
                "Manually counted",
            )
        val native = input.toNative()
        assertEquals(7UL, native.expectedStockRevision)
        assertEquals(11UL, native.expectedCatalogRevision)
        assertNull(native.supplierId)
        assertEquals(input.producerId, native.producerId)
        assertEquals(input.source.existingSourceId, native.source.existingSourceId)
        assertEquals(HubSourceKindDto.PRODUCER_BATCH, native.source.kind)
        assertEquals(HubSourceCertaintyDto.POSSIBLE, native.source.certainty)
        assertEquals(HubOccurrenceDto(HubOccurrenceKindDto.INTERVAL, 80L, 90L), native.occurrence)
        assertEquals(20U, native.wholePacks)
        assertEquals(input.observation, native.observation)
        val query = HubStockQuery(11UL, 7UL, 64U, 64U, input.receivingId, "07".repeat(16), "BATCH").toNative()
        assertEquals(11UL, query.expectedCatalogRevision)
        assertEquals(7UL, query.expectedStockRevision)
        assertEquals(64U, query.offset)
        assertEquals("BATCH", query.text)
    }

    @Test
    fun nativePageKeepsCurrentNamesOriginalLabelsAndAuthoritativeWorkSeparate() {
        fun product(label: String) =
            HubReferenceRecordDto(
                "01".repeat(16),
                1UL,
                HubReferenceInputDto(HubReferenceKindDto.PRODUCT, label, null, null, null),
            )
        val original = product("Original apples")
        val current = product("Current apples").copy(revision = 2UL)
        val pack =
            HubReferenceRecordDto(
                "02".repeat(16),
                1UL,
                HubReferenceInputDto(HubReferenceKindDto.PACK, "Original crate", original.id, null, null),
            )
        val receiving =
            HubReferenceRecordDto(
                "03".repeat(16),
                1UL,
                HubReferenceInputDto(HubReferenceKindDto.LOCATION, "Receiving", null, null, HubLocationPurposeDto.RECEIVING),
            )
        val source = HubSourceDto(null, null, null, null, null, HubSourceCertaintyDto.UNKNOWN)
        val trace =
            HubReceiptTraceDto(
                "04".repeat(16),
                pack,
                original,
                null,
                null,
                source,
                true,
                20U,
                receiving,
                null,
                HubOccurrenceDto(HubOccurrenceKindDto.UNKNOWN, null, null),
                100L,
                "01".repeat(32),
                "Original observation",
            )
        val position = HubStockPositionDto("05".repeat(16), trace.id, pack.id, receiving.id, 12U, 3UL)
        val work = HubQuantityWorkDto(20U, 8U, 5U, 7U, 4UL, null)
        val page =
            HubStockPageDto(
                HubHandlingBasisDto("whole-pack-demo-v1", 1U, "Whole-pack demo", "UTC"),
                9UL,
                11UL,
                0U,
                1U,
                false,
                listOf(HubStockRowDto(current, position, receiving, trace, work)),
            ).toModel()
        val row = page.rows.single()
        assertEquals("Current apples", row.product.value.label)
        assertEquals("Original apples", row.trace.product.value.label)
        assertEquals(20U, row.trace.originalWholePacks)
        assertEquals(12U, row.position.wholePacks)
        assertEquals(8U, row.putAway.completed)
        assertEquals(5U, row.putAway.noLongerRequired)
        assertEquals(7U, row.putAway.remaining)
        assertEquals(HubSourceCertainty.Unknown, row.trace.source.certainty)
        assertNull(row.trace.source.id)
        assertNull(row.trace.occurrence.startUnixSeconds)
        assertEquals(100L, row.trace.recordedAt.value)
        val op = "01900000-0000-7000-8000-000000000031"
        val receipt =
            HubStockReceiptDto(
                op,
                2UL,
                trace,
                position.copy(wholePacks = 20U, revision = 1UL),
                work.copy(completed = 0U, noLongerRequired = 0U, remaining = 20U, revision = 1UL),
            ).toModel()
        assertEquals(OperationId.from(op), receipt.operationId)
        assertEquals(20U, receipt.initialPosition.wholePacks)
        assertEquals(0U, receipt.initialPutAway.completed)
    }
}
