import org.jetbrains.compose.desktop.application.dsl.TargetFormat
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

tasks.withType<Test>().configureEach {
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
