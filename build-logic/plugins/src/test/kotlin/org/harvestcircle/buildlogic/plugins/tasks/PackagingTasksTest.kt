package org.harvestcircle.buildlogic.plugins.tasks

import kotlin.test.Test
import kotlin.test.assertFailsWith

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
}
