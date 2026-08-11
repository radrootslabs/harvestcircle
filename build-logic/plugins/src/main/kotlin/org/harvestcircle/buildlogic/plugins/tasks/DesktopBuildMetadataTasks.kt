package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.CacheableTask
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.OutputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.contracts.DesktopBuildMetadataValues
import org.harvestcircle.buildlogic.contracts.GeneratedKotlin

public abstract class DesktopBuildMetadataTask : DefaultTask() {
    @get:Input
    public abstract val productVersion: Property<String>

    @get:Input
    public abstract val distributionPackageVersion: Property<String>

    @get:Input
    public abstract val gradleToolchain: Property<String>

    @get:Input
    public abstract val javaToolchain: Property<String>

    @get:Input
    public abstract val kotlinToolchain: Property<String>

    @get:Input
    public abstract val composeMultiplatformVersion: Property<String>

    protected fun values(): DesktopBuildMetadataValues =
        DesktopBuildMetadataValues(
            productVersion = productVersion.get(),
            distributionPackageVersion = distributionPackageVersion.get(),
            gradleToolchain = gradleToolchain.get(),
            javaToolchain = javaToolchain.get(),
            kotlinToolchain = kotlinToolchain.get(),
            composeMultiplatformVersion = composeMultiplatformVersion.get(),
        )
}

@CacheableTask
public abstract class GenerateDesktopBuildMetadata : DesktopBuildMetadataTask() {
    @get:OutputFile
    public abstract val outputFile: RegularFileProperty

    @TaskAction
    public fun generate() {
        val output = outputFile.get().asFile
        output.parentFile.mkdirs()
        output.writeText(GeneratedKotlin.desktopBuildMetadata(values()))
    }
}

@DisableCachingByDefault(because = "Generated metadata freshness verification produces no reusable output")
public abstract class VerifyGeneratedDesktopBuildMetadata : DesktopBuildMetadataTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val generatedFile: RegularFileProperty

    @TaskAction
    public fun verify() {
        GeneratedKotlin.requireFresh(
            generatedFile.get().asFile.takeIf { it.isFile }?.readText(),
            GeneratedKotlin.desktopBuildMetadata(values()),
            "Generated desktop build metadata",
        )
    }
}
