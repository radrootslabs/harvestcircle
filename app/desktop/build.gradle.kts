import com.github.jk1.license.filter.SpdxLicenseBundleNormalizer
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Delete
import org.gradle.api.tasks.Exec
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.api.tasks.testing.Test
import org.gradle.jvm.tasks.Jar
import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile
import java.io.File
import java.util.Properties
import java.util.jar.JarFile

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.compose.compiler)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
    alias(libs.plugins.license.report)
    alias(libs.plugins.owasp.dependency.check)
}

licenseReport {
    projects = arrayOf(project)
    configurations = arrayOf("runtimeClasspath")
    filters = arrayOf(SpdxLicenseBundleNormalizer())
    allowedLicensesFile = rootProject.layout.projectDirectory.file("config/licenses/allowed-licenses.json")
}

dependencyCheck {
    failBuildOnCVSS = 0.0F
    failOnError = true
    formats = listOf("HTML", "JSON")
    scanConfigurations = listOf("runtimeClasspath")
    skipTestGroups = true
    providers.environmentVariable("NVD_API_KEY").orNull?.takeIf(String::isNotBlank)?.let {
        nvd.apiKey = it
    }
}

tasks.matching { it.name.startsWith("dependencyCheck") }.configureEach {
    notCompatibleWithConfigurationCache("Advisory data and environment-only credentials must not be cached")
}

configure<org.jlleitschuh.gradle.ktlint.KtlintExtension> {
    additionalEditorconfig.set(
        mapOf(
            "ktlint_function_naming_ignore_when_annotated_with" to "Composable",
        ),
    )
    filter {
        exclude("**/generated/uniffi/**")
    }
}

val rustCoreSource =
    rootProject.layout.projectDirectory
        .dir("core")
        .asFile
providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("app-desktop"))
}
val cargoTargetRoot =
    providers.environmentVariable("CARGO_TARGET_DIR").orNull?.let(::file)
        ?: rustCoreSource.resolve("target")

val radrootsOffline =
    providers
        .gradleProperty("radrootsOffline")
        .map(String::toBooleanStrict)
        .orElse(false)
val immutableCargoArguments =
    if (radrootsOffline.get()) {
        listOf("--frozen", "--offline")
    } else {
        listOf("--locked")
    }

val rustManifest = rustCoreSource.resolve("Cargo.toml")

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

val compatibilityBaseline = rootProject.layout.projectDirectory.file("core/compatibility/v5-baseline.properties")
val appVersion = workspacePackageValue("version")
val macOsBuildVersion = "1"
val baseline =
    Properties().apply {
        compatibilityBaseline.asFile.inputStream().use(::load)
    }

fun baselineValue(key: String): String =
    baseline.getProperty(key)?.takeIf(String::isNotBlank)
        ?: throw GradleException("Compatibility baseline is missing $key")

check(baselineValue("ffi.runtime.version") == appVersion) {
    "Compatibility runtime version must match the Cargo workspace version"
}
val installableVersion = baselineValue("package.version")
check(Regex("""[1-9]\d*(\.\d+){0,2}""").matches(installableVersion)) {
    "Compatibility package version must satisfy the macOS jpackage contract"
}
val applicationName = baselineValue("package.name")
val bundleId = baselineValue("package.bundle_id")
val applicationNamespace = baselineValue("source.namespace")
version = appVersion

val rustSources =
    fileTree(rustCoreSource) {
        include(
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "compatibility/**",
            "crates/**",
        )
        exclude("target/**")
    }

data class NativeTarget(
    val libraryName: String,
    val jnaPrefix: String,
)

fun resolveNativeTarget(
    osName: String,
    architecture: String,
): NativeTarget {
    val os = osName.lowercase()
    val arch = architecture.lowercase()
    return when {
        os.startsWith("mac") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("libharvestcircle_ffi.dylib", "darwin-aarch64")
        os.startsWith("mac") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("libharvestcircle_ffi.dylib", "darwin-x86-64")
        os.startsWith("windows") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("harvestcircle_ffi.dll", "win32-aarch64")
        os.startsWith("windows") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("harvestcircle_ffi.dll", "win32-x86-64")
        os.startsWith("linux") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("libharvestcircle_ffi.so", "linux-aarch64")
        os.startsWith("linux") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("libharvestcircle_ffi.so", "linux-x86-64")
        else -> throw GradleException("Unsupported native desktop host: $osName/$architecture")
    }
}

