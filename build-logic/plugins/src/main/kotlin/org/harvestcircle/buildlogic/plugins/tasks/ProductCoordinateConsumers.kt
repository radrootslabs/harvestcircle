package org.harvestcircle.buildlogic.plugins.tasks

import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.gradle.api.DefaultTask
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

abstract class VerifyProductCoordinateConsumers : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val manifestFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val desktopBuildFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val desktopPluginFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val uniFfiConfigFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val productBuildFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val ffiConsumerFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val keyringConsumerFile: RegularFileProperty

    @TaskAction
    fun verify() {
        val coordinates = ProductCoordinates.load(manifestFile.get().asFile)
        val desktopBuild =
            desktopBuildFile.get().asFile.readText() +
                "\n" +
                desktopPluginFile.get().asFile.readText()
        listOf(
            "product.name",
            "product.slug",
            "desktop.application_id",
            "desktop.bundle_id",
            "desktop.main_class",
            "ffi.kotlin_package",
            "ffi.cdylib_name",
            "environment.prefix",
            "vendor.name",
            "copyright.notice",
        ).forEach { key ->
            check(desktopBuild.contains("productCoordinates[\"$key\"]")) {
                "Desktop build logic does not consume product coordinate $key"
            }
        }
        listOf(
            "desktop.application_id",
            "desktop.bundle_id",
            "desktop.main_class",
            "database.filename",
            "keyring.service",
            "environment.prefix",
        ).forEach { key ->
            check(!desktopBuild.contains("\"${coordinates[key]}\"")) {
                "Desktop build duplicates the approved value for $key"
            }
        }

        val uniFfi = uniFfiConfigFile.get().asFile.readText()
        check(uniFfi.contains("package_name = \"${coordinates["ffi.kotlin_package"]}\""))
        check(uniFfi.contains("cdylib_name = \"${coordinates["ffi.cdylib_name"]}\""))

        val productBuild = productBuildFile.get().asFile.readText()
        check(productBuild.contains("generate_rust_constants(&source)"))
        val ffiConsumer = ffiConsumerFile.get().asFile.readText()
        listOf(
            "DATABASE_APPLICATION",
            "DATABASE_FILENAME",
            "DATABASE_ORGANIZATION",
            "DATABASE_QUALIFIER",
            "DEVELOPMENT_DATA_DIR_ENVIRONMENT",
        ).forEach { constant -> check(ffiConsumer.contains(constant)) }
        check(keyringConsumerFile.get().asFile.readText().contains("harvestcircle_product::KEYRING_SERVICE"))
    }
}
