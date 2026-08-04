package org.radroots.studio.application

import org.radroots.studio.ffi.CompatibilityDescriptor
import org.radroots.studio.ffi.CompatibilityExpectation

internal const val EXPECTED_FFI_CONTRACT_HASH = "radroots-studio-native-v2-2026-08-03"
internal val EXPECTED_FFI_CONTRACT_MAJOR: UShort = 2.toUShort()
internal val MINIMUM_FFI_CONTRACT_MINOR: UShort = 0.toUShort()
internal const val MINIMUM_STORAGE_SCHEMA: UInt = 5U
internal const val MAXIMUM_STORAGE_SCHEMA: UInt = 9U

internal class NativeCompatibilityException :
    IllegalStateException(
        "The application and native runtime are incompatible.",
    )

internal fun verifyNativeCompatibility(descriptor: CompatibilityDescriptor): CompatibilityExpectation {
    val compatible =
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
