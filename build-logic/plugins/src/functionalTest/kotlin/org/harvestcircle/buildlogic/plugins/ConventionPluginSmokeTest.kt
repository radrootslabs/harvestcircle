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
            fixture.resolve("settings.gradle.kts").writeText("rootProject.name = \"fixture\"\n")
            fixture.resolve("build.gradle.kts").writeText("plugins { id(\"$pluginId\") }\n")

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
}
