package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.harvestcircle.buildlogic.contracts.RadrootsLibSourceLock
import org.harvestcircle.buildlogic.contracts.SourceProvenance

@DisableCachingByDefault(because = "Product coordinate verification produces no reusable output")
abstract class VerifyProductCoordinates : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val manifestFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val uniFfiConfigFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val ffiBaselineFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sourceProvenanceFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val radrootsLibSourceLockFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val cargoLockFile: RegularFileProperty

    @get:Internal
    abstract val repositoryRoot: DirectoryProperty

    @TaskAction
    fun verify() {
        val source = manifestFile.get().asFile.readText()
        val coordinates = ProductCoordinates.parse(source)
        check(coordinates.digest.matches(Regex("[0-9a-f]{64}")))

        val uniFfiConfig = uniFfiConfigFile.get().asFile.readText()
        check(
            uniFfiConfig.contains(
                "package_name = \"${coordinates["ffi.kotlin_package"]}\"",
            ),
        )
        check(
            uniFfiConfig.contains(
                "cdylib_name = \"${coordinates["ffi.cdylib_name"]}\"",
            ),
        )

        val baseline = FfiCompatibilityBaseline.load(ffiBaselineFile.get().asFile)
        check(baseline["product.coordinate_digest"] == coordinates.digest)
        val provenance = SourceProvenance.load(sourceProvenanceFile.get().asFile)
        check(baseline["source.provenance_digest"] == provenance.digest)
        check(provenance.foundationBaseline == baseline["source.foundation_baseline"])
        RadrootsLibSourceLock.load(
            sourceLockFile = radrootsLibSourceLockFile.get().asFile,
            repositoryRoot = repositoryRoot.get().asFile,
        )
    }
}
