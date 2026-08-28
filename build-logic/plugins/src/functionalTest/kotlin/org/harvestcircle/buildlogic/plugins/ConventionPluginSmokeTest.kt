package org.harvestcircle.buildlogic.plugins

import org.gradle.testkit.runner.GradleRunner
import org.gradle.testkit.runner.UnexpectedBuildFailure
import kotlin.io.path.createTempDirectory
import kotlin.io.path.createDirectories
import kotlin.io.path.writeText
import kotlin.test.Test
import kotlin.test.assertTrue

class ConventionPluginSmokeTest {
    @Test
    fun everyConventionPluginAppliesToAnIsolatedBuild() {
        val temporaryDirectory = createTempDirectory("harvestcircle-build-logic-")
        val pluginIds =
            listOf(
                "org.harvestcircle.build.root",
                "org.harvestcircle.build.kmp-shared",
                "org.harvestcircle.build.design-system",
                "org.harvestcircle.build.design-catalog",
                "org.harvestcircle.build.desktop-app",
                "org.harvestcircle.build.rust-ffi",
                "org.harvestcircle.build.packaging",
            )

        pluginIds.forEach { pluginId ->
            val fixture = temporaryDirectory.resolve(pluginId.substringAfterLast('.')).createDirectories()
            val desktopFixture =
                pluginId in
                    setOf(
                        "org.harvestcircle.build.desktop-app",
                        "org.harvestcircle.build.rust-ffi",
                        "org.harvestcircle.build.packaging",
                    )
            fixture.resolve("settings.gradle.kts").writeText(
                buildString {
                    append("pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }\n")
                    append("dependencyResolutionManagement { repositories { mavenCentral() } }\n")
                    append("rootProject.name = \"fixture\"\n")
                    if (pluginId == "org.harvestcircle.build.design-system") {
                        append("include(\":app:design_system\")\n")
                    }
                    if (pluginId == "org.harvestcircle.build.kmp-shared") {
                        append("include(\":app:design_system\", \":app:shared\")\n")
                    }
                    if (pluginId == "org.harvestcircle.build.design-catalog") {
                        append("include(\":app:design_system\", \":tools:design_catalog\")\n")
                    }
                    if (desktopFixture) append("include(\":app:design_system\", \":app:shared\", \":app:desktop\")\n")
                },
            )
            if (
                pluginId == "org.harvestcircle.build.kmp-shared" ||
                pluginId.startsWith("org.harvestcircle.build.design-") ||
                desktopFixture
            ) {
                fixture.resolve("gradle").createDirectories().resolve("libs.versions.toml").writeText(kmpCatalog)
            }
            val buildFile =
                if (desktopFixture) {
                    prepareDesktopFixture(fixture)
                    fixture.resolve("app/desktop/build.gradle.kts")
                } else if (pluginId == "org.harvestcircle.build.design-system") {
                    fixture.resolve("app/design_system").createDirectories()
                        .resolve("build.gradle.kts")
                } else if (pluginId == "org.harvestcircle.build.kmp-shared") {
                    fixture.resolve("app/design_system").createDirectories()
                        .resolve("build.gradle.kts")
                        .writeText("plugins { `java-library` }\n")
                    fixture.resolve("app/shared").createDirectories()
                        .resolve("build.gradle.kts")
                } else if (pluginId == "org.harvestcircle.build.design-catalog") {
                    fixture.resolve("app/design_system").createDirectories()
                        .resolve("build.gradle.kts")
                        .writeText("plugins { `java-library` }\n")
                    fixture.resolve("tools/design_catalog").createDirectories()
                        .resolve("build.gradle.kts")
                } else {
                    fixture.resolve("build.gradle.kts")
                }
            val pluginBlock =
                when (pluginId) {
                    "org.harvestcircle.build.kmp-shared" -> kmpPlugins
                    "org.harvestcircle.build.rust-ffi" ->
                        "id(\"org.harvestcircle.build.desktop-app\")\nid(\"$pluginId\")"
                    "org.harvestcircle.build.packaging" ->
                        """
                        id("org.harvestcircle.build.desktop-app")
                        id("org.harvestcircle.build.rust-ffi")
                        id("$pluginId")
                        """.trimIndent()
                    else -> "id(\"$pluginId\")"
                }
            buildFile.writeText("plugins { $pluginBlock }\n")

            val runner =
                GradleRunner.create()
                    .withProjectDir(fixture.toFile())
                    .withPluginClasspath()
                    .withArguments("tasks", "--configuration-cache", "--configuration-cache-problems=fail", "--stacktrace")
            val result = runner.build()

            assertTrue(result.output.contains("BUILD SUCCESSFUL"), pluginId)
            if (pluginId == "org.harvestcircle.build.root") {
                assertTrue(result.output.contains("verifyProductCoordinates"), result.output)
            }
            assertTrue(runner.build().output.contains("Reusing configuration cache"), pluginId)
        }
    }

