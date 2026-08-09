package org.radroots.harvestcircle.application

import org.radroots.harvestcircle.ffi.CompatibilityDescriptor
import org.radroots.harvestcircle.ffi.CompatibilityExpectation

internal const val EXPECTED_PRODUCT_VERSION = "0.1.0-alpha"
internal const val EXPECTED_CARGO_PACKAGE_VERSION = "0.1.0-alpha"
internal const val EXPECTED_FFI_CONTRACT_HASH = "d4e298f0abeaa65aa68e70d7a6e8f69f8182f12f93c12b2dd056d3ed5d83e9c0"
internal val EXPECTED_FFI_CONTRACT_MAJOR: UShort = 3.toUShort()
internal val MINIMUM_FFI_CONTRACT_MINOR: UShort = 0.toUShort()
internal const val MINIMUM_STORAGE_SCHEMA: UInt = 5U
internal const val MAXIMUM_STORAGE_SCHEMA: UInt = 10U

internal class NativeCompatibilityException :
    IllegalStateException(
        "The application and native runtime are incompatible.",
    )

internal fun verifyNativeCompatibility(descriptor: CompatibilityDescriptor): CompatibilityExpectation {
    val compatible =
        descriptor.productVersion == EXPECTED_PRODUCT_VERSION &&
            descriptor.cargoPackageVersion == EXPECTED_CARGO_PACKAGE_VERSION &&
            descriptor.contractMajor == EXPECTED_FFI_CONTRACT_MAJOR &&
            descriptor.contractMinor >= MINIMUM_FFI_CONTRACT_MINOR &&
            descriptor.contractHash == EXPECTED_FFI_CONTRACT_HASH &&
            descriptor.currentSchemaVersion >= MINIMUM_STORAGE_SCHEMA &&
            descriptor.minimumSchemaVersion <= MAXIMUM_STORAGE_SCHEMA
    if (!compatible) throw NativeCompatibilityException()
    return CompatibilityExpectation(
        contractMajor = EXPECTED_FFI_CONTRACT_MAJOR,
        minimumContractMinor = MINIMUM_FFI_CONTRACT_MINOR,
        contractHash = EXPECTED_FFI_CONTRACT_HASH,
        minimumSchemaVersion = MINIMUM_STORAGE_SCHEMA,
        maximumSchemaVersion = MAXIMUM_STORAGE_SCHEMA,
    )
}
