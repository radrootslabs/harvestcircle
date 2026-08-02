package org.radroots.studio.application

import java.awt.Toolkit
import java.awt.datatransfer.DataFlavor
import java.awt.datatransfer.StringSelection
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

internal interface TextClipboard {
    fun readText(): String?

    fun writeText(value: String)
}

internal class SecretClipboardController(
    private val scope: CoroutineScope,
    private val clipboard: TextClipboard = SystemTextClipboard,
    private val clearDelayMillis: Long = 60_000,
) : AutoCloseable {
    private var clearJob: Job? = null

    fun copy(value: String) {
        clipboard.writeText(value)
        clearJob?.cancel()
        clearJob = scope.launch {
            delay(clearDelayMillis)
            if (clipboard.readText() == value) clipboard.writeText("")
        }
    }

    override fun close() {
        clearJob?.cancel()
        clearJob = null
    }
}

private object SystemTextClipboard : TextClipboard {
    private val clipboard
        get() = Toolkit.getDefaultToolkit().systemClipboard

    override fun readText(): String? = runCatching {
        clipboard.getData(DataFlavor.stringFlavor) as? String
    }.getOrNull()

    override fun writeText(value: String) {
        clipboard.setContents(StringSelection(value), null)
    }
}
