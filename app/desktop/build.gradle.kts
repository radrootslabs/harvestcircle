import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Exec
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.api.tasks.testing.Test
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

version = "0.1.0-alpha"

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.compose.compiler)
}

val rustManifest = rootProject.layout.projectDirectory.file("core/Cargo.toml")
val rustSources = rootProject.fileTree("core") {
    include("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/**/*.rs", "crates/**/*.sql")
    exclude("target/**")
}
val rustLibraryName = when {
    System.getProperty("os.name").startsWith("Mac") -> "libradroots_studio_ffi.dylib"
    System.getProperty("os.name").startsWith("Windows") -> "radroots_studio_ffi.dll"
    else -> "libradroots_studio_ffi.so"
}
val rustDebugLibrary = rootProject.layout.projectDirectory
    .file("core/target/debug/$rustLibraryName")
val jnaPlatformPrefix = when {
    System.getProperty("os.name").startsWith("Mac") &&
        System.getProperty("os.arch") == "aarch64" -> "darwin-aarch64"
    System.getProperty("os.name").startsWith("Mac") -> "darwin-x86-64"
    System.getProperty("os.name").startsWith("Windows") &&
        System.getProperty("os.arch") == "aarch64" -> "win32-aarch64"
    System.getProperty("os.name").startsWith("Windows") -> "win32-x86-64"
    System.getProperty("os.arch") == "aarch64" -> "linux-aarch64"
    else -> "linux-x86-64"
}

val buildRustCore by tasks.registering(Exec::class) {
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

val generatedUniFfiKotlin = layout.buildDirectory.dir("generated/uniffi/kotlin")
val generatedNativeResources = layout.buildDirectory.dir("generated/uniffi/native-resources")
val generateUniFfiKotlin by tasks.registering(Exec::class) {
    dependsOn(buildRustCore)
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
val stageUniFfiNativeLibrary by tasks.registering(Copy::class) {
    dependsOn(buildRustCore)
    from(rustDebugLibrary)
    into(generatedNativeResources.map { it.dir(jnaPlatformPrefix) })
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(libs.compose.foundation)
    implementation(libs.jna)

    testImplementation(kotlin("test-junit"))
    testImplementation(libs.compose.ui.test.junit4)
}

val testInventoryRoot = providers
    .gradleProperty("testInventoryRoot")
    .orElse("src/test/kotlin")
val expectedTests = fileTree(testInventoryRoot.get()) {
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
        resources.srcDir(generatedNativeResources)
    }

    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }

    jvmToolchain(21)
}

tasks.named("compileKotlin") {
    dependsOn(generateUniFfiKotlin)
}
tasks.named("processResources") {
    dependsOn(stageUniFfiNativeLibrary)
}
tasks.withType<Test>().configureEach {
    dependsOn(buildRustCore)
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.asFile.parentFile.absolutePath,
    )
}
tasks.withType<JavaExec>().configureEach {
    dependsOn(buildRustCore)
    systemProperty(
        "jna.library.path",
        rustDebugLibrary.asFile.parentFile.absolutePath,
    )
}

compose.desktop {
    application {
        mainClass = "org.radroots.studio.desktop.MainKt"

        jvmArgs += listOf(
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
