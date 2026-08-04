package org.radroots.studio.application

import kotlinx.coroutines.test.runTest
import org.radroots.studio.ffi.StudioAppCore
import org.radroots.studio.ffi.compatibilityDescriptor
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
        Path.of(checkNotNull(System.getenv("RADROOTS_STUDIO_DEVELOPMENT_DATA_DIR")))

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
                StudioAppCore.openCompatible(
                    expectation = verifyNativeCompatibility(compatibilityDescriptor()),
                    developmentMode = true,
                )
            val gateway = NativeStudioCoreGateway(core)
            try {
                gateway.bootstrap()
                val recovery = gateway.beginGeneratedAccount()

                assertTrue(recovery.account.npub.startsWith("npub1"))
                assertTrue(recovery.takeRecoveryNsec().startsWith("nsec1"))
                assertTrue(recovery.cancel())
                assertFalse(recovery.cancel())
                assertEquals(0, gateway.snapshot().accounts.size)
                recovery.close()
            } finally {
                gateway.shutdown()
            }
        }
}
