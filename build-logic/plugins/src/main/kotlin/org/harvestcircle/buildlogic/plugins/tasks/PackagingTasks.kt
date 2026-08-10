package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault
import java.io.File
import java.util.concurrent.TimeUnit
import java.util.jar.JarFile

public abstract class VerifyDesktopBuildMetadataArtifact : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val desktopJar: RegularFileProperty

    @get:Input
    public abstract val expectedBuildEvidence: ListProperty<String>

    @TaskAction
    public fun verify() {
        JarFile(desktopJar.get().asFile).use { jar ->
            val entry =
                jar.getJarEntry("org/harvestcircle/application/generated/DesktopBuildMetadata.class")
                    ?: throw GradleException("Desktop build metadata is missing from the application artifact")
            val metadata = jar.getInputStream(entry).use { it.readBytes() }.toString(Charsets.ISO_8859_1)
            requireBuildMetadataEvidence(metadata, expectedBuildEvidence.get())
        }
    }
}

@DisableCachingByDefault(because = "Package inspection invokes host tools")
public abstract class VerifyMacOsDistribution : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val appDirectory: DirectoryProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val releaseLibrary: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val iconSource: RegularFileProperty

    @get:Input
    public abstract val expectedBundleId: Property<String>

    @get:Input
    public abstract val expectedPackageVersion: Property<String>

    @get:Input
    public abstract val expectedBuildVersion: Property<String>

    @get:Input
    public abstract val expectedNativeEntry: Property<String>

    @TaskAction
    public fun verify() {
        val app = appDirectory.get().asFile
        val plist = app.resolve("Contents/Info.plist")
        require(plist.isFile) { "Packaged macOS Info.plist is missing" }
        require(plistValue(plist, "CFBundleIdentifier") == expectedBundleId.get()) {
            "Packaged macOS bundle identifier is incorrect"
        }
        require(plistValue(plist, "CFBundleShortVersionString") == expectedPackageVersion.get()) {
            "Packaged macOS version is incorrect"
        }
        require(plistValue(plist, "CFBundleVersion") == expectedBuildVersion.get()) {
            "Packaged macOS build version is incorrect"
        }

        val sourceIcon = iconSource.get().asFile.readBytes()
        val matchingIcons =
            app.walkTopDown()
                .filter { it.isFile && it.extension == "icns" }
                .count { it.readBytes().contentEquals(sourceIcon) }
        require(matchingIcons == 1) { "Packaged macOS icon does not match the canonical icon" }
        verifyPackagedNativeLibraries(app, releaseLibrary.get().asFile, expectedNativeEntry.get(), true)
    }

    private fun plistValue(
        plist: File,
        key: String,
    ): String = commandOutput("/usr/libexec/PlistBuddy", "-c", "Print :$key", plist.absolutePath)

    private fun verifyPackagedNativeLibraries(
        app: File,
        release: File,
        expectedEntry: String,
        verifyMachO: Boolean,
    ) {
        val packagedLibraries = packagedNativeLibraries(app, expectedEntry)
        require(packagedLibraries.size == 1) {
            "Packaged application must contain exactly one release native library"
        }
        val packaged = temporaryDir.resolve(release.name).apply { writeBytes(packagedLibraries.single()) }
        if (verifyMachO) {
            require(machOIdentity(packaged) == machOIdentity(release)) {
                "Packaged native library identity does not match the Cargo release artifact"
            }
            commandOutput("/usr/bin/codesign", "--verify", "--strict", packaged.absolutePath)
        }
    }

    private fun machOIdentity(binary: File): String {
        val output = commandOutput("/usr/bin/dwarfdump", "--uuid", binary.absolutePath)
        return Regex("""UUID: ([0-9A-F-]+) \(([^)]+)\)""")
            .find(output)
            ?.value
            ?: throw GradleException("Could not read the packaged native library identity")
    }
}

