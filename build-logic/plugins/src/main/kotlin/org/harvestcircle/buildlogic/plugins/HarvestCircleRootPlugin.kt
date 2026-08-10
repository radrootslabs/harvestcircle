package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.harvestcircle.buildlogic.plugins.tasks.VerifyFoundationBoundaries
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGitSourcePolicy
import org.harvestcircle.buildlogic.plugins.tasks.VerifyProductCoordinateConsumers
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
                task.nativeCompatibilityFile.set(
                    target.layout.projectDirectory.file(
                        "app/desktop/src/main/kotlin/org/harvestcircle/application/NativeCompatibility.kt",
                    ),
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
        target.tasks.register("verifyProductCoordinateConsumers", VerifyProductCoordinateConsumers::class.java) { task ->
            task.group = "verification"
            task.description = "Validates that build and runtime identities consume the product manifest."
            task.manifestFile.set(productCoordinatesFile)
            task.desktopBuildFile.set(target.layout.projectDirectory.file("app/desktop/build.gradle.kts"))
            task.desktopPluginFile.set(
                target.layout.projectDirectory.file(
                    "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCircleDesktopAppPlugin.kt",
                ),
            )
            task.rustPluginFile.set(
                target.layout.projectDirectory.file(
                    "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCircleRustFfiPlugin.kt",
                ),
            )
            task.packagingPluginFile.set(
                target.layout.projectDirectory.file(
                    "build-logic/plugins/src/main/kotlin/org/harvestcircle/buildlogic/plugins/HarvestCirclePackagingPlugin.kt",
                ),
            )
            task.uniFfiConfigFile.set(target.layout.projectDirectory.file("core/crates/harvestcircle_ffi/uniffi.toml"))
            task.productBuildFile.set(target.layout.projectDirectory.file("core/crates/harvestcircle_product/build.rs"))
            task.ffiConsumerFile.set(target.layout.projectDirectory.file("core/crates/harvestcircle_ffi/src/commands.rs"))
            task.keyringConsumerFile.set(target.layout.projectDirectory.file("core/crates/harvestcircle_storage/src/os_keyring.rs"))
        }
        target.tasks.register("verifyVerificationLanes", VerifyVerificationLanes::class.java) { task ->
            task.group = "verification"
            task.description = "Validates forge-agnostic verification lanes and least-privilege policy."
            task.policyFile.set(verificationLanesFile)
            task.productManifestFile.set(productCoordinatesFile)
            task.repositoryRoot.set(target.layout.projectDirectory)
        }
        val verifyGitSourcePolicy =
            target.tasks.register("verifyGitSourcePolicy", VerifyGitSourcePolicy::class.java) { task ->
                task.group = "verification"
                task.description = "Validates immutable and allowlisted Cargo Git dependency sources."
                task.denyConfigFile.set(target.layout.projectDirectory.file("core/deny.toml"))
                task.cargoLockFile.set(target.layout.projectDirectory.file("core/Cargo.lock"))
                task.cargoManifestFiles.from(
                    target.fileTree("core") { tree ->
                        tree.include("Cargo.toml", "crates/*/Cargo.toml")
                    },
                )
            }
        target.tasks.register("verifyFoundationBoundaries", VerifyFoundationBoundaries::class.java) { task ->
            task.group = "verification"
            task.description = "Audits tracked sources against the HarvestCircle foundation boundaries."
            task.repositoryRoot.set(target.layout.projectDirectory)
            task.gitAware.set(true)
            task.dependsOn(verifyGitSourcePolicy)
        }
        target.tasks.register("verifyFoundationArchive", VerifyFoundationBoundaries::class.java) { task ->
            task.group = "verification"
            task.description = "Audits a source-archive inventory without Git metadata."
            task.repositoryRoot.set(target.layout.projectDirectory)
            task.gitAware.set(false)
            task.dependsOn(verifyGitSourcePolicy)
        }

        target.providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
            target.layout.buildDirectory.set(target.file(outputRoot).resolve("root"))
        }
    }
}
