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
                "org.harvestcircle.build.desktop-app",
                "org.harvestcircle.build.rust-ffi",
                "org.harvestcircle.build.packaging",
            )

        pluginIds.forEach { pluginId ->
            val fixture = temporaryDirectory.resolve(pluginId.substringAfterLast('.')).createDirectories()
            fixture.resolve("settings.gradle.kts").writeText(
                "pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }\nrootProject.name = \"fixture\"\n",
            )
            if (pluginId == "org.harvestcircle.build.kmp-shared") {
                fixture.resolve("gradle").createDirectories().resolve("libs.versions.toml").writeText(kmpCatalog)
            }
            val pluginBlock = if (pluginId == "org.harvestcircle.build.kmp-shared") kmpPlugins else "id(\"$pluginId\")"
            fixture.resolve("build.gradle.kts").writeText("plugins { $pluginBlock }\n")

            val runner =
                GradleRunner.create()
                    .withProjectDir(fixture.toFile())
                    .withPluginClasspath()
                    .withArguments("tasks", "--configuration-cache", "--configuration-cache-problems=fail", "--stacktrace")
            val result = runner.build()

            assertTrue(result.output.contains("BUILD SUCCESSFUL"), pluginId)
            if (pluginId == "org.harvestcircle.build.root") {
                assertTrue(result.output.contains("verifyProductCoordinates"), result.output)
                assertTrue(runner.build().output.contains("Reusing configuration cache"))
            }
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
            "pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }\nrootProject.name = \"fixture\"\n",
        )
        fixture.resolve("gradle").createDirectories().resolve("libs.versions.toml").writeText(kmpCatalog)
        fixture.resolve("build.gradle.kts").writeText("plugins { id(\"org.harvestcircle.build.kmp-shared\") }\n")
        fixture.resolve("src/commonMain/kotlin").createDirectories().resolve("Leak.kt").writeText(
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

    private val kmpCatalog =
        """
        [versions]
        kotlin = "2.4.10"
        compose = "1.11.1"
        coroutines = "1.9.0"

        [libraries]
        compose-foundation = { module = "org.jetbrains.compose.foundation:foundation", version.ref = "compose" }
        compose-runtime = { module = "org.jetbrains.compose.runtime:runtime", version.ref = "compose" }
        compose-ui = { module = "org.jetbrains.compose.ui:ui", version.ref = "compose" }
        compose-ui-test-junit4 = { module = "org.jetbrains.compose.ui:ui-test-junit4", version.ref = "compose" }
        kotlinx-coroutines-core = { module = "org.jetbrains.kotlinx:kotlinx-coroutines-core", version.ref = "coroutines" }
        kotlinx-coroutines-test = { module = "org.jetbrains.kotlinx:kotlinx-coroutines-test", version.ref = "coroutines" }
        """.trimIndent() + "\n"

    private val kmpPlugins =
        """
        id("org.harvestcircle.build.kmp-shared")
        """.trimIndent()
}
