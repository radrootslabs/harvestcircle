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

version = "0.1.0-alpha"

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.compose.compiler)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

configure<org.jlleitschuh.gradle.ktlint.KtlintExtension> {
    additionalEditorconfig.set(
        mapOf(
            "ktlint_function_naming_ignore_when_annotated_with" to "Composable",
        ),
    )
    filter {
        exclude { it.file.path.contains("/build/generated/") }
    }
}

tasks.withType<org.jlleitschuh.gradle.ktlint.tasks.BaseKtLintCheckTask>().configureEach {
    exclude { it.file.path.contains("/build/generated/") }
}

val rustManifest = rootProject.layout.projectDirectory.file("core/Cargo.toml")
val rustSources =
    rootProject.fileTree("core") {
        include(
            "**/Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "**/*.rs",
            "**/*.sql",
            "**/uniffi.toml",
            "compatibility/**",
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
            NativeTarget("libradroots_studio_ffi.dylib", "darwin-aarch64")
        os.startsWith("mac") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("libradroots_studio_ffi.dylib", "darwin-x86-64")
        os.startsWith("windows") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("radroots_studio_ffi.dll", "win32-aarch64")
        os.startsWith("windows") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("radroots_studio_ffi.dll", "win32-x86-64")
        os.startsWith("linux") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("libradroots_studio_ffi.so", "linux-aarch64")
        os.startsWith("linux") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("libradroots_studio_ffi.so", "linux-x86-64")
        else -> throw GradleException("Unsupported native desktop host: $osName/$architecture")
    }
}

val nativeTarget =
    resolveNativeTarget(
        providers.gradleProperty("nativeOs").getOrElse(System.getProperty("os.name")),
        providers.gradleProperty("nativeArch").getOrElse(System.getProperty("os.arch")),
    )
val rustLibraryName = nativeTarget.libraryName
val rustDebugLibrary = rootProject.layout.projectDirectory.file("core/target/debug/$rustLibraryName")
val rustReleaseLibrary = rootProject.layout.projectDirectory.file("core/target/release/$rustLibraryName")
val jnaPlatformPrefix = nativeTarget.jnaPrefix

val buildRustCoreDebug by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir)
    commandLine(
        "cargo",
        "build",
        "--manifest-path",
        rustManifest.asFile.absolutePath,
        "-p",
        "radroots-studio-ffi",
    )
    inputs.files(rustSources)
    outputs.file(rustDebugLibrary)
}

val buildRustCoreRelease by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir)
    commandLine(
        "cargo",
        "build",
        "--release",
        "--manifest-path",
        rustManifest.asFile.absolutePath,
        "-p",
        "radroots-studio-ffi",
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
    workingDir(rootProject.projectDir)
    commandLine(
        "cargo",
        "run",
        "--manifest-path",
        rustManifest.asFile.absolutePath,
        "-p",
        "radroots-studio-uniffi-bindgen",
        "--",
        "generate",
        "--library",
        "--language",
        "kotlin",
        "--metadata-no-deps",
        "--no-format",
        "--config",
        rootProject.layout.projectDirectory
            .file("core/crates/ffi/uniffi.toml")
            .asFile.absolutePath,
        "--out-dir",
        generatedUniFfiKotlin.get().asFile.absolutePath,
        rustDebugLibrary.asFile.absolutePath,
    )
    inputs.file(rustDebugLibrary)
    inputs.file(rootProject.layout.projectDirectory.file("core/crates/ffi/uniffi.toml"))
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
    expectedPackage.set("org.radroots.studio.ffi")
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
    sourceSets.main {
        kotlin.srcDir(generatedUniFfiKotlin)
    }

    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }

    jvmToolchain(21)
}

tasks.named("compileKotlin") {
    dependsOn(generateUniFfiKotlin)
}
tasks.named("runKtlintCheckOverMainSourceSet") {
    dependsOn(generateUniFfiKotlin)
}
tasks.named("runKtlintFormatOverMainSourceSet") {
    dependsOn(generateUniFfiKotlin)
}
tasks.withType<Test>().configureEach {
    dependsOn(buildRustCoreDebug)
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.asFile.parentFile.absolutePath,
    )
}
tasks.withType<JavaExec>().configureEach {
    dependsOn(buildRustCoreDebug)
    systemProperty("radroots.studio.development", "true")
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.asFile.parentFile.absolutePath,
    )
}

compose.desktop {
    application {
        dependsOn(releaseNativeResourcesJar.get())
        fromFiles(releaseNativeResourcesJar)
        mainClass = "org.radroots.studio.desktop.MainKt"

        jvmArgs +=
            listOf(
                "-Dapple.awt.application.name=Radroots",
                "-Dapple.awt.application.appearance=system",
            )

        nativeDistributions {
            targetFormats(TargetFormat.Dmg)

            packageName = "Radroots"
            packageVersion = "1.0.0"
            description = "Radroots Studio 0.1.0-alpha"
            copyright = "Copyright © 2024 Radroots, Inc."
            vendor = "Radroots, Inc"

            macOS {
                bundleID = "org.radroots.studio"
                iconFile.set(project.file("src/main/resources/icons/radroots.icns"))
                packageName = "Radroots"
                dockName = "Radroots"
                packageBuildVersion = "1"
            }
        }
    }
}
