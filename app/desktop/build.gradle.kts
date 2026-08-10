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
import org.gradle.jvm.tasks.Jar
import org.harvestcircle.buildlogic.plugins.HarvestCircleRustFfiExtension
import org.harvestcircle.gradle.FfiCompatibilityBaseline
import org.harvestcircle.gradle.ProductCoordinates
import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import java.io.File
import java.util.jar.JarFile

plugins {
    id("org.harvestcircle.build.desktop-app")
    id("org.harvestcircle.build.rust-ffi")
}

val rustManifest =
    rootProject.layout.projectDirectory
        .file("core/Cargo.toml")
        .asFile

fun workspacePackageValue(key: String): String {
    val workspacePackage =
        rustManifest
            .readText()
            .substringAfter("[workspace.package]", missingDelimiterValue = "")
            .substringBefore("\n[")
    require(workspacePackage.isNotBlank()) { "Cargo manifest is missing [workspace.package]" }
    val expression = Regex("""(?m)^${Regex.escape(key)}\s*=\s*"([^"]+)"\s*$""")
    return expression
        .find(workspacePackage)
        ?.groupValues
        ?.get(1)
        ?: throw GradleException("Cargo workspace package metadata is missing $key")
}

val productCoordinatesFile =
    rootProject.layout.projectDirectory.file("config/product/harvestcircle-v1.properties")
val productCoordinates =
    ProductCoordinates.parse(providers.fileContents(productCoordinatesFile).asText.get())
val ffiCompatibilityBaselineFile =
    rootProject.layout.projectDirectory.file("core/compatibility/harvestcircle-ffi-v4.properties")
val ffiCompatibilityBaseline =
    FfiCompatibilityBaseline.load(ffiCompatibilityBaselineFile.asFile)
val appVersion = workspacePackageValue("version")
val macOsBuildVersion = "1"
check(ffiCompatibilityBaseline["product.version"] == appVersion) {
    "FFI compatibility product version must match the Cargo workspace version"
}
check(ffiCompatibilityBaseline["product.coordinate_digest"] == productCoordinates.digest) {
    "FFI compatibility product-coordinate digest is stale"
}
val installableVersion = ffiCompatibilityBaseline["package.version"]
check(Regex("""[1-9]\d*(\.\d+){0,2}""").matches(installableVersion)) {
    "Package version must satisfy the macOS jpackage contract"
}
val applicationName = productCoordinates["product.name"]
val productSlug = productCoordinates["product.slug"]
val bundleId = productCoordinates["desktop.bundle_id"]
val copyrightNotice = productCoordinates["copyright.notice"]
val vendorName = productCoordinates["vendor.name"]
val rustFfi = extensions.getByType<HarvestCircleRustFfiExtension>()
val nativeOsName = rustFfi.nativeOsName.get()
val isMacOsHost = nativeOsName.lowercase().startsWith("mac")
val isLinuxHost = nativeOsName.lowercase().startsWith("linux")
val isWindowsHost = nativeOsName.lowercase().startsWith("windows")
val rustLibraryName = rustFfi.libraryName.get()
val rustDebugLibrary = rustFfi.debugLibrary.get().asFile
val rustReleaseLibrary = rustFfi.releaseLibrary.get().asFile
val jnaPlatformPrefix = rustFfi.jnaPlatformPrefix.get()
val buildSourceCommit = rustFfi.sourceCommit
val buildSourceDirty = rustFfi.sourceDirty
val buildRadrootsRevision = rustFfi.radrootsRevision
val buildSourceDateEpoch = rustFfi.sourceDateEpoch
val releaseNativeResourcesJar = tasks.named<Jar>("releaseNativeResourcesJar")
val releaseNativeRuntimeJar =
    releaseNativeResourcesJar
        .get()
        .archiveFile
        .get()
        .asFile

