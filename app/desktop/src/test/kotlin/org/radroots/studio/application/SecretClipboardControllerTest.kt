package org.radroots.studio.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalCoroutinesApi::class)
class SecretClipboardControllerTest {
    @Test
    fun clearsCopiedSecretAfterDelayWhenClipboardIsUnchanged() = runTest {
        val clipboard = FakeTextClipboard()
        val controller = SecretClipboardController(this, clipboard, clearDelayMillis = 60_000)

        controller.copy("nsec1generated")
        advanceTimeBy(60_000)
        runCurrent()

        assertEquals("", clipboard.value)
        controller.close()
    }

    @Test
    fun preservesClipboardContentReplacedByUserBeforeDelay() = runTest {
        val clipboard = FakeTextClipboard()
        val controller = SecretClipboardController(this, clipboard, clearDelayMillis = 60_000)

        controller.copy("nsec1generated")
        clipboard.writeText("replacement")
        advanceTimeBy(60_000)
        runCurrent()

        assertEquals("replacement", clipboard.value)
        controller.close()
    }

    @Test
    fun replacingCopiedSecretCancelsEarlierClearTimer() = runTest {
        val clipboard = FakeTextClipboard()
        val controller = SecretClipboardController(this, clipboard, clearDelayMillis = 60_000)

        controller.copy("nsec1first")
        advanceTimeBy(30_000)
        controller.copy("nsec1second")
        advanceTimeBy(30_000)
        runCurrent()
        assertEquals("nsec1second", clipboard.value)
        advanceTimeBy(30_000)
        runCurrent()
        assertEquals("", clipboard.value)
        controller.close()
    }
}

private class FakeTextClipboard : TextClipboard {
    var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value
    }
}
