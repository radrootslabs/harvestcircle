package org.radroots.studio.architecture

import java.nio.file.Files
import java.nio.file.Path
import java.util.Properties
import kotlin.io.path.inputStream
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class V5CompatibilityBaselineTest {
    @Test
    fun baselineFreezesPackageAndNativeCoordinates() {
        val root = findRepositoryRoot()
        val baseline =
            Properties().apply {
                root.resolve("core/compatibility/v5-baseline.properties").inputStream().use(::load)
            }

        assertEquals("studio-runtime-v5", baseline.getProperty("baseline.id"))
        assertEquals("5", baseline.getProperty("schema.version"))
        assertEquals("legacy-unversioned-v1", baseline.getProperty("ffi.contract"))
        assertEquals("1", baseline.getProperty("ffi.snapshot.schema"))
        assertEquals("0.1.0-alpha", baseline.getProperty("ffi.runtime.version"))
        assertEquals("org.radroots.studio", baseline.getProperty("package.namespace"))
        assertEquals("org.radroots.studio", baseline.getProperty("package.application_id"))
        assertEquals("Radroots", baseline.getProperty("package.name"))
        assertEquals("org.radroots.studio", baseline.getProperty("package.bundle_id"))
        assertEquals("1.0.0", baseline.getProperty("package.version"))
        assertEquals("org.radroots.studio.nostr", baseline.getProperty("keyring.service"))
        assertEquals(
            "canonical-lowercase-public-key-hex",
            baseline.getProperty("keyring.account"),
        )

        val manifest = root.resolve("core/Cargo.toml").readText()
        val workspacePackage = manifest.substringAfter("[workspace.package]").substringBefore("\n[")
        assertTrue(workspacePackage.contains("version = \"${baseline.getProperty("ffi.runtime.version")}\""))
        val build = root.resolve("app/desktop/build.gradle.kts").readText()
        assertTrue(build.contains("version = appVersion"))
        assertTrue(build.contains("packageName = applicationName"))
        assertTrue(build.contains("bundleID = bundleId"))
        assertTrue(build.contains("packageVersion = installableVersion"))
    }
}

private fun findRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("core/compatibility/v5-baseline.properties")) }
