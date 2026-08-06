plugins {
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.compose.compiler) apply false
}

val extBuildGradleRoot =
    providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull
        ?: error("EXT_BUILD_GRADLE_BUILD_DIR is required; run Gradle through cargo extbuild")
layout.buildDirectory.set(file(extBuildGradleRoot).resolve("root"))
