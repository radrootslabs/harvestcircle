package org.radroots.harvestcircle.application

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.awt.Toolkit
import java.awt.datatransfer.DataFlavor
import java.awt.datatransfer.StringSelection

internal interface TextClipboard {
    fun readText(): String?

    fun writeText(value: String)
}

internal sealed interface SecretClipboardResult {
    data object Copied : SecretClipboardResult

    data object Unavailable : SecretClipboardResult
}

internal class SecretClipboardController(
    private val scope: CoroutineScope,
    private val clipboard: TextClipboard = SystemTextClipboard,
    private val clearDelayMillis: Long = 60_000,
) : AutoCloseable {
    private var clearJob: Job? = null
    private var copiedValue: String? = null

    fun copy(value: String): SecretClipboardResult {
        if (runCatching { clipboard.writeText(value) }.isFailure) {
            return SecretClipboardResult.Unavailable
        }
        copiedValue = value
        clearJob?.cancel()
        clearJob =
            scope.launch {
                delay(clearDelayMillis)
                runCatching {
                    if (clipboard.readText() == value) clipboard.writeText("")
                }
                if (copiedValue == value) copiedValue = null
            }
        return SecretClipboardResult.Copied
    }

    override fun close() {
        clearJob?.cancel()
        clearJob = null
        val value = copiedValue
        runCatching {
            if (value != null && clipboard.readText() == value) clipboard.writeText("")
        }
        copiedValue = null
    }
}

private object SystemTextClipboard : TextClipboard {
    private val clipboard
        get() = Toolkit.getDefaultToolkit().systemClipboard

    override fun readText(): String? =
        runCatching {
            clipboard.getData(DataFlavor.stringFlavor) as? String
        }.getOrNull()

    override fun writeText(value: String) {
        clipboard.setContents(StringSelection(value), null)
    }
}
