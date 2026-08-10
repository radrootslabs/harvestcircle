import org.harvestcircle.gradle.VerifyProductCoordinates

plugins {
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.compose.compiler) apply false
}

val productCoordinatesFile = layout.projectDirectory.file("config/product/harvestcircle-v1.properties")

val verifyProductCoordinates by tasks.registering(VerifyProductCoordinates::class) {
    group = "verification"
    description = "Validates the canonical HarvestCircle product-coordinate authority."
    manifestFile.set(productCoordinatesFile)
    uniFfiConfigFile.set(
        layout.projectDirectory.file("core/crates/harvestcircle_ffi/uniffi.toml"),
    )
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("root"))
}