public abstract class VerifyMacOsPackage : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val packageDirectory: DirectoryProperty

    @get:Input
    public abstract val expectedFileName: Property<String>

    @TaskAction
    public fun verify() {
        val packages = packageDirectory.asFileTree.files.filter { it.isFile && it.extension == "dmg" }
        require(packages.size == 1) { "Expected exactly one macOS disk image" }
        require(packages.single().name == expectedFileName.get()) { "Unexpected macOS disk image name" }
        require(packages.single().length() > 0L) { "Packaged macOS disk image is empty" }
    }
}

@DisableCachingByDefault(because = "Installation package extraction invokes host tools")
public abstract class VerifyNativeInstallPackage : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val packageDirectory: DirectoryProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val releaseLibrary: RegularFileProperty

    @get:Input
    public abstract val packageExtension: Property<String>

    @get:Input
    public abstract val expectedVersion: Property<String>

    @get:Input
    public abstract val expectedNativeEntry: Property<String>

    @get:Input
    public abstract val hostFamily: Property<String>

    @TaskAction
    public fun verify() {
        val extension = packageExtension.get()
        val packages = packageDirectory.asFileTree.files.filter { it.isFile && it.extension == extension }
        require(packages.size == 1) { "Expected exactly one .$extension installation package" }
        val installPackage = packages.single()
        require(installPackage.name.contains(expectedVersion.get())) {
            "Installation package name does not contain the governed version"
        }
        require(installPackage.length() > 0L) { "Installation package is empty" }

        val extracted = temporaryDir.resolve("extracted").apply { mkdirs() }
        when (hostFamily.get()) {
            "linux" -> commandOutput("dpkg-deb", "--extract", installPackage.absolutePath, extracted.absolutePath)
            "windows" ->
                commandOutput(
                    "msiexec.exe",
                    "/a",
                    installPackage.absolutePath,
                    "/qn",
                    "TARGETDIR=${extracted.absolutePath}",
                )
            else -> throw GradleException("Unsupported native package host")
        }

        val packagedLibraries = packagedNativeLibraries(extracted, expectedNativeEntry.get())
        require(packagedLibraries.size == 1) {
            "Installation package must contain exactly one canonical native library"
        }
        require(packagedLibraries.single().contentEquals(releaseLibrary.get().asFile.readBytes())) {
            "Installed native library does not match the canonical Cargo release artifact"
        }
    }
}

@DisableCachingByDefault(because = "The packaged executable is launched as a bounded host smoke test")
public abstract class VerifyPackagedApplicationHealth : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val executable: RegularFileProperty

    @get:Input
    public abstract val developmentDataEnvironment: Property<String>

    @get:Input
    public abstract val timeoutSeconds: Property<Long>

    @get:Input
    public abstract val readyEvidence: Property<String>

    @get:Input
    public abstract val closedEvidence: Property<String>

    @TaskAction
    public fun verify() {
        val dataRoot = temporaryDir.resolve("isolated-data").apply { mkdirs() }
        val process =
            ProcessBuilder(executable.get().asFile.absolutePath, "--health-check")
                .redirectErrorStream(true)
                .apply { environment()[developmentDataEnvironment.get()] = dataRoot.absolutePath }
                .start()
        val finished = process.waitFor(timeoutSeconds.get(), TimeUnit.SECONDS)
        if (!finished) {
            process.destroyForcibly()
            process.waitFor()
            throw GradleException("Packaged application health-check timed out")
        }
        val output = process.inputStream.bufferedReader().use { it.readText() }
        require(!containsSecretMaterial(output)) { "Packaged application health-check emitted secret material" }
        require(process.exitValue() == 0) { "Packaged application health-check failed" }
        require(output.contains(readyEvidence.get())) { "Packaged application did not report ready health evidence" }
        require(output.contains(closedEvidence.get())) { "Packaged application did not report closed health evidence" }
    }
}

