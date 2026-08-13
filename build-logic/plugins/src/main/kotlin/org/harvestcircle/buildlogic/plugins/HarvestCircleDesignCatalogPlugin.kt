package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project

public class HarvestCircleDesignCatalogPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target.path == ":tools:design_catalog") {
            "The HarvestCircle design-catalog plugin may only be applied to :tools:design_catalog"
        }
        target.applyDesktopDesignKmp()
        val catalog = target.versionCatalog()
        val kotlinVersion = catalog.findVersion("kotlin").get().requiredVersion
        target.dependencies.add("commonMainImplementation", target.project(":app:design_system"))
        target.dependencies.add(
            "commonMainImplementation",
            catalog.findLibrary("compose-foundation").get(),
        )
        target.dependencies.add(
            "commonMainImplementation",
            catalog.findLibrary("compose-runtime").get(),
        )
        target.dependencies.add(
            "commonMainImplementation",
            catalog.findLibrary("compose-ui").get(),
        )
        target.dependencies.add(
            "commonTestImplementation",
            "org.jetbrains.kotlin:kotlin-test:$kotlinVersion",
        )
        target.dependencies.add(
            "desktopMainImplementation",
            composeDesktopDependency(target, catalog.findVersion("compose").get().requiredVersion),
        )
        target.routeDesignBuildDirectory("tools-design-catalog")
    }
}
