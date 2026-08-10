package org.harvestcircle.buildlogic.plugins

import org.gradle.testkit.runner.GradleRunner
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

            val result =
                GradleRunner.create()
                    .withProjectDir(fixture.toFile())
                    .withPluginClasspath()
                    .withArguments("tasks", "--stacktrace")
                    .build()

            assertTrue(result.output.contains("BUILD SUCCESSFUL"), pluginId)
        }
    }
}
