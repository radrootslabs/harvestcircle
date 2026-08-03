package org.radroots.studio.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

@OptIn(ExperimentalCoroutinesApi::class)
class SecretClipboardControllerTest {
    @Test
    fun clearsCopiedSecretAfterDelayWhenClipboardIsUnchanged() = runTest {
        val clipboard = FakeTextClipboard()
        val controller = SecretClipboardController(this, clipboard, clearDelayMillis = 60_000)

        assertIs<SecretClipboardResult.Copied>(controller.copy("nsec1generated"))
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

    @Test
    fun disposalClearsOnlyClipboardTextOwnedByController() = runTest {
        val clipboard = FakeTextClipboard()
        val controller = SecretClipboardController(this, clipboard)
        controller.copy("nsec1generated")

        controller.close()

        assertEquals("", clipboard.value)

        val replacedClipboard = FakeTextClipboard()
        val replacedController = SecretClipboardController(this, replacedClipboard)
        replacedController.copy("nsec1generated")
        replacedClipboard.writeText("replacement")
        replacedController.close()
        assertEquals("replacement", replacedClipboard.value)
    }

    @Test
    fun clipboardFailuresReturnTypedUnavailableAndNeverCrashCleanup() = runTest {
        val unavailable = ThrowingTextClipboard(failWrites = true)
        val controller = SecretClipboardController(this, unavailable, clearDelayMillis = 1)
        assertIs<SecretClipboardResult.Unavailable>(controller.copy("nsec1generated"))
        controller.close()

        val failsDuringCleanup = ThrowingTextClipboard(failReads = true)
        val cleanupController = SecretClipboardController(this, failsDuringCleanup, clearDelayMillis = 1)
        assertIs<SecretClipboardResult.Copied>(cleanupController.copy("nsec1generated"))
        advanceTimeBy(1)
        runCurrent()
        cleanupController.close()
    }
}

private class FakeTextClipboard : TextClipboard {
    var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value
    }
}

private class ThrowingTextClipboard(
    private val failReads: Boolean = false,
    private val failWrites: Boolean = false,
) : TextClipboard {
    override fun readText(): String? {
        if (failReads) error("injected clipboard read failure")
        return null
    }

    override fun writeText(value: String) {
        if (failWrites) error("injected clipboard write failure")
    }
}
