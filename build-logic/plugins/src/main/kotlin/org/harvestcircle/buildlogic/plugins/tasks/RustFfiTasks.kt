package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.FileSystemOperations
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.MapProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.CacheableTask
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.OutputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.GeneratedKotlin
import javax.inject.Inject

@DisableCachingByDefault(because = "Cargo owns its incremental and artifact caches")
public abstract class CargoBuildTask
    @Inject
    constructor(
        private val execOperations: ExecOperations,
    ) : DefaultTask() {
        @get:Internal
        public abstract val workingDirectory: DirectoryProperty

        @get:InputFile
        @get:PathSensitive(PathSensitivity.RELATIVE)
        public abstract val manifestFile: RegularFileProperty

        @get:InputFiles
        @get:PathSensitive(PathSensitivity.RELATIVE)
        public abstract val rustSources: ConfigurableFileCollection

        @get:Input
        public abstract val packageName: Property<String>

        @get:Input
        public abstract val release: Property<Boolean>

        @get:Input
        public abstract val immutableArguments: ListProperty<String>

        @get:Input
        public abstract val buildEnvironment: MapProperty<String, String>

        @get:OutputFile
        public abstract val libraryFile: RegularFileProperty

        @TaskAction
        public fun build() {
            execOperations.exec { spec ->
                spec.workingDir(workingDirectory.get().asFile)
                spec.environment(buildEnvironment.get())
                spec.commandLine(
                    buildList {
                        add("cargo")
                        add("build")
                        if (release.get()) add("--release")
                        addAll(listOf("--manifest-path", manifestFile.get().asFile.absolutePath, "-p", packageName.get()))
                        addAll(immutableArguments.get())
                    },
                )
            }.assertNormalExitValue()
        }
    }

@DisableCachingByDefault(because = "UniFFI bindgen is a Cargo process over a native library")
public abstract class GenerateUniFfiKotlinTask
    @Inject
    constructor(
        private val execOperations: ExecOperations,
        private val fileSystemOperations: FileSystemOperations,
    ) : DefaultTask() {
        @get:Internal
        public abstract val workingDirectory: DirectoryProperty

        @get:InputFile
        @get:PathSensitive(PathSensitivity.RELATIVE)
        public abstract val manifestFile: RegularFileProperty

        @get:InputFile
        @get:PathSensitive(PathSensitivity.RELATIVE)
        public abstract val configFile: RegularFileProperty

        @get:InputFile
        @get:PathSensitive(PathSensitivity.NONE)
        public abstract val nativeLibrary: RegularFileProperty

        @get:Input
        public abstract val immutableArguments: ListProperty<String>

        @get:OutputDirectory
        public abstract val outputDirectory: DirectoryProperty

        @TaskAction
        public fun generate() {
            fileSystemOperations.delete { it.delete(outputDirectory) }
            execOperations.exec { spec ->
                spec.workingDir(workingDirectory.get().asFile)
                spec.commandLine(
                    buildList {
                        addAll(
                            listOf(
                                "cargo",
                                "run",
                                "--manifest-path",
                                manifestFile.get().asFile.absolutePath,
                                "-p",
                                "harvestcircle_uniffi_bindgen",
                            ),
                        )
                        addAll(immutableArguments.get())
                        addAll(
                            listOf(
                                "--",
                                "generate",
                                "--library",
                                "--language",
                                "kotlin",
                                "--metadata-no-deps",
                                "--no-format",
                                "--config",
                                configFile.get().asFile.absolutePath,
                                "--out-dir",
                                outputDirectory.get().asFile.absolutePath,
                                nativeLibrary.get().asFile.absolutePath,
                            ),
                        )
                    },
                )
            }.assertNormalExitValue()
        }
    }

@CacheableTask
public abstract class GenerateCompatibilityExpectations : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val baselineFile: RegularFileProperty

    @get:OutputFile
    public abstract val outputFile: RegularFileProperty

    @TaskAction
    public fun generate() {
        val output = outputFile.get().asFile
        output.parentFile.mkdirs()
        output.writeText(GeneratedKotlin.compatibilityExpectations(FfiCompatibilityBaseline.load(baselineFile.get().asFile)))
    }
}

@CacheableTask
public abstract class VerifyGeneratedCompatibilityExpectations : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val baselineFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val generatedFile: RegularFileProperty

    @TaskAction
    public fun verify() {
        val expected = GeneratedKotlin.compatibilityExpectations(FfiCompatibilityBaseline.load(baselineFile.get().asFile))
        GeneratedKotlin.requireFresh(
            generatedFile.get().asFile.takeIf { it.isFile }?.readText(),
            expected,
            "Generated Kotlin compatibility expectations",
        )
    }
}

@CacheableTask
public abstract class VerifyUniFfiBindings : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val generatedDirectory: DirectoryProperty

    @get:Input
    public abstract val expectedPackage: Property<String>

    @TaskAction
    public fun verify() {
        val kotlinFiles = generatedDirectory.asFileTree.files.filter { it.isFile && it.extension == "kt" }
        if (kotlinFiles.size != 1) throw GradleException("Expected exactly one generated UniFFI Kotlin source")
        if (!kotlinFiles.single().readText().contains("package ${expectedPackage.get()}")) {
            throw GradleException("Generated UniFFI Kotlin package does not match the runtime contract")
        }
    }
}

@CacheableTask
public abstract class StageReleaseNativeLibrary
    @Inject
    constructor(
        private val fileSystemOperations: FileSystemOperations,
    ) : DefaultTask() {
        @get:InputFile
        @get:PathSensitive(PathSensitivity.NONE)
        public abstract val releaseLibrary: RegularFileProperty

        @get:Input
        public abstract val platformPrefix: Property<String>

        @get:OutputDirectory
        public abstract val outputDirectory: DirectoryProperty

        @TaskAction
        public fun stage() {
            fileSystemOperations.delete { it.delete(outputDirectory) }
            fileSystemOperations.copy { spec ->
                spec.from(releaseLibrary)
                spec.into(outputDirectory.dir(platformPrefix))
            }
        }
    }

@CacheableTask
public abstract class VerifyReleaseNativeLibrary : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val releaseLibrary: RegularFileProperty

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val stagedDirectory: DirectoryProperty

    @get:Input
    public abstract val expectedName: Property<String>

    @get:Input
    public abstract val expectedBuildEvidence: ListProperty<String>

    @TaskAction
    public fun verify() {
        val files = stagedDirectory.asFileTree.files.filter { it.isFile }
        if (files.size != 1) throw GradleException("Release resources must contain exactly one native library")
        if (files.single().name != expectedName.get()) throw GradleException("Unexpected release native library name")
        if (!files.single().readBytes().contentEquals(releaseLibrary.get().asFile.readBytes())) {
            throw GradleException("Staged native library does not match the Cargo release artifact")
        }
        val binary = releaseLibrary.get().asFile.readBytes().toString(Charsets.ISO_8859_1)
        expectedBuildEvidence.get().forEach { evidence ->
            require(binary.contains(evidence)) { "Release native library is missing build provenance evidence" }
        }
    }
}
