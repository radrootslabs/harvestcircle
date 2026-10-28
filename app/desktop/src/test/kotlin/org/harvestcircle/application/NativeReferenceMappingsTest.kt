package org.harvestcircle.application

import org.harvestcircle.ffi.HubHandlingBasisDto
import org.harvestcircle.ffi.HubLocationPurposeDto
import org.harvestcircle.ffi.HubReferenceInputDto
import org.harvestcircle.ffi.HubReferenceKindDto
import org.harvestcircle.ffi.HubReferencePageDto
import org.harvestcircle.ffi.HubReferenceReceiptDto
import org.harvestcircle.ffi.HubReferenceRecordDto
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class NativeReferenceMappingsTest {
    @Test
    fun allReferenceKindsPreserveNativeFieldsWithoutInventingUnknownMass() {
        val values =
            listOf(
                HubReferenceInputDto(HubReferenceKindDto.PRODUCT, "Carottes 🥕", null, null, null),
                HubReferenceInputDto(HubReferenceKindDto.PACK, "Original crate", "02".repeat(16), null, null),
                HubReferenceInputDto(HubReferenceKindDto.PACK, "Nominal crate", "02".repeat(16), 1200U, null),
                HubReferenceInputDto(HubReferenceKindDto.PARTY, "Fictional producer", null, null, null),
            ) + HubLocationPurposeDto.entries.map { HubReferenceInputDto(HubReferenceKindDto.LOCATION, it.name, null, null, it) }
        values.forEach { value ->
            val record = HubReferenceRecordDto("01".repeat(16), 3UL, value).toModel()
            val edit = HubReferenceEdit(9UL, record.id, record.revision, record.value).toNative()
            assertEquals(value, edit.value)
            assertEquals(9UL, edit.expectedCatalogRevision)
            assertEquals(3UL, edit.expectedRecordRevision)
            assertEquals(record.id, edit.recordId)
        }
    }

    @Test
    fun pagesAndReceiptsPreserveHistoricalIdentityVersionsAndBasis() {
        val previous =
            HubReferenceRecordDto(
                "01".repeat(16),
                1UL,
                HubReferenceInputDto(HubReferenceKindDto.PRODUCT, "Old label", null, null, null),
            )
        val current = previous.copy(revision = 2UL, value = previous.value.copy(label = "New label"))
        val page =
            HubReferencePageDto(
                HubHandlingBasisDto("whole-pack-demo-v1", 1U, "Whole-pack demo", "UTC"),
                19UL,
                64U,
                129U,
                true,
                listOf(current),
            ).toModel()
        assertEquals("UTC", page.handlingBasis.siteTimezone)
        assertEquals(19UL, page.catalogRevision)
        assertEquals(64U, page.offset)
        assertEquals(129U, page.total)
        assertEquals(true, page.more)
        val receipt =
            HubReferenceReceiptDto(
                "01900000-0000-7000-8000-000000000021",
                "02".repeat(32),
                1700000000L,
                3UL,
                previous,
                current,
            ).toModel()
        assertEquals("Old label", receipt.previous?.value?.label)
        assertEquals(page.records.single(), receipt.current)
        assertEquals("02".repeat(32), receipt.actor.value)
        assertEquals(1700000000L, receipt.recordedAt.value)
        assertNull(receipt.current.value.nominalGrams)
        val query = HubReferenceQuery(19UL, 64U, 64U, HubReferenceKind.Product, "🥕").toNative()
        assertEquals(19UL, query.expectedCatalogRevision)
        assertEquals(HubReferenceKindDto.PRODUCT, query.kind)
        assertEquals("🥕", query.text)
    }
}
