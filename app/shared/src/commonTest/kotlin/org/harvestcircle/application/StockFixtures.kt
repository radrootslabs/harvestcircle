package org.harvestcircle.application

internal object StockFixtures {
    val actor = IdentityId.fromPublicKeyHex("01".repeat(32))
    val operation = OperationId.from("01900000-0000-7000-8000-000000000031")
    val product = HubReferenceRecord("01".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Product, "Fictional apples"))
    val pack = HubReferenceRecord("02".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Pack, "Original crate", productId = product.id))
    val receiving =
        HubReferenceRecord(
            "03".repeat(16),
            1UL,
            HubReferenceValue(HubReferenceKind.Location, "Receiving", locationPurpose = HubLocationPurpose.Receiving),
        )
    val planned =
        HubReferenceRecord(
            "04".repeat(16),
            1UL,
            HubReferenceValue(HubReferenceKind.Location, "A3", locationPurpose = HubLocationPurpose.Storage),
        )
    val references = listOf(product, pack, receiving, planned)
    val basis = HubHandlingBasis("whole-pack-demo-v1", 1U, "Whole-pack demo", "UTC")
    val empty = HubStockPage(basis, 1UL, 5UL, 0U, 0U, false, emptyList())
    val trace =
        HubStockPopulation(
            "05".repeat(16),
            pack,
            product,
            null,
            null,
            HubSource(null, null, null, null, null, HubSourceCertainty.Unknown),
            true,
            20U,
            receiving,
            planned,
            HubOccurrence(HubOccurrenceKind.Unknown),
            UnixSeconds(100L),
            actor,
            "Counted twenty original crates",
        )
    val position = HubStockPosition("06".repeat(16), trace.id, pack.id, receiving.id, 20U, 1UL)
    val work = HubQuantityWork(20U, 0U, 0U, 20U, 1UL, planned)
    val row = HubStockRow(product, position, receiving, trace, work)
    val received = empty.copy(stockRevision = 2UL, total = 1U, rows = listOf(row))

    fun receipt(op: OperationId = operation) = HubStockReceipt(op, 2UL, trace, position, work)

    fun readyDraft(draft: ReceiptDraft) =
        draft.copy(
            variantId = pack.id,
            receivingId = receiving.id,
            plannedLocationId = planned.id,
            quantity = "20",
            observation = "Counted twenty original crates",
        )
}
