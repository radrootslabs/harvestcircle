package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.jetbrains.kotlin.gradle.dsl.KotlinMultiplatformExtension

public class HarvestCircleDesignSystemPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target.path == ":app:design_system") {
            "The HarvestCircle design-system plugin may only be applied to :app:design_system"
        }
        target.applyDesktopDesignKmp()
        target.extensions.configure(KotlinMultiplatformExtension::class.java) { kotlin ->
            kotlin.explicitApi()
        }
        val catalog = target.versionCatalog()
        val kotlinVersion = catalog.findVersion("kotlin").get().requiredVersion
        target.dependencies.add(
            "commonMainApi",
            catalog.findLibrary("compose-foundation").get(),
        )
        target.dependencies.add(
            "commonMainApi",
            catalog.findLibrary("compose-runtime").get(),
        )
        target.dependencies.add("commonMainApi", catalog.findLibrary("compose-ui").get())
        target.dependencies.add(
            "commonTestImplementation",
            "org.jetbrains.kotlin:kotlin-test:$kotlinVersion",
        )
        target.routeDesignBuildDirectory("app-design-system")
    }
}