abstract class VerifyDesktopBuildMetadataArtifact : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val desktopJar: RegularFileProperty

    @get:Input
    abstract val expectedBuildEvidence: ListProperty<String>

    @TaskAction
    fun verify() {
        JarFile(desktopJar.get().asFile).use { jar ->
            val entry =
                jar.getJarEntry("org/harvestcircle/application/generated/DesktopBuildMetadata.class")
                    ?: throw GradleException("Desktop build metadata is missing from the application artifact")
            val metadata = jar.getInputStream(entry).use { it.readBytes() }.toString(Charsets.ISO_8859_1)
            expectedBuildEvidence.get().forEach { evidence ->
                require(metadata.contains(evidence)) {
                    "Desktop application artifact is missing generated build evidence"
                }
            }
        }
    }
}
val verifyDesktopBuildMetadataArtifact by tasks.registering(VerifyDesktopBuildMetadataArtifact::class) {
    dependsOn("jar")
    desktopJar.set(tasks.named<Jar>("jar").flatMap { it.archiveFile })
    expectedBuildEvidence.set(
        listOf(
            appVersion,
            installableVersion,
            gradle.gradleVersion,
            System.getProperty("java.version"),
            libs.versions.kotlin.get(),
            libs.versions.compose.get(),
        ),
    )
}

abstract class VerifyMacOsDistribution : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val appDirectory: DirectoryProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val releaseLibrary: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val iconSource: RegularFileProperty

    @get:Input
    abstract val expectedBundleId: Property<String>

    @get:Input
    abstract val expectedPackageVersion: Property<String>

    @get:Input
    abstract val expectedBuildVersion: Property<String>

    @get:Input
    abstract val expectedNativeEntry: Property<String>

    @TaskAction
    fun verify() {
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
            app
                .walkTopDown()
                .filter { it.isFile && it.extension == "icns" }
                .count { it.readBytes().contentEquals(sourceIcon) }
        require(matchingIcons == 1) { "Packaged macOS icon does not match the canonical icon" }

        val expectedEntry = expectedNativeEntry.get()
        val packagedLibraries = mutableListOf<ByteArray>()
        app
            .walkTopDown()
            .filter { it.isFile && it.extension == "jar" }
            .forEach { jarFile ->
                JarFile(jarFile).use { jar ->
                    jar.getJarEntry(expectedEntry)?.let { entry ->
                        packagedLibraries += jar.getInputStream(entry).use { it.readBytes() }
                    }
                }
            }
        require(packagedLibraries.size == 1) {
            "Packaged application must contain exactly one release native library"
        }
        val release = releaseLibrary.get().asFile
        val packaged = temporaryDir.resolve(release.name).apply { writeBytes(packagedLibraries.single()) }
        require(machOIdentity(packaged) == machOIdentity(release)) {
            "Packaged native library identity does not match the Cargo release artifact"
        }
        commandOutput("/usr/bin/codesign", "--verify", "--strict", packaged.absolutePath)
    }

    private fun plistValue(
        plist: File,
        key: String,
    ): String = commandOutput("/usr/libexec/PlistBuddy", "-c", "Print :$key", plist.absolutePath)

    private fun machOIdentity(binary: File): String {
        val output = commandOutput("/usr/bin/dwarfdump", "--uuid", binary.absolutePath)
        return Regex("""UUID: ([0-9A-F-]+) \(([^)]+)\)""")
            .find(output)
            ?.value
            ?: throw GradleException("Could not read the packaged native library identity")
    }

    private fun commandOutput(vararg command: String): String {
        val process =
            ProcessBuilder(*command)
                .redirectErrorStream(true)
                .start()
        val output =
            process.inputStream
                .bufferedReader()
                .use { it.readText() }
                .trim()
        require(process.waitFor() == 0) { "External package inspection failed: $output" }
        return output
    }
}

abstract class VerifyMacOsPackage : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val packageDirectory: DirectoryProperty

    @get:Input
    abstract val expectedFileName: Property<String>

    @TaskAction
    fun verify() {
        val packages = packageDirectory.asFileTree.files.filter { it.isFile && it.extension == "dmg" }
        require(packages.size == 1) { "Expected exactly one macOS disk image" }
        require(packages.single().name == expectedFileName.get()) { "Unexpected macOS disk image name" }
        require(packages.single().length() > 0L) { "Packaged macOS disk image is empty" }
    }
}

