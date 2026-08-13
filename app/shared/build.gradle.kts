plugins {
    id("org.harvestcircle.build.kmp-shared")
}

tasks.named<Test>("desktopTest") {
    systemProperty("harvestcircle.projectDir", rootProject.layout.projectDirectory.asFile.absolutePath)
    if (providers.gradleProperty("harvestcircle.updateMacosGoldens").orNull == "true") {
        systemProperty("harvestcircle.updateMacosGoldens", "true")
    }
}
