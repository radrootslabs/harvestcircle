package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SecretKeyInput
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.verifyNativeCompatibility
import org.harvestcircle.ffi.NostrReferenceKindDto
import org.harvestcircle.ffi.classifyNostrReference
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.testbridge.ffi.HarvestCircleTestBridge
import org.harvestcircle.testbridge.ffi.TestBridgeException
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readBytes
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.test.fail

class NativeRuntimeIntegrationTest {
    @Test
    fun generatedFfiClassifiesCanonicalReferencesAndRedactsPrivateKeys() {
        val eventId = "d94a3f4dd87b9a3b0bed183b32e916fa29c8020107845d1752d72697fe5309a5"
        val event = classifyNostrReference(eventId)
        assertEquals(NostrReferenceKindDto.EVENT_ID, event.classification)
        assertEquals(eventId, event.canonicalReference)

        val nsec = "nsec1j4c6269y9w0q2er2xjw8sv2ehyrtfxq3jwgdlxj6qfn8z4gjsq5qfvfk99"
        val privateReference = classifyNostrReference(nsec)
        assertEquals(NostrReferenceKindDto.PRIVATE_KEY_REJECTED, privateReference.classification)
        assertEquals(null, privateReference.canonicalReference)
        assertFalse(privateReference.toString().contains(nsec))
    }

    @Test
    fun generatedRecoveryRequestIsOpaqueOneUseAndResolvedByIdentity() {
        val dataRoot = Files.createTempDirectory("harvestcircle-recovery-request-")
        val bridge = HarvestCircleTestBridge.open(dataRoot.toString())
        try {
            val initial = bridge.bootstrap()
            val request = bridge.beginGeneratedIdentity()
            val secret = request.takeRecoveryNsec()
            assertTrue(secret.startsWith("nsec1"))
            val secondRead = assertFailsWith<TestBridgeException.Failure> { request.takeRecoveryNsec() }
            assertFalse(secondRead.safeMessage.contains(secret))
            assertFalse(request.toString().contains(secret))

            val committed =
                bridge.acknowledgeGeneratedIdentity(
                    "00000000-0000-7000-8000-000000000011",
                    initial.revision,
                    2_000UL,
                    request,
                )
            assertEquals(request.identity().publicKeyHex, committed.selectedPublicKeyHex)
            val duplicateAcknowledge =
                assertFailsWith<TestBridgeException.Failure> {
                    bridge.acknowledgeGeneratedIdentity(
                        "00000000-0000-7000-8000-000000000012",
                        committed.revision,
                        2_000UL,
                        request,
                    )
                }
            assertFalse(duplicateAcknowledge.safeMessage.contains(secret))
            request.close()

            val cancelled = bridge.beginGeneratedIdentity()
            val cancelledSecret = cancelled.takeRecoveryNsec()
            assertTrue(bridge.cancelGeneratedIdentity(cancelled))
            assertFalse(bridge.cancelGeneratedIdentity(cancelled))
            assertFalse(cancelled.toString().contains(cancelledSecret))
            cancelled.close()

            val publicEvidence = bridge.snapshot().toString() + secondRead.safeMessage + duplicateAcknowledge.safeMessage
            assertFalse(publicEvidence.contains(secret))
            assertFalse(publicEvidence.contains(cancelledSecret))
            assertTreeDoesNotContain(dataRoot, secret, cancelledSecret)
            bridge.shutdown()
        } finally {
            bridge.close()
            deleteTree(dataRoot)
        }
    }

    @Test
    fun nativeRemovalRequestsRejectMissingStaleAndRapidDuplicateAuthority() {
        val dataRoot = Files.createTempDirectory("harvestcircle-removal-authority-")
        val bridge = HarvestCircleTestBridge.open(dataRoot.toString())
        try {
            val initial = bridge.bootstrap()
            val generated = bridge.beginGeneratedIdentity()
            val secret = generated.takeRecoveryNsec()
            val created =
                bridge.acknowledgeGeneratedIdentity(
                    "00000000-0000-7000-8000-000000000021",
                    initial.revision,
                    2_000UL,
                    generated,
                )
            val identityId = assertNotNull(created.selectedPublicKeyHex)
            generated.close()

            val missing =
                assertFailsWith<TestBridgeException.Failure> {
                    bridge.requestIdentityRemoval("04".repeat(32))
                }
            assertFalse(missing.safeMessage.contains(secret))

            val cancelled = bridge.requestIdentityRemoval(identityId)
            assertTrue(bridge.cancelIdentityRemoval(cancelled))
            assertFalse(bridge.cancelIdentityRemoval(cancelled))
            val stale =
                assertFailsWith<TestBridgeException.Failure> {
                    bridge.confirmIdentityRemoval(
                        "00000000-0000-7000-8000-000000000022",
                        created.revision,
                        2_000UL,
                        cancelled,
                    )
                }
            assertFalse(stale.safeMessage.contains(secret))
            cancelled.close()

            val admitted = bridge.requestIdentityRemoval(identityId)
            val removed =
                bridge.confirmIdentityRemoval(
                    "00000000-0000-7000-8000-000000000023",
                    created.revision,
                    2_000UL,
                    admitted,
                )
            assertTrue(removed.identities.isEmpty())
            val duplicate =
                assertFailsWith<TestBridgeException.Failure> {
                    bridge.confirmIdentityRemoval(
                        "00000000-0000-7000-8000-000000000024",
                        removed.revision,
                        2_000UL,
                        admitted,
                    )
                }
            assertFalse(duplicate.safeMessage.contains(secret))
            admitted.close()
            assertFalse(bridge.snapshot().toString().contains(secret))
            bridge.shutdown()
        } finally {
            bridge.close()
            deleteTree(dataRoot)
        }
    }

    @Test
    fun nativeBridgeCoversIdentityRelayRestartObserverTimeoutAndRedaction() =
        runBlocking {
            val dataRoot = Files.createTempDirectory("harvestcircle-native-integration-")
            try {
                TestBridgeHarvestCircleRuntime.open(dataRoot.toString()).use { runtime ->
                    verifyNativeCompatibility(compatibilityDescriptor())
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

                    assertTreeDoesNotContain(dataRoot, importedSecret, timeoutSecret, "nsec1")
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

private fun assertTreeDoesNotContain(
    root: Path,
    vararg values: String,
) {
    val needles = values.map(String::encodeToByteArray)
    try {
        Files.walk(root).use { paths ->
            paths
                .filter(Files::isRegularFile)
                .forEach { file ->
                    val bytes = file.readBytes()
                    needles.forEach { needle -> assertFalse(bytes.containsBytes(needle)) }
                }
        }
    } finally {
        needles.forEach { it.fill(0) }
    }
}

private fun deleteTree(root: Path) {
    Files.walk(root).use { paths ->
        paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
    }
}
