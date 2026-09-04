package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.HarvestCircleArtifactContract
import org.harvestcircle.buildlogic.contracts.ProductCoordinates

@DisableCachingByDefault(because = "Artifact contract verification produces no reusable output")
public abstract class VerifyHarvestCircleArtifactContract : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val contractFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val productCoordinatesFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val ffiCompatibilityBaselineFile: RegularFileProperty

    @get:Input
    public abstract val expectedBuildVersion: Property<String>

    @get:Internal
    public abstract val repositoryRoot: DirectoryProperty

    @TaskAction
    public fun verify() {
        HarvestCircleArtifactContract.load(
            contractFile = contractFile.get().asFile,
            repositoryRoot = repositoryRoot.get().asFile,
        )
        val coordinates = ProductCoordinates.load(productCoordinatesFile.get().asFile)
        val baseline = FfiCompatibilityBaseline.load(ffiCompatibilityBaselineFile.get().asFile)
        val productName = coordinates["product.name"]
        val packageVersion = baseline["package.version"]
        HarvestCircleArtifactContract.validatePackageCoordinates(
            productName = productName,
            identity = coordinates["desktop.bundle_id"],
            productVersion = baseline["product.version"],
            packageVersion = packageVersion,
            buildVersion = expectedBuildVersion.get(),
            fileName = "$productName-$packageVersion.dmg",
        )
    }
}
