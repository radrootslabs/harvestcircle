package org.harvestcircle.application

import org.harvestcircle.ffi.CompatibilityDescriptor
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class NativeCompatibilityTest {
    @Test
    fun acceptsOnlyTheDeclaredNativeContractAndSchemaWindow() {
        val descriptor = compatibleDescriptor()
        val expectation = verifyNativeCompatibility(descriptor)
        assertEquals(EXPECTED_FFI_CONTRACT_MAJOR, expectation.contractMajor)
        assertEquals(EXPECTED_FFI_CONTRACT_HASH, expectation.contractHash)

        listOf(
            descriptor.copy(contractId = "wrong"),
            descriptor.copy(productVersion = "wrong"),
            descriptor.copy(cargoPackageVersion = "wrong"),
            descriptor.copy(distributionPackageVersion = "wrong"),
            descriptor.copy(contractMajor = 5.toUShort()),
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
            contractId = EXPECTED_FFI_CONTRACT_ID,
            productVersion = EXPECTED_PRODUCT_VERSION,
            cargoPackageVersion = EXPECTED_CARGO_PACKAGE_VERSION,
            distributionPackageVersion = EXPECTED_DISTRIBUTION_PACKAGE_VERSION,
            contractMajor = EXPECTED_FFI_CONTRACT_MAJOR,
            contractMinor = MINIMUM_FFI_CONTRACT_MINOR,
            contractHash = EXPECTED_FFI_CONTRACT_HASH,
            productCoordinateDigest = EXPECTED_PRODUCT_COORDINATE_DIGEST,
            snapshotSchemaVersion = EXPECTED_SNAPSHOT_SCHEMA,
            minimumSchemaVersion = MINIMUM_STORAGE_SCHEMA,
            currentSchemaVersion = MAXIMUM_STORAGE_SCHEMA,
            sourceProvenanceDigest = EXPECTED_SOURCE_PROVENANCE_DIGEST,
            sourceFoundationBaseline = EXPECTED_SOURCE_FOUNDATION_BASELINE,
        )
}