    @Test
    fun rootPluginFailsClosedWhenTheProductManifestIsMissing() {
        val fixture = createTempDirectory("harvestcircle-root-missing-manifest-")
        fixture.resolve("settings.gradle.kts").writeText("rootProject.name = \"fixture\"\n")
        fixture.resolve("build.gradle.kts").writeText("plugins { id(\"org.harvestcircle.build.root\") }\n")

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments("verifyProductCoordinates", "--stacktrace")
                .buildAndFail()

        assertTrue(result.output.contains("harvestcircle-v1.properties"), result.output)
    }

    private fun prepareDesktopFixture(
        fixture: java.nio.file.Path,
        withUnitTest: Boolean = true,
    ) {
        fixture.resolve("build.gradle.kts").writeText("plugins { id(\"org.harvestcircle.build.root\") }\n")
        fixture.resolve("app/design_system").createDirectories().resolve("build.gradle.kts")
            .writeText("plugins { `java-library` }\n")
        fixture.resolve("app/shared").createDirectories().resolve("build.gradle.kts").writeText("plugins { `java-library` }\n")
        fixture.resolve("app/desktop").createDirectories()
        fixture.resolve("config/product").createDirectories().resolve("harvestcircle-v1.properties").writeText(productCoordinates)
        fixture.resolve("config/licenses").createDirectories().resolve("allowed-licenses.json").writeText("{}\n")
        fixture.resolve("core/compatibility").createDirectories().resolve("harvestcircle-ffi-v4.properties").writeText(ffiBaseline)
        fixture.resolve("core").resolve("Cargo.toml").writeText(
            "[workspace.package]\nversion = \"0.1.0-alpha\"\n",
        )
        if (withUnitTest) {
            fixture.resolve("app/desktop/src/test/kotlin").createDirectories().resolve("FixtureTest.kt").writeText(
                "class FixtureTest\n",
            )
        }
    }

    @Test
    fun rootPluginRejectsApplicationToASubproject() {
        val fixture = createTempDirectory("harvestcircle-root-plugin-")
        fixture.resolve("settings.gradle.kts").writeText("rootProject.name = \"fixture\"\ninclude(\":child\")\n")
        fixture.resolve("build.gradle.kts").writeText("// root intentionally has no convention plugin\n")
        val child = fixture.resolve("child").createDirectories()
        child.resolve("build.gradle.kts").writeText("plugins { id(\"org.harvestcircle.build.root\") }\n")

        val failure =
            runCatching {
                GradleRunner.create()
                    .withProjectDir(fixture.toFile())
                    .withPluginClasspath()
                    .withArguments("tasks", "--stacktrace")
                    .build()
            }.exceptionOrNull()

        assertTrue(failure is UnexpectedBuildFailure)
        assertTrue(failure.message.orEmpty().contains("may only be applied to the root project"))
    }

    @Test
    fun sharedPluginRejectsPlatformDependenciesFromCommonSources() {
        val fixture = createTempDirectory("harvestcircle-shared-plugin-")
        fixture.resolve("settings.gradle.kts").writeText(
            "pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }\n" +
                "rootProject.name = \"fixture\"\n" +
                "include(\":app:design_system\", \":app:shared\")\n",
        )
        fixture.resolve("gradle").createDirectories().resolve("libs.versions.toml").writeText(kmpCatalog)
        fixture.resolve("app/design_system").createDirectories().resolve("build.gradle.kts")
            .writeText("plugins { `java-library` }\n")
        fixture.resolve("app/shared").createDirectories().resolve("build.gradle.kts")
            .writeText("plugins { id(\"org.harvestcircle.build.kmp-shared\") }\n")
        fixture.resolve("app/shared/src/commonMain/kotlin").createDirectories().resolve("Leak.kt").writeText(
            "package fixture\nimport org.harvestcircle.ffi.BuildInfoDto\n",
        )

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments("verifySharedBoundary", "--stacktrace")
                .buildAndFail()

        assertTrue(result.output.contains("prohibited common-source dependency"), result.output)
    }

