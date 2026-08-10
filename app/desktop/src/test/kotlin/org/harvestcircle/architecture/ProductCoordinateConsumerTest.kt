package org.harvestcircle.architecture

import java.nio.file.Files
import java.nio.file.Path
import java.util.Properties
import kotlin.io.path.inputStream
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ProductCoordinateConsumerTest {
    @Test
    fun finalManifestDrivesBuildNativeStorageAndKeyringCoordinates() {
        val root = findRepositoryRoot()
        val coordinates =
            Properties().apply {
                root.resolve("config/product/harvestcircle-v1.properties").inputStream().use(::load)
            }

        assertEquals("org.harvestcircle", coordinates.getProperty("kotlin.root_namespace"))
        assertEquals("org.harvestcircle.desktop", coordinates.getProperty("desktop.application_id"))
        assertEquals("org.harvestcircle.desktop", coordinates.getProperty("desktop.bundle_id"))
        assertEquals("org.harvestcircle.desktop.MainKt", coordinates.getProperty("desktop.main_class"))
        assertEquals("org.harvestcircle.ffi", coordinates.getProperty("ffi.kotlin_package"))
        assertEquals("harvestcircle", coordinates.getProperty("database.organization"))
        assertEquals("desktop", coordinates.getProperty("database.application"))
        assertEquals("harvestcircle.sqlite3", coordinates.getProperty("database.filename"))
        assertEquals("org.harvestcircle.desktop.nostr", coordinates.getProperty("keyring.service"))

        val build =
            listOf(
                "app/desktop/build.gradle.kts",
                "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCircleDesktopAppPlugin.kt",
                "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCircleRustFfiPlugin.kt",
                "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCirclePackagingPlugin.kt",
            ).joinToString("\n") { relativePath -> root.resolve(relativePath).readText() }
        assertTrue(build.contains("ProductCoordinates.load"))
        assertTrue(build.contains("application.mainClass = mainClass"))
        assertTrue(build.contains("mac.bundleID = bundleId"))
        assertTrue(build.contains("expectedPackage.set(productCoordinates[\"ffi.kotlin_package\"])"))
        assertFalse(Files.exists(root.resolve("core/compatibility/v5-baseline.properties")))
    }
}

private fun findRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("config/product/harvestcircle-v1.properties")) }
