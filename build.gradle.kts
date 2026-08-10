import org.harvestcircle.gradle.VerifyFoundationBoundaries
import org.harvestcircle.gradle.VerifyProductCoordinates
import org.harvestcircle.gradle.VerifyVerificationLanes

plugins {
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.compose.compiler) apply false
}

val productCoordinatesFile = layout.projectDirectory.file("config/product/harvestcircle-v1.properties")
val ffiCompatibilityBaselineFile =
    layout.projectDirectory.file("core/compatibility/harvestcircle-ffi-v4.properties")
val verificationLanesFile = layout.projectDirectory.file("config/verification/lanes-v1.properties")
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

val verifyVerificationLanes by tasks.registering(VerifyVerificationLanes::class) {
    group = "verification"
    description = "Validates forge-agnostic verification lanes and least-privilege policy."
    policyFile.set(verificationLanesFile)
}

val verifyFoundationBoundaries by tasks.registering(VerifyFoundationBoundaries::class) {
    group = "verification"
    description = "Audits tracked sources against the HarvestCircle foundation boundaries."
    repositoryRoot.set(layout.projectDirectory)
    gitAware.set(true)
}

val verifyFoundationArchive by tasks.registering(VerifyFoundationBoundaries::class) {
    group = "verification"
    description = "Audits a source-archive inventory without Git metadata."
    repositoryRoot.set(layout.projectDirectory)
    gitAware.set(false)
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("root"))
}
