plugins {
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.compose.compiler) apply false
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("root"))
}