    @Test
    fun desktopPluginPublishesTheApplicationAndIntegrationContracts() {
        val fixture = createTempDirectory("harvestcircle-desktop-plugin-")
        prepareDesktopBuild(fixture, withUnitTest = true)

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(":app:desktop:tasks", "--all", "--stacktrace")
                .build()

        listOf(
            "checkLicense",
            "dependencyCheckAnalyze",
            "generateDesktopBuildMetadata",
            "verifyGeneratedDesktopBuildMetadata",
            "verifyTestInventory",
            "compileIntegrationTestKotlin",
            "integrationTest",
            "compileHostUiTestKotlin",
            "hostUiLifecycleTest",
        ).forEach { taskName -> assertTrue(result.output.contains(taskName), result.output) }
    }

    @Test
    fun desktopPluginRejectsAnEmptyUnitTestInventory() {
        val fixture = createTempDirectory("harvestcircle-desktop-no-tests-")
        prepareDesktopBuild(fixture, withUnitTest = false)

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(":app:desktop:verifyTestInventory", "--stacktrace")
                .buildAndFail()

        assertTrue(result.output.contains("No Kotlin tests found under src/test/kotlin"), result.output)
    }

    @Test
    fun rustPluginRejectsAnUnsupportedNativeTarget() {
        val fixture = createTempDirectory("harvestcircle-rust-target-")
        prepareRustBuild(fixture)

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(":app:desktop:tasks", "-PnativeOs=plan9", "-PnativeArch=mips", "--stacktrace")
                .buildAndFail()

        assertTrue(result.output.contains("Unsupported native desktop host: plan9/mips"), result.output)
    }

    @Test
    fun rustPluginRejectsStaleGeneratedCompatibilityKotlin() {
        val fixture = createTempDirectory("harvestcircle-rust-stale-")
        prepareRustBuild(
            fixture,
            """
            tasks.named<org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedCompatibilityExpectations>(
                "verifyGeneratedSources",
            ) {
                generatedFile.set(layout.projectDirectory.file("stale.kt"))
            }
            """.trimIndent(),
        )
        fixture.resolve("app/desktop/stale.kt").writeText("// stale\n")

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(
                    ":app:desktop:verifyGeneratedSources",
                    "-x",
                    ":app:desktop:generateCompatibilityExpectations",
                    "--stacktrace",
                )
                .buildAndFail()

        assertTrue(result.output.contains("Generated Kotlin compatibility expectations is stale"), result.output)
    }

    @Test
    fun rustPluginRejectsNativeByteAndProvenanceMismatches() {
        listOf(
            Triple("bytes", "release", "staged"),
            Triple("provenance", "same", "same"),
        ).forEach { (caseName, releaseBytes, stagedBytes) ->
            val fixture = createTempDirectory("harvestcircle-rust-$caseName-")
            prepareRustBuild(
                fixture,
                """
                tasks.named<org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseNativeLibrary>(
                    "verifyReleaseNativeLibrary",
                ) {
                    releaseLibrary.set(layout.projectDirectory.file("native/release/libfixture.dylib"))
                    stagedDirectory.set(layout.projectDirectory.dir("native/staged"))
                    expectedName.set("libfixture.dylib")
                    expectedBuildEvidence.set(listOf("required-proof"))
                }
                """.trimIndent(),
            )
            fixture
                .resolve("app/desktop/native/release")
                .createDirectories()
                .resolve("libfixture.dylib")
                .writeText(releaseBytes)
            fixture
                .resolve("app/desktop/native/staged/darwin-aarch64")
                .createDirectories()
                .resolve("libfixture.dylib")
                .writeText(stagedBytes)

            val result =
                GradleRunner.create()
                    .withProjectDir(fixture.toFile())
                    .withPluginClasspath()
                    .withArguments(
                        ":app:desktop:verifyReleaseNativeLibrary",
                        "-x",
                        ":app:desktop:stageReleaseNativeLibrary",
                        "--stacktrace",
                    )
                    .buildAndFail()

            val expected =
                if (caseName == "bytes") {
                    "Staged native library does not match the Cargo release artifact"
                } else {
                    "Release native library is missing build provenance evidence"
                }
            assertTrue(result.output.contains(expected), result.output)
        }
    }