abstract class VerifyNativeInstallPackage : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val packageDirectory: DirectoryProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val releaseLibrary: RegularFileProperty

    @get:Input
    abstract val packageExtension: Property<String>

    @get:Input
    abstract val expectedVersion: Property<String>

    @get:Input
    abstract val expectedNativeEntry: Property<String>

    @get:Input
    abstract val hostFamily: Property<String>

    @TaskAction
    fun verify() {
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

        val expectedEntry = expectedNativeEntry.get()
        val packagedLibraries = mutableListOf<ByteArray>()
        extracted
            .walkTopDown()
            .filter { it.isFile && it.extension == "jar" }
            .forEach { jarFile ->
                JarFile(jarFile).use { jar ->
                    jar.getJarEntry(expectedEntry)?.let { entry ->
                        packagedLibraries += jar.getInputStream(entry).use { it.readBytes() }
                    }
                }
            }
        require(packagedLibraries.size == 1) {
            "Installation package must contain exactly one canonical native library"
        }
        require(packagedLibraries.single().contentEquals(releaseLibrary.get().asFile.readBytes())) {
            "Installed native library does not match the canonical Cargo release artifact"
        }
    }

    private fun commandOutput(vararg command: String) {
        val process =
            ProcessBuilder(*command)
                .redirectErrorStream(true)
                .start()
        val output =
            process.inputStream
                .bufferedReader()
                .use { it.readText() }
                .trim()
        require(process.waitFor() == 0) { "Installation package extraction failed: $output" }
    }
}

abstract class VerifyMacOsDeveloperIdSignature : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val appDirectory: DirectoryProperty

    @TaskAction
    fun verify() {
        val app = appDirectory.get().asFile
        commandOutput("/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", app.absolutePath)
        val signature = commandOutput("/usr/bin/codesign", "--display", "--verbose=4", app.absolutePath)
        require(!signature.contains("Signature=adhoc")) {
            "Release application is ad-hoc signed; a Developer ID Application signature is required"
        }
        require(signature.lineSequence().any { it.startsWith("Authority=Developer ID Application:") }) {
            "Release application is not signed by a Developer ID Application identity"
        }
        require(
            signature.lineSequence().any {
                it.startsWith("TeamIdentifier=") && it != "TeamIdentifier=not set"
            },
        ) {
            "Release application signature has no Apple team identifier"
        }
    }

    private fun commandOutput(vararg command: String): String {
        val process =
            ProcessBuilder(*command)
                .redirectErrorStream(true)
                .start()
        val output =
            process.inputStream
                .bufferedReader()
                .use { it.readText() }
                .trim()
        require(process.waitFor() == 0) { "Code-signature verification failed: $output" }
        return output
    }
}

abstract class VerifyMacOsNotarization : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val diskImage: RegularFileProperty

    @TaskAction
    fun verify() {
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

    private fun commandOutput(vararg command: String): String {
        val process =
            ProcessBuilder(*command)
                .redirectErrorStream(true)
                .start()
        val output =
            process.inputStream
                .bufferedReader()
                .use { it.readText() }
                .trim()
        require(process.waitFor() == 0) { "Notarization verification failed: $output" }
        return output
    }
}

tasks.named("check") {
    dependsOn(verifyDesktopBuildMetadataArtifact)
}

compose.desktop {
    application {
        disableDefaultConfiguration()
        dependsOn(releaseNativeResourcesJar.get())
        dependsOn("jar")
        val desktopJar = tasks.named<Jar>("jar").flatMap { it.archiveFile }
        mainJar.set(desktopJar)
        fromFiles(desktopJar, configurations.runtimeClasspath, releaseNativeRuntimeJar)
        if (isMacOsHost) {
            jvmArgs +=
                listOf(
                    "-Dapple.awt.application.name=$applicationName",
                    "-Dapple.awt.application.appearance=system",
                )
        }

        nativeDistributions {
            targetFormats(
                when {
                    isMacOsHost -> TargetFormat.Dmg
                    isLinuxHost -> TargetFormat.Deb
                    isWindowsHost -> TargetFormat.Msi
                    else -> throw GradleException("Unsupported desktop package host: $nativeOsName")
                },
            )

            packageName = applicationName
            packageVersion = installableVersion
            description = "$applicationName $appVersion"
            copyright = copyrightNotice
            vendor = vendorName

            macOS {
                bundleID = bundleId
                iconFile.set(project.file("src/main/resources/icons/$productSlug.icns"))
                packageName = applicationName
                dockName = applicationName
                packageBuildVersion = macOsBuildVersion
            }
        }
    }
}