val nativeOsName = providers.gradleProperty("nativeOs").getOrElse(System.getProperty("os.name"))
val nativeArchitecture = providers.gradleProperty("nativeArch").getOrElse(System.getProperty("os.arch"))
val nativeTarget = resolveNativeTarget(nativeOsName, nativeArchitecture)
val isMacOsHost = nativeOsName.lowercase().startsWith("mac")
val isLinuxHost = nativeOsName.lowercase().startsWith("linux")
val isWindowsHost = nativeOsName.lowercase().startsWith("windows")
val rustLibraryName = nativeTarget.libraryName
val rustDebugLibrary = file(cargoTargetRoot).resolve("debug/$rustLibraryName")
val rustReleaseLibrary = file(cargoTargetRoot).resolve("release/$rustLibraryName")
val jnaPlatformPrefix = nativeTarget.jnaPrefix

val buildRustCoreDebug by tasks.registering(Exec::class) {
    workingDir(rustCoreSource)
    commandLine(
        "cargo",
        "build",
        "--manifest-path",
        rustManifest.absolutePath,
        "-p",
        "harvestcircle_ffi",
        *immutableCargoArguments.toTypedArray(),
    )
    inputs.files(rustSources)
    outputs.file(rustDebugLibrary)
}

val buildRustCoreRelease by tasks.registering(Exec::class) {
    workingDir(rustCoreSource)
    commandLine(
        "cargo",
        "build",
        "--release",
        "--manifest-path",
        rustManifest.absolutePath,
        "-p",
        "harvestcircle_ffi",
        *immutableCargoArguments.toTypedArray(),
    )
    inputs.files(rustSources)
    outputs.file(rustReleaseLibrary)
}

val generatedUniFfiKotlin = layout.buildDirectory.dir("generated/uniffi/kotlin")
val generatedReleaseNativeResources = layout.buildDirectory.dir("generated/uniffi/release-native-resources")
val cleanGeneratedUniFfiKotlin by tasks.registering(Delete::class) {
    delete(generatedUniFfiKotlin)
}
val generateUniFfiKotlin by tasks.registering(Exec::class) {
    dependsOn(buildRustCoreDebug, cleanGeneratedUniFfiKotlin)
    workingDir(rustCoreSource)
    commandLine(
        "cargo",
        "run",
        "--manifest-path",
        rustManifest.absolutePath,
        "-p",
        "harvestcircle_uniffi_bindgen",
        *immutableCargoArguments.toTypedArray(),
        "--",
        "generate",
        "--library",
        "--language",
        "kotlin",
        "--metadata-no-deps",
        "--no-format",
        "--config",
        rustCoreSource.resolve("crates/harvestcircle_ffi/uniffi.toml").absolutePath,
        "--out-dir",
        generatedUniFfiKotlin.get().asFile.absolutePath,
        rustDebugLibrary.absolutePath,
    )
    inputs.file(rustDebugLibrary)
    inputs.file(rustCoreSource.resolve("crates/harvestcircle_ffi/uniffi.toml"))
    outputs.dir(generatedUniFfiKotlin)
}

abstract class VerifyUniFfiBindings : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val generatedDirectory: DirectoryProperty

    @get:Input
    abstract val expectedPackage: Property<String>

    @TaskAction
    fun verify() {
        val kotlinFiles =
            generatedDirectory.asFileTree.files
                .filter { it.isFile && it.extension == "kt" }
        if (kotlinFiles.size != 1) {
            throw GradleException("Expected exactly one generated UniFFI Kotlin source")
        }
        if (!kotlinFiles.single().readText().contains("package ${expectedPackage.get()}")) {
            throw GradleException("Generated UniFFI Kotlin package does not match the runtime contract")
        }
    }
}
val verifyUniFfiBindings by tasks.registering(VerifyUniFfiBindings::class) {
    dependsOn(generateUniFfiKotlin)
    generatedDirectory.set(generatedUniFfiKotlin)
    expectedPackage.set("org.radroots.harvestcircle.ffi")
}
val cleanReleaseNativeResources by tasks.registering(Delete::class) {
    delete(generatedReleaseNativeResources)
}
val stageReleaseNativeLibrary by tasks.registering(Copy::class) {
    dependsOn(buildRustCoreRelease, cleanReleaseNativeResources)
    from(rustReleaseLibrary)
    into(generatedReleaseNativeResources.map { it.dir(jnaPlatformPrefix) })
}