    @Test
    fun packagingPluginLaunchesAndClosesThePackagedHealthEntry() {
        val fixture = createTempDirectory("harvestcircle-package-health-")
        preparePackagingBuild(
            fixture,
            "test -n \"\${HARVESTCIRCLE_DEVELOPMENT_DATA_DIR:-}\" && " +
                "test -d \"\$HARVESTCIRCLE_DEVELOPMENT_DATA_DIR\" && " +
                "test -z \"\$(find \"\$HARVESTCIRCLE_DEVELOPMENT_DATA_DIR\" -mindepth 1 -print -quit)\" && " +
                "printf 'HARVESTCIRCLE_HEALTH_READY\\nHARVESTCIRCLE_HEALTH_CLOSED\\n'",
        )

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(
                    ":app:desktop:verifyPackagedApplicationHealth",
                    "-x",
                    ":app:desktop:createDistributable",
                    "--stacktrace",
                )
                .build()

        assertTrue(result.output.contains("BUILD SUCCESSFUL"), result.output)
    }

    @Test
    fun packagingPluginRejectsMissingCloseEvidence() {
        assertPackagingHealthFailure(
            caseName = "close",
            scriptBody = "printf 'HARVESTCIRCLE_HEALTH_READY\\n'",
            expected = "did not report closed health evidence",
        )
    }

    @Test
    fun packagingPluginRejectsHealthTimeout() {
        assertPackagingHealthFailure(
            caseName = "timeout",
            scriptBody = "sleep 5",
            expected = "health-check timed out",
            timeoutSeconds = 1L,
        )
    }

    @Test
    fun packagingPluginRejectsSecretHealthOutput() {
        assertPackagingHealthFailure(
            caseName = "redaction",
            scriptBody =
                "printf 'nsec1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa " +
                    "HARVESTCIRCLE_HEALTH_READY HARVESTCIRCLE_HEALTH_CLOSED\\n'",
            expected = "emitted secret material",
        )
    }

    @Test
    fun packagingPluginRejectsResidualHealthData() {
        assertPackagingHealthFailure(
            caseName = "residue",
            scriptBody =
                "touch \"\$HARVESTCIRCLE_DEVELOPMENT_DATA_DIR/leftover\"; " +
                    "printf 'HARVESTCIRCLE_HEALTH_READY\\nHARVESTCIRCLE_HEALTH_CLOSED\\n'",
            expected = "did not clean its isolated health data root",
        )
    }

    private fun assertPackagingHealthFailure(
        caseName: String,
        scriptBody: String,
        expected: String,
        timeoutSeconds: Long = 30L,
    ) {
        val fixture = createTempDirectory("harvestcircle-package-$caseName-")
        preparePackagingBuild(fixture, scriptBody, timeoutSeconds = timeoutSeconds)

        val result =
            GradleRunner.create()
                .withProjectDir(fixture.toFile())
                .withPluginClasspath()
                .withArguments(
                    ":app:desktop:verifyPackagedApplicationHealth",
                    "-x",
                    ":app:desktop:createDistributable",
                    "--stacktrace",
                )
                .buildAndFail()

        assertTrue(result.output.contains(expected), result.output)
    }

    @Test
    fun packagingPluginAcceptsGovernedProvenanceAndRejectsUnknownInputs() {
        val governed = createTempDirectory("harvestcircle-package-governed-provenance-")
        preparePackagingBuild(governed, "exit 0")
        val governedEnvironment =
            System.getenv() +
                mapOf(
                    "HARVESTCIRCLE_BUILD_SOURCE_COMMIT" to "a".repeat(40),
                    "HARVESTCIRCLE_BUILD_SOURCE_DIRTY" to "false",
                    "HARVESTCIRCLE_BUILD_RADROOTS_REVISION" to "b".repeat(40),
                    "SOURCE_DATE_EPOCH" to "1770000000",
                )
        val success =
            GradleRunner.create()
                .withProjectDir(governed.toFile())
                .withPluginClasspath()
                .withEnvironment(governedEnvironment)
                .withArguments(":app:desktop:verifyReleaseBuildProvenance", "--stacktrace")
                .build()
        assertTrue(success.output.contains("BUILD SUCCESSFUL"), success.output)

        val standalone = createTempDirectory("harvestcircle-package-unknown-provenance-")
        preparePackagingBuild(standalone, "exit 0")
        val failure =
            GradleRunner.create()
                .withProjectDir(standalone.toFile())
                .withPluginClasspath()
                .withEnvironment(
                    System.getenv() -
                        setOf(
                            "HARVESTCIRCLE_BUILD_SOURCE_COMMIT",
                            "HARVESTCIRCLE_BUILD_SOURCE_DIRTY",
                            "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
                            "SOURCE_DATE_EPOCH",
                        ),
                ).withArguments(":app:desktop:verifyReleaseBuildProvenance", "--stacktrace")
                .buildAndFail()
        assertTrue(failure.output.contains("Release source commit provenance is unknown or malformed"), failure.output)
    }

