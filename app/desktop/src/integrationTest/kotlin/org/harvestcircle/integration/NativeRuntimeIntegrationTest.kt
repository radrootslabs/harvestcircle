package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SecretKeyInput
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.testbridge.ffi.TestBridgeException
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readBytes
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.test.fail

class NativeRuntimeIntegrationTest {
    @Test
    fun nativeBridgeCoversIdentityRelayRestartObserverTimeoutAndRedaction() =
        runBlocking {
            val dataRoot = Files.createTempDirectory("harvestcircle-native-integration-")
            try {
                TestBridgeHarvestCircleRuntime.open(dataRoot.toString()).use { runtime ->
                    assertEquals(
                        "c7a84960e53cd9df35d676bab28294eb048a8b86c766d81cded2635b64a7f3d6",
                        compatibilityDescriptor().contractHash,
                    )
                    val initial = runtime.bootstrap()
                    assertTrue(initial.revision.value > 0UL)
                    assertTrue(initial.identities.isEmpty())

                    val bridge = runtime.nativeBridge()
                    bridge.startObserver()
                    assertEquals(initial.revision.value, assertNotNull(bridge.nextObservedSnapshot(2_000UL)).revision)

                    val generated = runtime.prepareLocalIdentity()
                    val recoverySecret = generated.backup.revealNsec()
                    assertTrue(recoverySecret.startsWith("nsec1"))
                    val created =
                        runtime
                            .execute(
                                ApplicationCommand.AcknowledgeGeneratedIdentity(
                                    generated.requestId,
                                    request("00000000-0000-7000-8000-000000000001", initial.revision),
                                ),
                            ).snapshot
                    assertEquals(1, created.identities.size)
                    assertEquals(created.revision.value, assertNotNull(bridge.nextObservedSnapshot(2_000UL)).revision)
                    assertTrue(bridge.stopObserver())

                    runtime.seedSelectedProfile("Farm Identity")
                    val generatedId = assertNotNull(created.selectedIdentityId)
                    runtime.execute(ApplicationCommand.ActivateIdentity(generatedId))
                    val refreshed = runtime.execute(ApplicationCommand.RefreshActiveProfile).snapshot
                    assertEquals(SessionLifecycle.Active, refreshed.session)
                    assertEquals("Farm Identity", refreshed.activeIdentity?.profile?.displayName)

                    runtime.execute(ApplicationCommand.SignOut)
                    val restarted = runtime.restart()
                    assertEquals(generatedId, restarted.selectedIdentityId)
                    assertEquals(SessionLifecycle.SignedOut, restarted.session)
                    assertEquals(
                        SessionLifecycle.Active,
                        runtime.execute(ApplicationCommand.ActivateIdentity(generatedId)).snapshot.session,
                    )

                    val signedOut = runtime.execute(ApplicationCommand.SignOut).snapshot
                    val importRecovery = runtime.prepareLocalIdentity()
                    val importedSecret = importRecovery.backup.revealNsec()
                    runtime.execute(ApplicationCommand.CancelGeneratedIdentity(importRecovery.requestId))
                    val imported =
                        runtime
                            .execute(
                                ApplicationCommand.ImportLocalIdentity(
                                    SecretKeyInput.from(importedSecret),
                                    request("00000000-0000-7000-8000-000000000002", signedOut.revision),
                                ),
                            ).snapshot
                    assertEquals(2, imported.identities.size)

                    val timeoutRecovery = runtime.prepareLocalIdentity()
                    val timeoutSecret = timeoutRecovery.backup.revealNsec()
                    runtime.execute(ApplicationCommand.CancelGeneratedIdentity(timeoutRecovery.requestId))
                    val timeoutBytes = timeoutSecret.encodeToByteArray()
                    val timeout =
                        try {
                            bridge.importIdentity(
                                "00000000-0000-7000-8000-000000000003",
                                imported.revision.value,
                                timeoutBytes,
                                0UL,
                            )
                            fail("zero-deadline command unexpectedly succeeded")
                        } catch (error: TestBridgeException.Failure) {
                            error.safeMessage
                        } finally {
                            timeoutBytes.fill(0)
                        }
                    assertFalse(timeout.contains(timeoutSecret))
                    assertEquals(2, bridge.snapshot().identities.size)

                    val databaseBytes = dataRoot.resolve("harvestcircle-integration.sqlite3").readBytes()
                    assertFalse(databaseBytes.containsBytes(importedSecret.encodeToByteArray()))
                    assertFalse(databaseBytes.containsBytes(timeoutSecret.encodeToByteArray()))
                    assertFalse(databaseBytes.containsBytes("nsec1".encodeToByteArray()))
                    val publicEvidence = bridge.snapshot().toString() + timeout
                    assertFalse(publicEvidence.contains(importedSecret))
                    assertFalse(publicEvidence.contains(timeoutSecret))
                    assertFalse(publicEvidence.contains(recoverySecret))

                    val shutdown = runtime.shutdown()
                    assertTrue(shutdown.closed)
                }
            } finally {
                deleteTree(dataRoot)
            }
        }
}

private fun request(
    operationId: String,
    revision: SnapshotRevision,
): RequestContext =
    RequestContext(
        operationId = OperationId.from(operationId),
        expectedRevision = revision,
        deadlineMillis = 2_000UL,
    )

private fun ByteArray.containsBytes(needle: ByteArray): Boolean =
    needle.isNotEmpty() &&
        indices.any { start ->
            start + needle.size <= size && needle.indices.all { offset -> this[start + offset] == needle[offset] }
        }

private fun deleteTree(root: Path) {
    Files.walk(root).use { paths ->
        paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
    }
}
