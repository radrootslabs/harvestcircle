package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.harvestcircle.buildlogic.plugins.tasks.VerifyProductCoordinates
import org.harvestcircle.buildlogic.plugins.tasks.VerifyVerificationLanes

public class HarvestCircleRootPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target == target.rootProject) { "The HarvestCircle root plugin may only be applied to the root project" }

        val productCoordinatesFile = target.layout.projectDirectory.file("config/product/harvestcircle-v1.properties")
        val ffiCompatibilityBaselineFile =
            target.layout.projectDirectory.file("core/compatibility/harvestcircle-ffi-v4.properties")
        val verificationLanesFile = target.layout.projectDirectory.file("config/verification/lanes-v2.properties")
        val legacyProduct = "stu" + "dio"

        val verifyProductCoordinates =
            target.tasks.register("verifyProductCoordinates", VerifyProductCoordinates::class.java) { task ->
                task.group = "verification"
                task.description = "Validates the canonical HarvestCircle product-coordinate authority."
                task.manifestFile.set(productCoordinatesFile)
                task.uniFfiConfigFile.set(
                    target.layout.projectDirectory.file("core/crates/harvestcircle_ffi/uniffi.toml"),
                )
                task.ffiBaselineFile.set(ffiCompatibilityBaselineFile)
                task.sourceProvenanceFile.set(
                    target.layout.projectDirectory.file("core/provenance/$legacyProduct-import-v1.toml"),
                )
            }

        target.tasks.register("verifyCompatibilityBaseline") { task ->
            task.group = "verification"
            task.description = "Validates the generated-code and native compatibility baseline."
            task.dependsOn(verifyProductCoordinates)
        }
        target.tasks.register("verifySourceProvenance") { task ->
            task.group = "verification"
            task.description = "Validates canonical source provenance and its governed digest."
            task.dependsOn(verifyProductCoordinates)
        }
        target.tasks.register("verifyVerificationLanes", VerifyVerificationLanes::class.java) { task ->
            task.group = "verification"
            task.description = "Validates forge-agnostic verification lanes and least-privilege policy."
            task.policyFile.set(verificationLanesFile)
            task.productManifestFile.set(productCoordinatesFile)
            task.makefileFile.set(target.layout.projectDirectory.file("Makefile"))
        }
        target.providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
            target.layout.buildDirectory.set(target.file(outputRoot).resolve("root"))
        }
    }
}
