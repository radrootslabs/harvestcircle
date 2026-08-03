package org.radroots.studio.application

import org.radroots.studio.ffi.CompatibilityDescriptor
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
            descriptor.copy(contractMajor = 3.toUShort()),
            descriptor.copy(contractHash = "wrong"),
            descriptor.copy(currentSchemaVersion = 4U),
            descriptor.copy(minimumSchemaVersion = 10U),
        ).forEach { incompatible ->
            assertFailsWith<NativeCompatibilityException> {
                verifyNativeCompatibility(incompatible)
            }
        }
    }

    private fun compatibleDescriptor() = CompatibilityDescriptor(
        contractMajor = EXPECTED_FFI_CONTRACT_MAJOR,
        contractMinor = MINIMUM_FFI_CONTRACT_MINOR,
        contractHash = EXPECTED_FFI_CONTRACT_HASH,
        minimumSchemaVersion = MINIMUM_STORAGE_SCHEMA,
        currentSchemaVersion = MAXIMUM_STORAGE_SCHEMA,
    )
}
