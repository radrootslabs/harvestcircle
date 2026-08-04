package org.radroots.studio.desktop

import java.io.ByteArrayInputStream
import kotlin.test.Test
import kotlin.test.assertNull

class MainTest {
    @Test
    fun missingOrInvalidRuntimeIconFailsSafely() {
        assertNull(loadRuntimeIcon { null })
        assertNull(loadRuntimeIcon { ByteArrayInputStream("not an image".encodeToByteArray()) })
    }
}
