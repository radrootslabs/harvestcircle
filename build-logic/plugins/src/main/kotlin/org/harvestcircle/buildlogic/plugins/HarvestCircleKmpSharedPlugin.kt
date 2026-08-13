package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.harvestcircle.buildlogic.plugins.tasks.VerifySharedBoundary

public class HarvestCircleKmpSharedPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        target.applyDesktopDesignKmp()

        val catalog = target.versionCatalog()
        val kotlinVersion = catalog.findVersion("kotlin").get().requiredVersion
        val composeVersion = catalog.findVersion("compose").get().requiredVersion

        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-foundation").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-runtime").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-ui").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("kotlinx-coroutines-core").get())
        target.dependencies.add("commonTestImplementation", "org.jetbrains.kotlin:kotlin-test:$kotlinVersion")
        target.dependencies.add("commonTestImplementation", catalog.findLibrary("kotlinx-coroutines-test").get())
        target.dependencies.add("desktopTestImplementation", composeDesktopDependency(target, composeVersion))
        target.dependencies.add("desktopTestImplementation", "org.jetbrains.kotlin:kotlin-test-junit:$kotlinVersion")
        target.dependencies.add("desktopTestImplementation", catalog.findLibrary("compose-ui-test-junit4").get())

        target.providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
            target.layout.buildDirectory.set(target.file(outputRoot).resolve("app-shared"))
        }

        val verifySharedBoundary =
            target.tasks.register("verifySharedBoundary", VerifySharedBoundary::class.java) { task ->
                task.commonSources.from(
                    target.fileTree("src/commonMain/kotlin") { tree -> tree.include("**/*.kt") },
                )
            }
        target.tasks.named("check") { task -> task.dependsOn(verifySharedBoundary) }
    }

}
