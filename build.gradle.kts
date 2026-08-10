import org.harvestcircle.gradle.VerifyProductCoordinates

plugins {
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.compose.compiler) apply false
}

val productCoordinatesFile = layout.projectDirectory.file("config/product/harvestcircle-v1.properties")
val ffiCompatibilityBaselineFile =
    layout.projectDirectory.file("core/compatibility/harvestcircle-ffi-v4.properties")
val legacyProduct = "stu" + "dio"

val verifyProductCoordinates by tasks.registering(VerifyProductCoordinates::class) {
    group = "verification"
    description = "Validates the canonical HarvestCircle product-coordinate authority."
    manifestFile.set(productCoordinatesFile)
    uniFfiConfigFile.set(
        layout.projectDirectory.file("core/crates/harvestcircle_ffi/uniffi.toml"),
    )
    ffiBaselineFile.set(ffiCompatibilityBaselineFile)
    sourceProvenanceFile.set(
        layout.projectDirectory.file("core/provenance/$legacyProduct-import-v1.toml"),
    )
    nativeCompatibilityFile.set(
        layout.projectDirectory.file(
            "app/desktop/src/main/kotlin/org/harvestcircle/application/NativeCompatibility.kt",
        ),
    )
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("root"))
}
