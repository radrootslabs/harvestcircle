package org.harvestcircle.desktop

import java.io.ByteArrayInputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MainTest {
    @Test
    fun desktopWindowUsesTheFoundationDimensions() {
        assertEquals(1280, INITIAL_WINDOW_WIDTH)
        assertEquals(800, INITIAL_WINDOW_HEIGHT)
        assertEquals(1100, MINIMUM_WINDOW_WIDTH)
        assertEquals(720, MINIMUM_WINDOW_HEIGHT)
    }

    @Test
    fun missingOrInvalidRuntimeIconFailsSafely() {
        assertNull(loadRuntimeIcon { null })
        assertNull(loadRuntimeIcon { ByteArrayInputStream("not an image".encodeToByteArray()) })
    }

    @Test
    fun healthCheckEntryRequiresTheSingleSupportedArgument() {
        assertTrue(isHealthCheck(arrayOf(HEALTH_CHECK_ARGUMENT)))
        assertFalse(isHealthCheck(emptyArray()))
        assertFalse(isHealthCheck(arrayOf(HEALTH_CHECK_ARGUMENT, "unexpected")))
    }
}
