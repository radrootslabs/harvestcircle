package org.harvestcircle.buildlogic.contracts

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class BuildContractsTest {
    @Test
    fun productCoordinatesAreCanonicalAndMatchTheMigrationAdapterFixture() {
        val coordinates = ProductCoordinates.parse(productCoordinates)

        assertEquals("HarvestCircle", coordinates["product.name"])
        assertEquals(
            "bf50f9ea6c2537406de255f025463e670eb6263c295f992f7e4c4db36d957064",
            coordinates.digest,
        )
        assertEquals(coordinates.digest, ProductCoordinates.parse(productCoordinates.replace("\n", "\r\n")).digest)
        assertEquals(coordinates.digest, ProductCoordinates.parse(productCoordinates.trimEnd()).digest)
        assertEquals(coordinates.digest, ProductCoordinates.parse("# comment\n$productCoordinates").digest)
        assertEquals(
            coordinates.digest,
            ProductCoordinates.parse(
                productCoordinates.lineSequence().joinToString("\n") { line ->
                    if (line.isBlank()) line else line.replaceFirst("=", " = ")
                },
            ).digest,
        )
        assertFails { ProductCoordinates.parse("\uFEFF$productCoordinates") }
        assertFails { ProductCoordinates.parse(productCoordinates + "schema=harvestcircle.product.v1\n") }
        assertFails { ProductCoordinates.parse(productCoordinates + "unknown=value\n") }
        assertFails { ProductCoordinates.parse(productCoordinates.substringAfter('\n')) }
        assertFails { ProductCoordinates.parse(productCoordinates.replace("product.name=HarvestCircle", "product.name")) }
        assertFails { ProductCoordinates.parse(productCoordinates.replaceCoordinate("product.slug", "INVALID")) }
        assertFails { ProductCoordinates.parse(productCoordinates.replaceCoordinate("storage.database_filename", "other.sqlite")) }
        assertFails { ProductCoordinates.parse(productCoordinates.replaceCoordinate("legacy.database.filename", "../other.sqlite3")) }
        assertFails { ProductCoordinates.parse(productCoordinates.replaceCoordinate("limit.identities", "257")) }
        assertFails { ProductCoordinates.parse(productCoordinates.replaceCoordinate("platform.linux.architecture", "aarch64")) }

        val validMutations =
            linkedMapOf(
                "product.name" to "Harvest Circle Test",
                "product.slug" to "harvestcircle_test",
                "kotlin.root_namespace" to "org.example",
                "desktop.application_id" to "org.example.desktop",
                "desktop.bundle_id" to "org.example.bundle",
                "desktop.main_class" to "org.example.MainKt",
                "ffi.kotlin_package" to "org.example.ffi",
                "ffi.cdylib_name" to "example_ffi",
                "keyring.service" to "org.example.desktop.nostr",
                "environment.prefix" to "EXAMPLE_",
                "vendor.name" to "Example Cooperative",
                "copyright.notice" to "Copyright Example contributors",
            )
        assertTrue(ProductCoordinates.requiredKeys.size > validMutations.size)
        validMutations.forEach { (key, replacement) ->
            val mutated = ProductCoordinates.parse(productCoordinates.replaceCoordinate(key, replacement))
            assertEquals(replacement, mutated[key])
            assertTrue(mutated.digest != coordinates.digest)
        }
    }

    @Test
    fun ffiBaselineRejectsMalformedStaleAndUnknownValues() {
        val baseline = FfiCompatibilityBaseline.parse(ffiBaseline)

        assertEquals("harvestcircle-desktop-ffi-v4", baseline["contract.id"])
        assertFails { FfiCompatibilityBaseline.parse("\uFEFF$ffiBaseline") }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline + "unknown=value\n") }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline + "contract.id=harvestcircle-desktop-ffi-v4\n") }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.substringAfter('\n')) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("contract.id=", "contract.id")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("contract.hash=${"a".repeat(64)}", "contract.hash=bad")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("product.version=0.1.0-alpha", "product.version=invalid")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("package.version=1.0.0", "package.version=invalid")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("schema=harvestcircle.ffi.v4", "schema=harvestcircle.ffi.v3")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("contract.major=4", "contract.major=3")) }
        assertFails { FfiCompatibilityBaseline.parse(ffiBaseline.replace("contract.minor=3", "contract.minor=2")) }
    }

    @Test
    fun provenanceCanonicalizesImportOrderAndRejectsMalformedSources() {
        val canonical = SourceProvenance.parse(sourceProvenance)
        val reordered =
            sourceProvenance.replace(
                "component = \"domain\"\ncommit = \"${"b".repeat(40)}\"",
                "commit = \"${"b".repeat(40)}\"\ncomponent = \"domain\"",
            )

        assertEquals(canonical.digest, SourceProvenance.parse(sourceProvenance.replace("\n", "\r\n")).digest)
        assertEquals(canonical.digest, SourceProvenance.parse("# comment\n$sourceProvenance").digest)
        assertEquals(canonical.digest, SourceProvenance.parse(reordered).digest)
        assertEquals("a".repeat(40), canonical.foundationBaseline)
        assertFails { SourceProvenance.parse("\uFEFF$sourceProvenance") }
        assertFails { SourceProvenance.parse("unknown = \"value\"\n$sourceProvenance") }
        assertFails { SourceProvenance.parse(sourceProvenance.replace("schema = ", "schema ")) }
        assertFails { SourceProvenance.parse(sourceProvenance.substringAfter('\n')) }
        assertFails { SourceProvenance.parse(sourceProvenance.replace("source_product = \"HarvestCircle\"", "source_product = \"HarvestCircle\"\nsource_product = \"HarvestCircle\"")) }
        assertFails { SourceProvenance.parse(sourceProvenance.replace("b".repeat(40), "BAD")) }
        assertFails {
            SourceProvenance.parse(
                sourceProvenance + "\n[[import]]\ncomponent = \"domain\"\ncommit = \"${"d".repeat(40)}\"\n",
            )
        }
        assertTrue(
            SourceProvenance.parse(sourceProvenance.replace("b".repeat(40), "d".repeat(40))).digest != canonical.digest,
        )
    }

    @Test
    fun nativeTargetMatrixIsExplicitAndFailsClosed() {
        val expected =
            mapOf(
                ("macOS" to "arm64") to NativeTarget("libharvestcircle_ffi.dylib", "darwin-aarch64"),
                ("Mac OS X" to "amd64") to NativeTarget("libharvestcircle_ffi.dylib", "darwin-x86-64"),
                ("Windows 11" to "aarch64") to NativeTarget("harvestcircle_ffi.dll", "win32-aarch64"),
                ("Windows" to "x86_64") to NativeTarget("harvestcircle_ffi.dll", "win32-x86-64"),
                ("Linux" to "arm64") to NativeTarget("libharvestcircle_ffi.so", "linux-aarch64"),
                ("linux" to "amd64") to NativeTarget("libharvestcircle_ffi.so", "linux-x86-64"),
            )

        expected.forEach { (host, target) ->
            assertEquals(target, resolveNativeTarget(host.first, host.second, "harvestcircle_ffi"))
        }
        assertFails { resolveNativeTarget("Solaris", "sparc", "harvestcircle_ffi") }
        assertFails { resolveNativeTarget("Linux", "riscv64", "harvestcircle_ffi") }
    }

    @Test
    fun generatedKotlinIsDeterministicEscapedAndFreshnessChecked() {
        val metadata =
            GeneratedKotlin.desktopBuildMetadata(
                DesktopBuildMetadataValues(
                    productVersion = "0.1.0-alpha",
                    distributionPackageVersion = "1.0.0",
                    gradleToolchain = "9.5.0",
                    javaToolchain = "21",
                    kotlinToolchain = "2.4.10\"test",
                    composeMultiplatformVersion = "1.11.1",
                ),
            )
        val compatibility = GeneratedKotlin.compatibilityExpectations(FfiCompatibilityBaseline.parse(ffiBaseline))

        assertTrue(metadata.startsWith("// @generated by the HarvestCircle Gradle build. Do not edit.\n"))
        assertTrue(metadata.contains("2.4.10\\\"test"))
        assertTrue(metadata.endsWith("\n"))
        assertTrue(compatibility.contains("const val ffiContractHash = \"${"a".repeat(64)}\""))
        GeneratedKotlin.requireFresh(metadata, metadata, "metadata")
        assertFails { GeneratedKotlin.requireFresh(null, metadata, "metadata") }
        assertFails { GeneratedKotlin.requireFresh("$metadata// stale", metadata, "metadata") }
    }

    @Test
    fun contractsHaveNoGradleApiOnTheirRuntimeClasspath() {
        assertFalse(runCatching { Class.forName("org.gradle.api.Project") }.isSuccess)
    }

    private val productCoordinates =
        """
        schema=harvestcircle.product.v1

        product.name=HarvestCircle
        product.slug=harvestcircle

        kotlin.root_namespace=org.harvestcircle
        desktop.application_id=org.harvestcircle.desktop
        desktop.bundle_id=org.harvestcircle.desktop
        desktop.main_class=org.harvestcircle.desktop.MainKt

        ffi.kotlin_package=org.harvestcircle.ffi
        ffi.cdylib_name=harvestcircle_ffi

        storage.service_id=harvestcircle
        storage.instance_id=desktop
        storage.database_filename=state.sqlite
        storage.lock_filename=state.lock
        storage.application_id=1212371505
        storage.application_id_text=HCR1
        storage.initial_schema_version=1

        legacy.database.qualifier=org
        legacy.database.organization=harvestcircle
        legacy.database.application=desktop
        legacy.database.filename=harvestcircle.sqlite3
        legacy.database.disposition=untouched_and_unsupported

        platform.macos.architecture=aarch64
        platform.linux.architecture=x86_64

        limit.identities=256
        limit.unfinished_durable_operations=1024
        limit.preference_value_utf8_bytes=4096
        limit.relay_endpoints=16
        limit.relay_url_bytes=2048
        limit.events_per_relay=64
        limit.events_total=1024
        limit.observers=32
        limit.actor_mailbox=64
        limit.command_deadline_min_ms=1
        limit.command_deadline_max_ms=30000
        backup.member_limit=caller_supplied_positive

        keyring.service=org.harvestcircle.desktop.nostr
        environment.prefix=HARVESTCIRCLE_

        vendor.name=Radroots Labs
        copyright.notice=Copyright © 2026 HarvestCircle contributors
        """.trimIndent() + "\n"

    private val ffiBaseline =
        """
        schema=harvestcircle.ffi.v4
        contract.id=harvestcircle-desktop-ffi-v4
        contract.major=4
        contract.minor=3
        contract.hash=${"a".repeat(64)}
        product.coordinate_digest=${"b".repeat(64)}
        snapshot.schema=1
        storage.schema.minimum=1
        storage.schema.current=1
        product.version=0.1.0-alpha
        package.version=1.0.0
        source.provenance_digest=${"c".repeat(64)}
        source.foundation_baseline=${"d".repeat(40)}
        """.trimIndent() + "\n"

    private val sourceProvenance =
        """
        schema = "harvestcircle.source_provenance.v1"
        source_product = "HarvestCircle"
        source_repository = "https://example.invalid/harvestcircle"
        foundation_baseline = "${"a".repeat(40)}"
        canonical_radroots_repository = "https://example.invalid/radroots"
        canonical_radroots_revision = "${"c".repeat(40)}"

        [[import]]
        component = "domain"
        commit = "${"b".repeat(40)}"
        """.trimIndent() + "\n"

    private fun String.replaceCoordinate(
        key: String,
        replacement: String,
    ): String =
        lineSequence().joinToString("\n") { line ->
            if (line.substringBefore('=', missingDelimiterValue = "") == key) "$key=$replacement" else line
        }
}
