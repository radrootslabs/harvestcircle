package org.harvestcircle.buildlogic.plugins.tasks

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse

class PackagingTasksTest {
    @Test
    fun metadataEvidenceMustBeComplete() {
        requireBuildMetadataEvidence("version=1.0 toolchain=21", listOf("1.0", "21"))

        assertFailsWith<IllegalArgumentException> {
            requireBuildMetadataEvidence("version=1.0", listOf("1.0", "missing-toolchain"))
        }
    }

    @Test
    fun nativeInventoryRequiresOneCanonicalProductionPayload() {
        val expected = "darwin-aarch64/libharvestcircle_ffi.dylib"
        requireSingleCanonicalProductNativeEntry(listOf("com/sun/jna/darwin-aarch64/libjnidispatch.jnilib", expected), expected)

        assertFailsWith<IllegalArgumentException> {
            requireSingleCanonicalProductNativeEntry(listOf(expected, expected), expected)
        }
        assertFailsWith<IllegalArgumentException> {
            requireSingleCanonicalProductNativeEntry(listOf("darwin-aarch64/libharvestcircle_test_ffi.dylib"), expected)
        }
    }

    @Test
    fun hostCommandResultsPreserveOutputAndFailureDiagnostics() {
        assertEquals("value", requireSuccessfulCommand(0, " value\n"))
        val failure = assertFailsWith<IllegalArgumentException> { requireSuccessfulCommand(17, "tool failed\n") }
        assertEquals("External package inspection failed with exit 17: tool failed", failure.message)
    }

    @Test
    fun unsignedDistributionVerifierContainsNoSigningToolInvocation() {
        val bytecode =
            checkNotNull(
                javaClass.classLoader.getResourceAsStream(
                    "org/harvestcircle/buildlogic/plugins/tasks/VerifyMacOsDistribution.class",
                ),
            ) { "VerifyMacOsDistribution bytecode is unavailable" }
                .use { it.readBytes().toString(Charsets.ISO_8859_1) }

        listOf("/usr/bin/codesign", "/usr/bin/xcrun", "stapler", "/usr/sbin/spctl").forEach { forbidden ->
            assertFalse(bytecode.contains(forbidden), "Unsigned distribution verifier invokes $forbidden")
        }
    }
}
