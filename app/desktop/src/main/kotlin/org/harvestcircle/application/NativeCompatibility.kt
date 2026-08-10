package org.harvestcircle.application

import org.harvestcircle.ffi.CompatibilityDescriptor
import org.harvestcircle.ffi.CompatibilityExpectation

internal const val EXPECTED_FFI_CONTRACT_ID = "harvestcircle-desktop-ffi-v4"
internal const val EXPECTED_PRODUCT_VERSION = "0.1.0-alpha"
internal const val EXPECTED_CARGO_PACKAGE_VERSION = "0.1.0-alpha"
internal const val EXPECTED_DISTRIBUTION_PACKAGE_VERSION = "1.0.0"
internal const val EXPECTED_PRODUCT_COORDINATE_DIGEST = "93bf10e334e989b20ba5fb8ed05e5d55b83f4502efba5f893aef4dc1a66c8223"
internal const val EXPECTED_SOURCE_PROVENANCE_DIGEST = "db238195b4a5938a8d4d9ac5681c4b125e65c57aa8133ad03e59da4e4bd062bc"
internal const val EXPECTED_SOURCE_FOUNDATION_BASELINE = "a2038b3e25b9e34f0b8fd001f26a8ed10b5772cb"
internal const val EXPECTED_FFI_CONTRACT_HASH = "565f25d8a3ddf418b06a320c92284455ec4d8b82886cde8609a93bbb2486c3a4"
internal val EXPECTED_FFI_CONTRACT_MAJOR: UShort = 4.toUShort()
internal val MINIMUM_FFI_CONTRACT_MINOR: UShort = 0.toUShort()
internal const val EXPECTED_SNAPSHOT_SCHEMA: UInt = 1U
internal const val MINIMUM_STORAGE_SCHEMA: UInt = 5U
internal const val MAXIMUM_STORAGE_SCHEMA: UInt = 10U

internal class NativeCompatibilityException :
    IllegalStateException(
        "The application and native runtime are incompatible.",
    )

internal fun verifyNativeCompatibility(descriptor: CompatibilityDescriptor): CompatibilityExpectation {
    val compatible =
        descriptor.contractId == EXPECTED_FFI_CONTRACT_ID &&
            descriptor.productVersion == EXPECTED_PRODUCT_VERSION &&
            descriptor.cargoPackageVersion == EXPECTED_CARGO_PACKAGE_VERSION &&
            descriptor.distributionPackageVersion == EXPECTED_DISTRIBUTION_PACKAGE_VERSION &&
            descriptor.contractMajor == EXPECTED_FFI_CONTRACT_MAJOR &&
            descriptor.contractMinor >= MINIMUM_FFI_CONTRACT_MINOR &&
            descriptor.contractHash == EXPECTED_FFI_CONTRACT_HASH &&
            descriptor.productCoordinateDigest == EXPECTED_PRODUCT_COORDINATE_DIGEST &&
            descriptor.snapshotSchemaVersion == EXPECTED_SNAPSHOT_SCHEMA &&
            descriptor.currentSchemaVersion >= MINIMUM_STORAGE_SCHEMA &&
            descriptor.minimumSchemaVersion <= MAXIMUM_STORAGE_SCHEMA &&
            descriptor.sourceProvenanceDigest == EXPECTED_SOURCE_PROVENANCE_DIGEST &&
            descriptor.sourceFoundationBaseline == EXPECTED_SOURCE_FOUNDATION_BASELINE
    if (!compatible) throw NativeCompatibilityException()
    return CompatibilityExpectation(
        contractId = EXPECTED_FFI_CONTRACT_ID,
        contractMajor = EXPECTED_FFI_CONTRACT_MAJOR,
        minimumContractMinor = MINIMUM_FFI_CONTRACT_MINOR,
        contractHash = EXPECTED_FFI_CONTRACT_HASH,
        productCoordinateDigest = EXPECTED_PRODUCT_COORDINATE_DIGEST,
        snapshotSchemaVersion = EXPECTED_SNAPSHOT_SCHEMA,
        minimumSchemaVersion = MINIMUM_STORAGE_SCHEMA,
        maximumSchemaVersion = MAXIMUM_STORAGE_SCHEMA,
    )
}
