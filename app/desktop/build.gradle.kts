import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.api.tasks.testing.Test
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

version = "0.1.0-alpha"

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.compose.compiler)
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(libs.compose.foundation)

    testImplementation(kotlin("test-junit"))
    testImplementation(libs.compose.ui.test.junit4)
}

val testInventoryRoot = providers
    .gradleProperty("testInventoryRoot")
    .orElse("src/test/kotlin")
val expectedTests = fileTree(testInventoryRoot.get()) {
    include("**/*Test.kt")
}
abstract class VerifyTestInventory : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val testFiles: ConfigurableFileCollection

    @get:Input
    abstract val sourceRoot: Property<String>

    @TaskAction
    fun verify() {
        if (testFiles.isEmpty) {
            throw GradleException("No Kotlin tests found under ${sourceRoot.get()}")
        }
    }
}
val verifyTestInventory by tasks.registering(VerifyTestInventory::class) {
    testFiles.from(expectedTests)
    sourceRoot.set(testInventoryRoot)
}

tasks.withType<Test>().configureEach {
    dependsOn(verifyTestInventory)
    failOnNoDiscoveredTests.set(true)
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }

    jvmToolchain(21)
}

compose.desktop {
    application {
        mainClass = "org.radroots.studio.desktop.MainKt"

        jvmArgs += listOf(
            "-Dapple.awt.application.name=Radroots",
            "-Dapple.awt.application.appearance=system",
        )

        nativeDistributions {
            targetFormats(TargetFormat.Dmg)

            packageName = "Radroots"
            packageVersion = "1.0.0"
            description = "Radroots Studio 0.1.0-alpha"
            copyright = "Copyright © 2024 Radroots, Inc."
            vendor = "Radroots, Inc"

            macOS {
                bundleID = "org.radroots.studio"
                iconFile.set(project.file("src/main/resources/icons/radroots.icns"))
                packageName = "Radroots"
                dockName = "Radroots"
                packageBuildVersion = "1"
            }
        }
    }
}
