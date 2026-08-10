package org.harvestcircle.application

import kotlinx.coroutines.test.runTest
import org.harvestcircle.ffi.HarvestCircleAppCore
import org.harvestcircle.ffi.compatibilityDescriptor
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
            val core =
                HarvestCircleAppCore.openCompatible(
                    expectation = verifyNativeCompatibility(compatibilityDescriptor()),
                    developmentMode = true,
                )
            val gateway = NativeHarvestCircleCoreGateway(core)
            try {
                gateway.bootstrap()
                val recovery = gateway.beginGeneratedIdentity()

                assertTrue(recovery.identity.npub.startsWith("npub1"))
                assertTrue(recovery.takeRecoveryNsec().startsWith("nsec1"))
                assertTrue(recovery.cancel())
                assertFalse(recovery.cancel())
                assertEquals(0, gateway.snapshot().identities.size)
                recovery.close()
            } finally {
                gateway.shutdown()
            }
        }
}
