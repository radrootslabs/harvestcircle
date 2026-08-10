import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("app-shared"))
}

kotlin {
    jvm("desktop") {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_21)
        }
    }

    sourceSets {
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
    }

    jvmToolchain(21)

    check(
        targets
            .filter { it.platformType != KotlinPlatformType.common }
            .map { it.name } == listOf("desktop"),
    ) {
        "HarvestCircle shared must declare exactly one KMP platform target named desktop"
    }
}