    private fun prepareDesktopBuild(
        fixture: java.nio.file.Path,
        withUnitTest: Boolean,
    ) {
        fixture.resolve("settings.gradle.kts").writeText(
            """
            pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }
            dependencyResolutionManagement { repositories { mavenCentral() } }
            rootProject.name = "fixture"
            include(":app:design_system", ":app:shared", ":app:desktop")
            """.trimIndent() + "\n",
        )
        fixture.resolve("gradle").createDirectories().resolve("libs.versions.toml").writeText(kmpCatalog)
        prepareDesktopFixture(fixture, withUnitTest)
        fixture.resolve("app/desktop/build.gradle.kts").writeText(
            "plugins { id(\"org.harvestcircle.build.desktop-app\") }\n",
        )
    }

    private fun prepareRustBuild(
        fixture: java.nio.file.Path,
        extraBuildLogic: String = "",
    ) {
        prepareDesktopBuild(fixture, withUnitTest = true)
        fixture.resolve("app/desktop/build.gradle.kts").writeText(
            """
            plugins {
                id("org.harvestcircle.build.desktop-app")
                id("org.harvestcircle.build.rust-ffi")
            }

            $extraBuildLogic
            """.trimIndent() + "\n",
        )
    }

    private fun preparePackagingBuild(
        fixture: java.nio.file.Path,
        scriptBody: String,
        timeoutSeconds: Long = 30L,
    ) {
        prepareDesktopBuild(fixture, withUnitTest = true)
        val executable = fixture.resolve("app/desktop/fixture-health.sh")
        executable.writeText("#!/bin/sh\n$scriptBody\n")
        check(executable.toFile().setExecutable(true))
        fixture.resolve("app/desktop/build.gradle.kts").writeText(
            """
            plugins {
                id("org.harvestcircle.build.desktop-app")
                id("org.harvestcircle.build.rust-ffi")
                id("org.harvestcircle.build.packaging")
            }

            tasks.named<org.harvestcircle.buildlogic.plugins.tasks.VerifyPackagedApplicationHealth>(
                "verifyPackagedApplicationHealth",
            ) {
                executable.set(layout.projectDirectory.file("fixture-health.sh"))
                timeoutSeconds.set(${timeoutSeconds}L)
            }
            """.trimIndent() + "\n",
        )
    }

    private val kmpCatalog =
        """
        [versions]
        kotlin = "2.4.10"
        compose = "1.11.1"
        coroutines = "1.9.0"

        [libraries]
        compose-animation = { module = "org.jetbrains.compose.animation:animation", version.ref = "compose" }
        compose-components-resources = { module = "org.jetbrains.compose.components:components-resources", version.ref = "compose" }
        compose-foundation = { module = "org.jetbrains.compose.foundation:foundation", version.ref = "compose" }
        compose-runtime = { module = "org.jetbrains.compose.runtime:runtime", version.ref = "compose" }
        compose-ui = { module = "org.jetbrains.compose.ui:ui", version.ref = "compose" }
        compose-ui-test-junit4 = { module = "org.jetbrains.compose.ui:ui-test-junit4", version.ref = "compose" }
        kotlinx-coroutines-core = { module = "org.jetbrains.kotlinx:kotlinx-coroutines-core", version.ref = "coroutines" }
        kotlinx-coroutines-test = { module = "org.jetbrains.kotlinx:kotlinx-coroutines-test", version.ref = "coroutines" }
        jna = { module = "net.java.dev.jna:jna", version = "5.17.0" }
        """.trimIndent() + "\n"

    private val kmpPlugins =
        """
        id("org.harvestcircle.build.kmp-shared")
        """.trimIndent()

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
        contract.hash=b32b9a47d12e445e93866ae0ab668b18de503ba6c999e3a053f26dc9509ddaf9
        product.coordinate_digest=bf50f9ea6c2537406de255f025463e670eb6263c295f992f7e4c4db36d957064
        snapshot.schema=1
        storage.schema.minimum=1
        storage.schema.current=2
        product.version=0.1.0-alpha
        package.version=1.0.0
        source.provenance_digest=40b9eccd486026128f92de8d55d002a9030f235a35f9b754c98c0b0d387bd8c0
        source.foundation_baseline=c08d18ea569351dddeef70d4c1410708daf067b6
        """.trimIndent() + "\n"
}
