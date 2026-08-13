plugins {
    id("org.harvestcircle.build.root")
}

tasks.register("designFormatCheck") {
    group = "verification"
    description = "Checks formatting for every HarvestCircle-owned design module."
    dependsOn(
        ":app:design_system:ktlintCheck",
        ":tools:design_catalog:ktlintCheck",
    )
}

tasks.register("designLint") {
    group = "verification"
    description = "Runs static analysis for every HarvestCircle-owned design module."
    dependsOn(
        ":app:design_system:detektCommonMainSourceSet",
        ":app:design_system:detektCommonTestSourceSet",
        ":tools:design_catalog:detektCommonMainSourceSet",
        ":tools:design_catalog:detektCommonTestSourceSet",
    )
}

tasks.register("designTest") {
    group = "verification"
    description = "Runs tests for every HarvestCircle-owned design module."
    dependsOn(
        ":app:design_system:desktopTest",
        ":tools:design_catalog:desktopTest",
    )
}

tasks.register("designCheck") {
    group = "verification"
    description = "Runs the complete HarvestCircle-owned design module lifecycle."
    dependsOn("designFormatCheck", "designLint", "designTest")
}
