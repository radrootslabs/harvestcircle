package org.harvestcircle.application

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class DesktopLocalOutputPortTest {
    @Test
    fun atomicUtf8SavingReplacesOnlyTheSelectedFileAndLeavesNoOwnedTemporary() =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-local-output-")
            try {
                val destination = root.resolve("chosen.html")
                Files.writeString(destination, "previous content")
                Files.writeString(root.resolve("unrelated.txt"), "retain")
                val port = DesktopLocalOutputPort { destination }
                try {
                    assertEquals(LocalOutputResult.Saved(destination.toString()), port.perform(LocalOutputAction.SaveHtml, DOCUMENT))
                    assertEquals(DOCUMENT.html, Files.readString(destination))
                    assertEquals("retain", Files.readString(root.resolve("unrelated.txt")))
                    Files.list(root).use { files -> assertEquals(2L, files.count()) }
                } finally {
                    port.close()
                }
            } finally {
                root.toFile().deleteRecursively()
            }
        }

    @Test
    fun cancellationAndBoundsDoNotSelectOrWriteAnOutput() =
        runBlocking {
            var selections = 0
            val port =
                DesktopLocalOutputPort {
                    selections += 1
                    null
                }
            try {
                assertEquals(LocalOutputResult.Cancelled, port.perform(LocalOutputAction.SaveHtml, DOCUMENT))
                assertEquals(1, selections)
                assertEquals(
                    LocalOutputResult.Failed,
                    port.perform(
                        LocalOutputAction.SaveHtml,
                        DOCUMENT.copy(
                            html =
                                "x".repeat(
                                    2 * 1024 * 1024 + 1,
                                ),
                        ),
                    ),
                )
                assertEquals(1, selections)
                port.close()
                assertEquals(LocalOutputResult.Failed, port.perform(LocalOutputAction.SaveHtml, DOCUMENT))
                assertEquals(1, selections)
            } finally {
                port.close()
            }
        }

    @Test
    fun cancellingOneOwnedSelectionDoesNotCancelAnotherAdapterOrWriteItsSelectedFile(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-output-cancel-")
            val entered = CompletableDeferred<Unit>()
            val release = CompletableDeferred<Unit>()
            val firstPath = root.resolve("first.html")
            val secondPath = root.resolve("second.html")
            val first =
                DesktopLocalOutputPort {
                    entered.complete(Unit)
                    release.await()
                    firstPath
                }
            val second = DesktopLocalOutputPort { secondPath }
            try {
                val pending = async { first.perform(LocalOutputAction.SaveHtml, DOCUMENT) }
                entered.await()
                first.cancel()
                assertIs<LocalOutputResult.Saved>(second.perform(LocalOutputAction.SaveHtml, DOCUMENT))
                release.complete(Unit)
                assertEquals(LocalOutputResult.Cancelled, pending.await())
                assertFalse(Files.exists(firstPath))
                assertEquals(DOCUMENT.html, Files.readString(secondPath))
            } finally {
                release.complete(Unit)
                first.close()
                second.close()
                root.toFile().deleteRecursively()
            }
        }

    @Test
    fun failedAtomicReplacementPreservesExistingDestinationAndCleansOnlyOwnedTemporary(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-output-failure-")
            try {
                val directory = Files.createDirectory(root.resolve("existing-directory"))
                Files.writeString(directory.resolve("retain.txt"), "retain")
                val port = DesktopLocalOutputPort { directory }
                try {
                    assertEquals(LocalOutputResult.Uncertain, port.perform(LocalOutputAction.SaveHtml, DOCUMENT))
                    assertTrue(Files.isDirectory(directory))
                    assertEquals("retain", Files.readString(directory.resolve("retain.txt")))
                    Files.list(root).use { files -> assertEquals(1L, files.count()) }
                } finally {
                    port.close()
                }
            } finally {
                root.toFile().deleteRecursively()
            }
        }

    private companion object {
        val DOCUMENT = HubLocalOutput("frozen-content", "captured.html", "<html><body>Fictional Ω &amp; &lt;source&gt;</body></html>")
    }
}
