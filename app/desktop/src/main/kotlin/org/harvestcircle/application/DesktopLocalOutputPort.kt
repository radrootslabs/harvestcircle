package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import java.awt.EventQueue
import java.awt.FileDialog
import java.awt.Frame
import java.awt.Window
import java.awt.print.PrinterJob
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.print.attribute.HashPrintRequestAttributeSet
import javax.print.attribute.standard.DialogOwner
import javax.print.attribute.standard.DialogTypeSelection
import javax.swing.JEditorPane
import javax.swing.JWindow
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** The native core renders content; this adapter owns only desktop dialogs and local I/O. */
internal class DesktopLocalOutputPort(
    private val chooseDestination: (suspend (String) -> Path?)? = null,
) : LocalOutputPort {
    private val closed = AtomicBoolean(false)
    private val active = AtomicReference<OutputLease?>()

    override suspend fun perform(
        action: LocalOutputAction,
        document: HubLocalOutput,
    ): LocalOutputResult {
        if (closed.get()) return LocalOutputResult.Failed
        val bytes = document.html.toByteArray(Charsets.UTF_8)
        if (bytes.isEmpty() || bytes.size > MAX_OUTPUT_BYTES) return LocalOutputResult.Failed
        val lease = OutputLease()
        if (!active.compareAndSet(null, lease)) return LocalOutputResult.Failed
        if (closed.get()) lease.cancel()
        return try {
            // Cancellation owns the dialog/job. Keep its result observable until local I/O settles.
            withContext(NonCancellable + Dispatchers.IO) {
                when (action) {
                    LocalOutputAction.SaveHtml -> save(document.suggestedFilename, bytes, lease)
                    LocalOutputAction.Print -> print(document, lease)
                }
            }
        } catch (error: CancellationException) {
            throw error
        } catch (_: Exception) {
            if (lease.submitting.get()) LocalOutputResult.Uncertain else LocalOutputResult.Failed
        } finally {
            lease.disposeWindow()
            active.compareAndSet(lease, null)
        }
    }

    private suspend fun save(
        filename: String,
        bytes: ByteArray,
        lease: OutputLease,
    ): LocalOutputResult {
        if (lease.cancelled.get()) return LocalOutputResult.Cancelled
        val destination = if (chooseDestination != null) chooseDestination.invoke(filename) else chooseFile(filename, lease)
        if (destination == null || lease.cancelled.get()) return LocalOutputResult.Cancelled
        val path = destination.toAbsolutePath().normalize()
        val parent = path.parent ?: return LocalOutputResult.Failed
        val temporary = Files.createTempFile(parent, ".harvestcircle-output-", ".html")
        try {
            FileChannel.open(temporary, StandardOpenOption.WRITE).use { channel ->
                val buffer = ByteBuffer.wrap(bytes)
                while (buffer.hasRemaining()) channel.write(buffer)
                channel.force(true)
            }
            if (lease.cancelled.get()) return LocalOutputResult.Cancelled
            lease.submitting.set(true)
            // Do not degrade to a non-atomic replacement when the selected filesystem cannot support it.
            Files.move(temporary, path, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            return LocalOutputResult.Saved(path.toString())
        } finally {
            Files.deleteIfExists(temporary)
        }
    }

    private suspend fun chooseFile(
        filename: String,
        lease: OutputLease,
    ): Path? =
        onEdt {
            if (lease.cancelled.get()) return@onEdt null
            val dialog = FileDialog(null as Frame?, "Save captured HarvestCircle document", FileDialog.SAVE)
            lease.window.set(dialog)
            try {
                dialog.file = filename
                if (lease.cancelled.get()) return@onEdt null
                dialog.isVisible = true
                val selected = dialog.file ?: return@onEdt null
                val directory = dialog.directory ?: return@onEdt null
                Path.of(directory).resolve(selected)
            } finally {
                lease.window.compareAndSet(dialog, null)
                dialog.dispose()
            }
        }

    private suspend fun print(
        document: HubLocalOutput,
        lease: OutputLease,
    ): LocalOutputResult {
        if (lease.cancelled.get()) return LocalOutputResult.Cancelled
        if (PrinterJob.lookupPrintServices().isEmpty()) return LocalOutputResult.Failed
        val job = PrinterJob.getPrinterJob()
        lease.printer.set(job)
        val attributes = HashPrintRequestAttributeSet()
        val accepted =
            onEdt {
                if (lease.cancelled.get()) return@onEdt false
                val owner = JWindow()
                lease.window.set(owner)
                val editor = JEditorPane("text/html", document.html)
                editor.isEditable = false
                job.setPrintable(editor.getPrintable(null, null))
                job.jobName = document.suggestedFilename
                attributes.add(DialogOwner(owner))
                attributes.add(DialogTypeSelection.COMMON)
                // The common dialog is owned by this window; cancellation disposes no other application's UI.
                if (lease.cancelled.get()) false else job.printDialog(attributes)
            }
        if (!accepted || lease.cancelled.get()) return LocalOutputResult.Cancelled
        lease.submitting.set(true)
        job.print(attributes)
        return if (lease.cancelled.get() || job.isCancelled) LocalOutputResult.Uncertain else LocalOutputResult.PrintSubmitted
    }

    override fun cancel() {
        active.get()?.cancel()
    }

    override fun close() {
        closed.set(true)
        cancel()
    }

    private class OutputLease {
        val cancelled = AtomicBoolean(false)
        val submitting = AtomicBoolean(false)
        val window = AtomicReference<Window?>()
        val printer = AtomicReference<PrinterJob?>()

        fun cancel() {
            cancelled.set(true)
            printer.get()?.cancel()
            disposeWindow()
        }

        fun disposeWindow() {
            val owned = window.getAndSet(null) ?: return
            EventQueue.invokeLater { owned.dispose() }
        }
    }

    private suspend fun <T> onEdt(block: () -> T): T =
        suspendCancellableCoroutine { continuation ->
            EventQueue.invokeLater {
                if (continuation.isActive) {
                    try {
                        continuation.resume(block())
                    } catch (error: Exception) {
                        continuation.resumeWithException(error)
                    }
                }
            }
        }

    private companion object {
        const val MAX_OUTPUT_BYTES = 2 * 1024 * 1024
    }
}