@DisableCachingByDefault(because = "Signing verification invokes the host codesign tool")
public abstract class VerifyMacOsDeveloperIdSignature : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val appDirectory: DirectoryProperty

    @TaskAction
    public fun verify() {
        val app = appDirectory.get().asFile
        commandOutput("/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", app.absolutePath)
        val signature = commandOutput("/usr/bin/codesign", "--display", "--verbose=4", app.absolutePath)
        require(!signature.contains("Signature=adhoc")) {
            "Release application is ad-hoc signed; a Developer ID Application signature is required"
        }
        require(signature.lineSequence().any { it.startsWith("Authority=Developer ID Application:") }) {
            "Release application is not signed by a Developer ID Application identity"
        }
        require(signature.lineSequence().any { it.startsWith("TeamIdentifier=") && it != "TeamIdentifier=not set" }) {
            "Release application signature has no Apple team identifier"
        }
    }
}

@DisableCachingByDefault(because = "Notarization verification invokes host Apple tools")
public abstract class VerifyMacOsNotarization : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    public abstract val diskImage: RegularFileProperty

    @TaskAction
    public fun verify() {
        val image = diskImage.get().asFile
        commandOutput("/usr/bin/xcrun", "stapler", "validate", image.absolutePath)
        commandOutput(
            "/usr/sbin/spctl",
            "--assess",
            "--type",
            "open",
            "--context",
            "context:primary-signature",
            "--verbose=2",
            image.absolutePath,
        )
    }
}

public abstract class VerifyReleaseBuildProvenance : DefaultTask() {
    @get:Input
    public abstract val sourceCommit: Property<String>

    @get:Input
    public abstract val sourceDirty: Property<String>

    @get:Input
    public abstract val radrootsRevision: Property<String>

    @get:Input
    public abstract val sourceDateEpoch: Property<String>

    @TaskAction
    public fun verify() {
        require(Regex("[0-9a-f]{40}").matches(sourceCommit.get())) {
            "Release source commit provenance is unknown or malformed"
        }
        require(sourceDirty.get() == "false") { "Release provenance reports a dirty or unknown source tree" }
        require(Regex("[0-9a-f]{40}").matches(radrootsRevision.get())) {
            "Release Radroots revision provenance is unknown or malformed"
        }
        require(sourceDateEpoch.get().toULongOrNull()?.let { it > 0UL } == true) {
            "Release SOURCE_DATE_EPOCH provenance is unknown or malformed"
        }
    }
}

private fun packagedNativeLibraries(
    root: File,
    expectedEntry: String,
): List<ByteArray> {
    val entries =
        root.walkTopDown()
            .filter { it.isFile && it.extension == "jar" }
            .flatMap { jarFile ->
                JarFile(jarFile).use { jar ->
                    jar.entries().asSequence().filter { entry ->
                        !entry.isDirectory &&
                            entry.name.substringAfterLast('.').lowercase() in setOf("dylib", "so", "dll")
                    }.map { entry -> entry.name to jar.getInputStream(entry).use { it.readBytes() } }.toList()
                }.asSequence()
            }.toList()
    requireSingleCanonicalProductNativeEntry(entries.map { it.first }, expectedEntry)
    return entries.filter { it.first == expectedEntry }.map { it.second }
}

internal fun requireBuildMetadataEvidence(
    metadata: String,
    expectedBuildEvidence: List<String>,
) {
    expectedBuildEvidence.forEach { evidence ->
        require(metadata.contains(evidence)) {
            "Desktop application artifact is missing generated build evidence"
        }
    }
}

internal fun requireSingleCanonicalProductNativeEntry(
    nativeEntries: List<String>,
    expectedEntry: String,
) {
    val productEntries =
        nativeEntries.filter { entry ->
            entry == expectedEntry || entry.lowercase().contains("harvestcircle")
        }
    require(productEntries.none { it != expectedEntry }) {
        "Package contains an unexpected or test native payload"
    }
    require(productEntries.size == 1) {
        "Package must contain exactly one production native library"
    }
}

private fun commandOutput(vararg command: String): String {
    val process = ProcessBuilder(*command).redirectErrorStream(true).start()
    val output = process.inputStream.bufferedReader().use { it.readText() }.trim()
    require(process.waitFor() == 0) { "External package inspection failed" }
    return output
}

private fun containsSecretMaterial(output: String): Boolean =
    Regex("(?i)(nsec1[0-9a-z]{20,}|private[_ -]?key|secret[_ -]?key)").containsMatchIn(output)
