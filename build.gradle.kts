import org.harvestcircle.gradle.VerifyFoundationBoundaries
import org.harvestcircle.gradle.VerifyProductCoordinateConsumers
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

val verifyCompatibilityBaseline by tasks.registering {
    group = "verification"
    description = "Validates the generated-code and native compatibility baseline."
    dependsOn(verifyProductCoordinates)
}

val verifyProductCoordinateConsumers by tasks.registering(VerifyProductCoordinateConsumers::class) {
    group = "verification"
    description = "Validates that build and runtime identities consume the product manifest."
    manifestFile.set(productCoordinatesFile)
    desktopBuildFile.set(layout.projectDirectory.file("app/desktop/build.gradle.kts"))
    uniFfiConfigFile.set(layout.projectDirectory.file("core/crates/harvestcircle_ffi/uniffi.toml"))
    productBuildFile.set(layout.projectDirectory.file("core/crates/harvestcircle_product/build.rs"))
    ffiConsumerFile.set(layout.projectDirectory.file("core/crates/harvestcircle_ffi/src/commands.rs"))
    keyringConsumerFile.set(layout.projectDirectory.file("core/crates/harvestcircle_storage/src/os_keyring.rs"))
}

val verifyVerificationLanes by tasks.registering(VerifyVerificationLanes::class) {
    group = "verification"
    description = "Validates forge-agnostic verification lanes and least-privilege policy."
    policyFile.set(verificationLanesFile)
    productManifestFile.set(productCoordinatesFile)
    repositoryRoot.set(layout.projectDirectory)
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
