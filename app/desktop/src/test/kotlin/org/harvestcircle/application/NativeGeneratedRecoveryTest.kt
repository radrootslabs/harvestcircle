package org.harvestcircle.application

import kotlinx.coroutines.test.runTest
import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class NativeGeneratedRecoveryTest {
    private val dataDirectory =
        Path.of(checkNotNull(System.getProperty("harvestcircle.development.data.dir")))

    @BeforeTest
    fun prepareDataDirectory() {
        dataDirectory.toFile().deleteRecursively()
        Files.createDirectories(dataDirectory)
    }

    @AfterTest
    fun removeDataDirectory() {
        dataDirectory.toFile().deleteRecursively()
    }

    @Test
    fun generatedRecoveryCrossesTheNativeBoundaryAndCancelsWithoutPersistence() =
        runTest {
            val runtime = NativeHarvestCircleRuntime.open(developmentMode = true)
            try {
                runtime.bootstrap()
                val recovery = runtime.prepareLocalIdentity()

                assertTrue(recovery.identity.npub.startsWith("npub1"))
                assertTrue(recovery.backup.revealNsec().startsWith("nsec1"))
                runtime.execute(ApplicationCommand.CancelGeneratedIdentity(recovery.requestId))
                recovery.backup.clear()
                assertFalse(runtime.currentSnapshot().identities.isNotEmpty())
                assertEquals(0, runtime.currentSnapshot().identities.size)
            } finally {
                runtime.shutdown()
            }
        }
}
