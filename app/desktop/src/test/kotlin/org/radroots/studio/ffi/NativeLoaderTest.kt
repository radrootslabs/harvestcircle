package org.radroots.studio.ffi

import kotlin.test.Test
import kotlin.test.assertEquals

class NativeLoaderTest {
    @Test
    fun generatedBindingLoadsTheCurrentHostLibrary() {
        assertEquals("0.1.0-alpha", nativeRuntimeVersion())
    }
}