abstract class VerifyReleaseNativeLibrary : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val releaseLibrary: RegularFileProperty

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val stagedDirectory: DirectoryProperty

    @get:Input
    abstract val expectedName: Property<String>

    @TaskAction
    fun verify() {
        val files = stagedDirectory.asFileTree.files.filter { it.isFile }
        if (files.size != 1) {
            throw GradleException("Release resources must contain exactly one native library")
        }
        if (files.single().name != expectedName.get()) {
            throw GradleException("Unexpected release native library name")
        }
        if (!files.single().readBytes().contentEquals(releaseLibrary.get().asFile.readBytes())) {
            throw GradleException("Staged native library does not match the Cargo release artifact")
        }
    }
}
val verifyReleaseNativeLibrary by tasks.registering(VerifyReleaseNativeLibrary::class) {
    dependsOn(stageReleaseNativeLibrary)
    releaseLibrary.set(rustReleaseLibrary)
    stagedDirectory.set(generatedReleaseNativeResources)
    expectedName.set(rustLibraryName)
}
val releaseNativeResourcesJar by tasks.registering(Jar::class) {
    dependsOn(verifyReleaseNativeLibrary)
    archiveClassifier.set("release-native-resources")
    from(generatedReleaseNativeResources)
}
val releaseNativeRuntimeJar =
    releaseNativeResourcesJar
        .get()
        .archiveFile
        .get()
        .asFile

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

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(libs.compose.foundation)
    implementation(libs.jna)
    implementation(libs.kotlinx.coroutines.core)

    testImplementation(kotlin("test-junit"))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.kotlinx.coroutines.test)
}

val testInventoryRoot =
    providers
        .gradleProperty("testInventoryRoot")
        .orElse("src/test/kotlin")
val expectedTests =
    fileTree(testInventoryRoot.get()) {
        include("**/*Test.kt")
    }

abstract class VerifyTestInventory : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val testFiles: ConfigurableFileCollection

    @get:Input
    abstract val sourceRoot: Property<String>

    @TaskAction
    fun verify() {
        if (testFiles.isEmpty) {
            throw GradleException("No Kotlin tests found under ${sourceRoot.get()}")
        }
    }
}
val verifyTestInventory by tasks.registering(VerifyTestInventory::class) {
    testFiles.from(expectedTests)
    sourceRoot.set(testInventoryRoot)
}

tasks.withType<Test>().configureEach {
    dependsOn(verifyTestInventory)
    failOnNoDiscoveredTests.set(true)
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }

    jvmToolchain(21)
}

tasks.named<KotlinCompile>("compileKotlin") {
    dependsOn(generateUniFfiKotlin)
    source(generatedUniFfiKotlin)
}
tasks.named("runKtlintCheckOverMainSourceSet") {
    dependsOn(generateUniFfiKotlin)
}
tasks.named("runKtlintFormatOverMainSourceSet") {
    dependsOn(generateUniFfiKotlin)
}
tasks.withType<Test>().configureEach {
    dependsOn(buildRustCoreDebug)
    environment(
        "HARVESTCIRCLE_DEVELOPMENT_DATA_DIR",
        layout.buildDirectory
            .dir("native-test-data")
            .get()
            .asFile.absolutePath,
    )
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.parentFile.absolutePath,
    )
}
tasks.withType<JavaExec>().configureEach {
    dependsOn(buildRustCoreDebug)
    systemProperty("harvestcircle.development", "true")
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.parentFile.absolutePath,
    )
}

compose.desktop {
    application {
        disableDefaultConfiguration()
        dependsOn(releaseNativeResourcesJar.get())
        dependsOn("jar")
        val desktopJar = tasks.named<Jar>("jar").flatMap { it.archiveFile }
        mainJar.set(desktopJar)
        fromFiles(desktopJar, configurations.runtimeClasspath, releaseNativeRuntimeJar)
        mainClass = "$applicationNamespace.desktop.MainKt"

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
            description = "HarvestCircle $appVersion"
            copyright = "Copyright © 2024 Radroots, Inc."
            vendor = "Radroots, Inc"

            macOS {
                bundleID = bundleId
                iconFile.set(project.file("src/main/resources/icons/harvestcircle.icns"))
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
    iconSource.set(layout.projectDirectory.file("src/main/resources/icons/harvestcircle.icns"))
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
    dependsOn("packageDeb", verifyReleaseNativeLibrary)
    packageDirectory.set(layout.buildDirectory.dir("compose/binaries/main/deb"))
    releaseLibrary.set(rustReleaseLibrary)
    packageExtension.set("deb")
    expectedVersion.set(installableVersion)
    expectedNativeEntry.set("$jnaPlatformPrefix/$rustLibraryName")
    hostFamily.set("linux")
}
val verifyWindowsPackage by tasks.registering(VerifyNativeInstallPackage::class) {
    dependsOn("packageMsi", verifyReleaseNativeLibrary)
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
tasks.register("releaseReadiness") {
    dependsOn("checkLicense", "dependencyCheckAnalyze", verifyHostPackage)
    if (isMacOsHost) {
        dependsOn(verifyMacOsDeveloperIdSignature, verifyMacOsNotarization)
    }
}