val verifyMacOsDistribution by tasks.registering(VerifyMacOsDistribution::class) {
    dependsOn("createDistributable")
    appDirectory.set(layout.buildDirectory.dir("compose/binaries/main/app/$applicationName.app"))
    releaseLibrary.set(rustReleaseLibrary)
    iconSource.set(layout.projectDirectory.file("src/main/resources/icons/$productSlug.icns"))
    expectedBundleId.set(bundleId)
    expectedPackageVersion.set(installableVersion)
    expectedBuildVersion.set(macOsBuildVersion)
    expectedNativeEntry.set("$jnaPlatformPrefix/$rustLibraryName")
}
tasks.matching { it.name == "createDistributable" }.configureEach {
    dependsOn(releaseNativeResourcesJar)
}
val verifyMacOsPackage by tasks.registering(VerifyMacOsPackage::class) {
    dependsOn("packageDmg", verifyMacOsDistribution)
    packageDirectory.set(layout.buildDirectory.dir("compose/binaries/main/dmg"))
    expectedFileName.set("$applicationName-$installableVersion.dmg")
}
val verifyLinuxPackage by tasks.registering(VerifyNativeInstallPackage::class) {
    dependsOn("packageDeb", "verifyReleaseNativeLibrary")
    packageDirectory.set(layout.buildDirectory.dir("compose/binaries/main/deb"))
    releaseLibrary.set(rustReleaseLibrary)
    packageExtension.set("deb")
    expectedVersion.set(installableVersion)
    expectedNativeEntry.set("$jnaPlatformPrefix/$rustLibraryName")
    hostFamily.set("linux")
}
val verifyWindowsPackage by tasks.registering(VerifyNativeInstallPackage::class) {
    dependsOn("packageMsi", "verifyReleaseNativeLibrary")
    packageDirectory.set(layout.buildDirectory.dir("compose/binaries/main/msi"))
    releaseLibrary.set(rustReleaseLibrary)
    packageExtension.set("msi")
    expectedVersion.set(installableVersion)
    expectedNativeEntry.set("$jnaPlatformPrefix/$rustLibraryName")
    hostFamily.set("windows")
}
val verifyHostPackage by tasks.registering {
    when {
        isMacOsHost -> dependsOn(verifyMacOsPackage)
        isLinuxHost -> dependsOn(verifyLinuxPackage)
        isWindowsHost -> dependsOn(verifyWindowsPackage)
        else -> throw GradleException("Unsupported desktop package host: $nativeOsName")
    }
}
val verifyMacOsDeveloperIdSignature by tasks.registering(VerifyMacOsDeveloperIdSignature::class) {
    dependsOn(verifyMacOsPackage)
    appDirectory.set(layout.buildDirectory.dir("compose/binaries/main/app/$applicationName.app"))
}
val verifyMacOsNotarization by tasks.registering(VerifyMacOsNotarization::class) {
    dependsOn(verifyMacOsPackage)
    diskImage.set(layout.buildDirectory.file("compose/binaries/main/dmg/$applicationName-$installableVersion.dmg"))
}

abstract class VerifyReleaseBuildProvenance : DefaultTask() {
    @get:Input
    abstract val sourceCommit: Property<String>

    @get:Input
    abstract val sourceDirty: Property<String>

    @get:Input
    abstract val radrootsRevision: Property<String>

    @get:Input
    abstract val sourceDateEpoch: Property<String>

    @TaskAction
    fun verify() {
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
val verifyReleaseBuildProvenance by tasks.registering(VerifyReleaseBuildProvenance::class) {
    sourceCommit.set(buildSourceCommit)
    sourceDirty.set(buildSourceDirty)
    radrootsRevision.set(buildRadrootsRevision)
    sourceDateEpoch.set(buildSourceDateEpoch)
}
val sourceReadiness by tasks.registering {
    dependsOn(
        ":verifyProductCoordinates",
        ":verifyVerificationLanes",
        ":verifyFoundationBoundaries",
        ":verifyFoundationArchive",
        ":app:shared:check",
        "check",
        "verifyUniFfiBindings",
    )
}
val packageReadiness by tasks.registering {
    dependsOn(verifyHostPackage, verifyReleaseBuildProvenance, verifyDesktopBuildMetadataArtifact)
}
val signingReadiness by tasks.registering {
    dependsOn(verifyMacOsDeveloperIdSignature)
}
val notarizationReadiness by tasks.registering {
    dependsOn(verifyMacOsNotarization)
}
tasks.register("releaseReadiness") {
    dependsOn("checkLicense", "dependencyCheckAnalyze", sourceReadiness, packageReadiness)
    if (isMacOsHost) {
        dependsOn(signingReadiness, notarizationReadiness)
    }
}
