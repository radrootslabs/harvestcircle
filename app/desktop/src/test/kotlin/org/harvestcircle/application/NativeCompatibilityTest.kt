package org.harvestcircle.application

import org.harvestcircle.ffi.CompatibilityDescriptor
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import org.harvestcircle.application.generated.NativeCompatibilityExpectations as Expected

class NativeCompatibilityTest {
    @Test
    fun acceptsOnlyTheDeclaredNativeContractAndSchemaWindow() {
        val descriptor = compatibleDescriptor()
        val expectation = verifyNativeCompatibility(descriptor)
        assertEquals(Expected.ffiContractMajor, expectation.contractMajor)
        assertEquals(Expected.ffiContractHash, expectation.contractHash)
        assertEquals(
            expectation,
            verifyNativeCompatibility(
                descriptor.copy(contractMinor = (Expected.minimumFfiContractMinor.toUInt() + 1U).toUShort()),
            ),
        )

        listOf(
            descriptor.copy(contractId = "wrong"),
            descriptor.copy(productVersion = "wrong"),
            descriptor.copy(cargoPackageVersion = "wrong"),
            descriptor.copy(distributionPackageVersion = "wrong"),
            descriptor.copy(contractMajor = 5.toUShort()),
            descriptor.copy(contractMinor = (Expected.minimumFfiContractMinor.toUInt() - 1U).toUShort()),
            descriptor.copy(contractHash = "wrong"),
            descriptor.copy(productCoordinateDigest = "wrong"),
            descriptor.copy(snapshotSchemaVersion = 2U),
            descriptor.copy(currentSchemaVersion = 4U),
            descriptor.copy(minimumSchemaVersion = 11U),
            descriptor.copy(sourceProvenanceDigest = "wrong"),
            descriptor.copy(sourceFoundationBaseline = "wrong"),
        ).forEach { incompatible ->
            assertFailsWith<NativeCompatibilityException> {
                verifyNativeCompatibility(incompatible)
            }
        }
    }

    private fun compatibleDescriptor() =
        CompatibilityDescriptor(
            contractId = Expected.ffiContractId,
            productVersion = Expected.productVersion,
            cargoPackageVersion = Expected.cargoPackageVersion,
            distributionPackageVersion = Expected.distributionPackageVersion,
            contractMajor = Expected.ffiContractMajor,
            contractMinor = Expected.minimumFfiContractMinor,
            contractHash = Expected.ffiContractHash,
            productCoordinateDigest = Expected.productCoordinateDigest,
            snapshotSchemaVersion = Expected.snapshotSchema,
            minimumSchemaVersion = Expected.minimumStorageSchema,
            currentSchemaVersion = Expected.maximumStorageSchema,
            sourceProvenanceDigest = Expected.sourceProvenanceDigest,
            sourceFoundationBaseline = Expected.sourceFoundationBaseline,
        )
}
