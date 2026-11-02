package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubCountObserveInput
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.NativeSessionBinding
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.toApplicationSnapshot
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.testbridge.ffi.TestSession
import org.harvestcircle.testbridge.ffi.sessionBindingEncodingProbe
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SessionBindingAcceptanceTest {
    @Test
    fun nativeSameOwnerReturnKeepsHistoricalMarkersAndRejectsTheOldRenderedSnapshot(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-session-binding-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val before = runtime.currentSnapshot()
                val originalBinding = assertNotNull(before.runtimeSession)
                val group = runtime.group(hub.receipt.initialPosition.groupId)
                val originalContext = hub.context()
                val input =
                    HubCountObserveInput(
                        group.stockRevision,
                        group.catalogRevision,
                        group.group.id,
                        group.group.revision,
                        18U,
                        HubOccurrence(HubOccurrenceKind.Known, 1_700_000_000L),
                        "Original rendered count",
                    )
                val bridge = runtime.nativeBridge()
                bridge.startObserver()
                assertEquals(originalBinding.generation, assertNotNull(bridge.nextObservedSnapshot(1_000UL)).runtimeSession?.generation)
                runtime.execute(ApplicationCommand.SignOut)
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                val after = runtime.currentSnapshot()
                val replacement = assertNotNull(after.runtimeSession)
                assertEquals(before.activeIdentity?.identity?.id, after.activeIdentity?.identity?.id)
                assertEquals(originalBinding.incarnation, replacement.incarnation)
                assertTrue(replacement.generation > originalBinding.generation)
                val delayedSignout = assertNotNull(bridge.nextObservedSnapshot(1_000UL))
                assertEquals(TestSession.SIGNED_OUT, delayedSignout.session)
                assertEquals(originalBinding.generation + 1UL, delayedSignout.runtimeSession?.generation)
                val delayedReturn = assertNotNull(bridge.nextObservedSnapshot(1_000UL))
                assertEquals(TestSession.ACTIVE, delayedReturn.session)
                assertEquals(replacement.generation, delayedReturn.runtimeSession?.generation)
                val failure = assertFailsWith<ApplicationFailure> { runtime.observeCount(originalContext, input) }
                assertEquals(ApplicationErrorCode.HubConflict, failure.problem.code)
                assertNull(runtime.countObservationOperation(originalContext.operationId))
                val stock = runtime.stock(HubStockQuery()).rows.single()
                assertEquals(20U, stock.position.wholePacks)
                assertEquals(20U, stock.putAway?.remaining)
                bridge.stopObserver()
                val reopened = runtime.restart()
                assertNotEquals(replacement.incarnation, assertNotNull(reopened.runtimeSession).incarnation)
                assertEquals(SessionLifecycle.SignedOut, reopened.session)
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun encodingOnlyNativeFixtureCarriesTheFullUnsignedGenerationThroughProductionMapping() {
        val encoded = sessionBindingEncodingProbe()
        val snapshot =
            AppSnapshotDto(
                revision = 0UL,
                runtimeSession = encoded,
                lifecycle = AppLifecycleDto.READY,
                lifecycleError = null,
                configuredRelays = emptyList(),
                identities = emptyList(),
                selectedPublicKeyHex = null,
                session = SessionStateDto.SIGNED_OUT,
                sessionSubjectPublicKeyHex = null,
                sessionError = null,
                activeIdentity = null,
                recoverableProblem = null,
            ).toApplicationSnapshot()
        assertEquals(NativeSessionBinding("ab".repeat(16), ULong.MAX_VALUE), snapshot.runtimeSession)
        assertNull(snapshot.activeIdentity)
    }
}
