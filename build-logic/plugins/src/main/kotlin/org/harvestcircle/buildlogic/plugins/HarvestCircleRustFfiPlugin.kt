package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.JavaExec
import org.gradle.api.tasks.testing.Test
import org.gradle.jvm.tasks.Jar
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.harvestcircle.buildlogic.contracts.resolveNativeTarget
import org.harvestcircle.buildlogic.plugins.tasks.CargoBuildTask
import org.harvestcircle.buildlogic.plugins.tasks.GenerateCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.GenerateUniFfiKotlinTask
import org.harvestcircle.buildlogic.plugins.tasks.StageReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestBridgeIsolation
import org.harvestcircle.buildlogic.plugins.tasks.VerifyUniFfiBindings
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile
import java.security.MessageDigest

public abstract class HarvestCircleRustFfiExtension {
    public abstract val nativeOsName: Property<String>
    public abstract val nativeArchitecture: Property<String>
    public abstract val libraryName: Property<String>
    public abstract val jnaPlatformPrefix: Property<String>
    public abstract val debugLibrary: RegularFileProperty
    public abstract val releaseLibrary: RegularFileProperty
    public abstract val sourceCommit: Property<String>
    public abstract val sourceDirty: Property<String>
    public abstract val radrootsRevision: Property<String>
    public abstract val sourceDateEpoch: Property<String>
}

public class HarvestCircleRustFfiPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target.plugins.hasPlugin("org.harvestcircle.build.desktop-app")) {
            "HarvestCircle Rust FFI convention requires the desktop application convention"
        }
        val catalog = target.extensions.getByType(VersionCatalogsExtension::class.java).named("libs")
        val productCoordinates =
            ProductCoordinates.load(
                target.rootProject.layout.projectDirectory.file("config/product/harvestcircle-v1.properties").asFile,
            )
        val baselineFile =
            target.rootProject.layout.projectDirectory.file("core/compatibility/harvestcircle-ffi-v4.properties")
        val baseline = FfiCompatibilityBaseline.load(baselineFile.asFile)
        val rustRoot = target.rootProject.layout.projectDirectory.dir("core")
        val rustManifest = rustRoot.file("Cargo.toml")
        val cargoTargetRoot =
            target.providers.environmentVariable("CARGO_TARGET_DIR").orNull?.let(target::file)
                ?: rustRoot.dir("target").asFile
        val immutableArguments =
            if (target.providers.gradleProperty("radrootsOffline").map(String::toBooleanStrict).orElse(false).get()) {
                listOf("--frozen", "--offline")
            } else {
                listOf("--locked")
            }
        val osName = target.providers.gradleProperty("nativeOs").getOrElse(System.getProperty("os.name"))
        val architecture = target.providers.gradleProperty("nativeArch").getOrElse(System.getProperty("os.arch"))
        val nativeTarget = resolveNativeTarget(osName, architecture, productCoordinates["ffi.cdylib_name"])
        val debugLibraryFile = target.file(cargoTargetRoot).resolve("debug/${nativeTarget.libraryName}")
        val releaseLibraryFile = target.file(cargoTargetRoot).resolve("release/${nativeTarget.libraryName}")
        val testBridgeTarget = resolveNativeTarget(osName, architecture, "harvestcircle_test_bridge")
        val testBridgeLibraryFile = target.file(cargoTargetRoot).resolve("debug/${testBridgeTarget.libraryName}")
        val environmentPrefix = productCoordinates["environment.prefix"]
        fun productEnvironment(suffix: String): String = environmentPrefix + suffix
        val sourceCommit = target.providers.environmentVariable(productEnvironment("BUILD_SOURCE_COMMIT")).orElse("unknown")
        val sourceDirty = target.providers.environmentVariable(productEnvironment("BUILD_SOURCE_DIRTY")).orElse("unknown")
        val radrootsRevision =
            target.providers.environmentVariable(productEnvironment("BUILD_RADROOTS_REVISION")).orElse("unknown")
        val rustToolchain =
            target.providers.environmentVariable(productEnvironment("BUILD_RUST_TOOLCHAIN")).orElse("1.97.1")
        val javaToolchain =
            target.providers.environmentVariable(productEnvironment("BUILD_JAVA_TOOLCHAIN")).orElse(System.getProperty("java.version"))
        val kotlinToolchain =
            target.providers
                .environmentVariable(productEnvironment("BUILD_KOTLIN_TOOLCHAIN"))
                .orElse(catalog.findVersion("kotlin").get().requiredVersion)
        val sourceDateEpoch = target.providers.environmentVariable("SOURCE_DATE_EPOCH").orElse("0")
        val buildEnvironment =
            target.providers.provider {
                linkedMapOf(
                    productEnvironment("BUILD_SOURCE_COMMIT") to sourceCommit.get(),
                    productEnvironment("BUILD_SOURCE_DIRTY") to sourceDirty.get(),
                    productEnvironment("BUILD_RADROOTS_REVISION") to radrootsRevision.get(),
                    productEnvironment("BUILD_RUST_TOOLCHAIN") to rustToolchain.get(),
                    productEnvironment("BUILD_JAVA_TOOLCHAIN") to javaToolchain.get(),
                    productEnvironment("BUILD_KOTLIN_TOOLCHAIN") to kotlinToolchain.get(),
                    "SOURCE_DATE_EPOCH" to sourceDateEpoch.get(),
                )
            }
        val provenanceDigest =
            buildEnvironment.map { environment ->
                environment.entries.joinToString("\n") { (key, value) -> "$key=$value" }.sha256()
            }
        val rustSources =
            target.fileTree(rustRoot) { tree ->
                tree.include("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "compatibility/**", "crates/**")
                tree.exclude("target/**")
            }

        val buildDebug =
            target.tasks.register("buildRustCoreDebug", CargoBuildTask::class.java) { task ->
                task.workingDirectory.set(rustRoot)
                task.manifestFile.set(rustManifest)
                task.rustSources.from(rustSources)
                task.packageName.set("harvestcircle_ffi")
                task.release.set(false)
                task.immutableArguments.set(immutableArguments)
                task.buildEnvironment.set(buildEnvironment)
                task.libraryFile.set(debugLibraryFile)
            }
        val buildRelease =
            target.tasks.register("buildRustCoreRelease", CargoBuildTask::class.java) { task ->
                task.workingDirectory.set(rustRoot)
                task.manifestFile.set(rustManifest)
                task.rustSources.from(rustSources)
                task.packageName.set("harvestcircle_ffi")
                task.release.set(true)
                task.immutableArguments.set(immutableArguments)
                task.buildEnvironment.set(buildEnvironment)
                task.libraryFile.set(releaseLibraryFile)
            }
        val buildTestBridge =
            target.tasks.register("buildRustTestBridgeDebug", CargoBuildTask::class.java) { task ->
                task.workingDirectory.set(rustRoot)
                task.manifestFile.set(rustManifest)
                task.rustSources.from(rustSources)
                task.packageName.set("harvestcircle_test_bridge")
                task.release.set(false)
                task.immutableArguments.set(immutableArguments)
                task.buildEnvironment.set(buildEnvironment)
                task.libraryFile.set(testBridgeLibraryFile)
            }
        val generatedUniFfi = target.layout.buildDirectory.dir("generated/uniffi/kotlin")
        val generatedCompatibility = target.layout.buildDirectory.dir("generated/compatibility/kotlin")
        val compatibilityFile =
            generatedCompatibility.map {
                it.file("org/harvestcircle/application/generated/NativeCompatibilityExpectations.kt")
            }
        val generateCompatibility =
            target.tasks.register(
                "generateCompatibilityExpectations",
                GenerateCompatibilityExpectations::class.java,
            ) { task ->
                task.baselineFile.set(baselineFile)
                task.outputFile.set(compatibilityFile)
            }
        val verifyGeneratedSources =
            target.tasks.register(
                "verifyGeneratedSources",
                VerifyGeneratedCompatibilityExpectations::class.java,
            ) { task ->
                task.dependsOn(generateCompatibility)
                task.baselineFile.set(baselineFile)
                task.generatedFile.set(compatibilityFile)
            }
        val generateUniFfi =
            target.tasks.register("generateUniFfiKotlin", GenerateUniFfiKotlinTask::class.java) { task ->
                task.dependsOn(buildDebug)
                task.workingDirectory.set(rustRoot)
                task.manifestFile.set(rustManifest)
                task.configFile.set(rustRoot.file("crates/harvestcircle_ffi/uniffi.toml"))
                task.nativeLibrary.set(debugLibraryFile)
                task.immutableArguments.set(immutableArguments)
                task.outputDirectory.set(generatedUniFfi)
            }
        val generatedTestUniFfi = target.layout.buildDirectory.dir("generated/test-bridge/uniffi/kotlin")
        val generateTestUniFfi =
            target.tasks.register("generateTestBridgeUniFfiKotlin", GenerateUniFfiKotlinTask::class.java) { task ->
                task.dependsOn(buildTestBridge)
                task.workingDirectory.set(rustRoot)
                task.manifestFile.set(rustManifest)
                task.configFile.set(rustRoot.file("crates/harvestcircle_test_bridge/uniffi.toml"))
                task.nativeLibrary.set(testBridgeLibraryFile)
                task.immutableArguments.set(immutableArguments)
                task.outputDirectory.set(generatedTestUniFfi)
            }
        val verifyTestBindings =
            target.tasks.register("verifyTestBridgeUniFfiBindings", VerifyUniFfiBindings::class.java) { task ->
                task.dependsOn(generateTestUniFfi)
                task.generatedDirectory.set(generatedTestUniFfi)
                task.expectedPackage.set("org.harvestcircle.testbridge.ffi")
            }
        val verifyBindings =
            target.tasks.register("verifyUniFfiBindings", VerifyUniFfiBindings::class.java) { task ->
                task.dependsOn(generateUniFfi)
                task.generatedDirectory.set(generatedUniFfi)
                task.expectedPackage.set(productCoordinates["ffi.kotlin_package"])
            }
        val stagedRelease = target.layout.buildDirectory.dir("generated/uniffi/release-native-resources")
        val stageRelease =
            target.tasks.register("stageReleaseNativeLibrary", StageReleaseNativeLibrary::class.java) { task ->
                task.dependsOn(buildRelease)
                task.releaseLibrary.set(releaseLibraryFile)
                task.platformPrefix.set(nativeTarget.jnaPrefix)
                task.outputDirectory.set(stagedRelease)
            }
        val verifyRelease =
            target.tasks.register("verifyReleaseNativeLibrary", VerifyReleaseNativeLibrary::class.java) { task ->
                task.dependsOn(stageRelease)
                task.releaseLibrary.set(releaseLibraryFile)
                task.stagedDirectory.set(stagedRelease)
                task.expectedName.set(nativeTarget.libraryName)
                task.expectedBuildEvidence.set(provenanceDigest.map(::listOf))
            }
        val verifyTestIsolation =
            target.tasks.register("verifyTestBridgeIsolation", VerifyTestBridgeIsolation::class.java) { task ->
                task.dependsOn(verifyBindings, verifyTestBindings, verifyRelease)
                task.productionBindings.set(generatedUniFfi)
                task.testBindings.set(generatedTestUniFfi)
                task.releaseNativeResources.set(stagedRelease)
                task.productionLibraryName.set(nativeTarget.libraryName)
                task.testLibraryName.set(testBridgeTarget.libraryName)
            }
        target.tasks.register("releaseNativeResourcesJar", Jar::class.java) { task ->
            task.dependsOn(verifyRelease)
            task.archiveClassifier.set("release-native-resources")
            task.from(stagedRelease)
        }

        val desktopExtension = target.extensions.getByType(HarvestCircleDesktopAppExtension::class.java)
        desktopExtension.generatedKotlinSources.from(generatedUniFfi, generatedCompatibility)
        target.tasks.named("compileKotlin", KotlinCompile::class.java) { task ->
            task.dependsOn(generateUniFfi, generateCompatibility)
        }
        target.tasks.named("compileIntegrationTestKotlin", KotlinCompile::class.java) { task ->
            task.dependsOn(generateTestUniFfi)
            task.source(generatedTestUniFfi)
            task.compilerOptions.freeCompilerArgs.add(
                target.layout.buildDirectory.dir("classes/kotlin/main").map { output ->
                    "-Xfriend-paths=${output.asFile.absolutePath}"
                },
            )
        }
        target.tasks.named("integrationTest", Test::class.java) { task ->
            task.dependsOn(verifyTestIsolation)
            task.systemProperty("jna.library.path", testBridgeLibraryFile.parentFile.absolutePath)
        }
        target.tasks.withType(Test::class.java).configureEach { task ->
            task.dependsOn(buildDebug)
            val nativeTestData = target.layout.buildDirectory.dir("native-test-data").get().asFile.absolutePath
            task.environment(productEnvironment("DEVELOPMENT_DATA_DIR"), nativeTestData)
            task.systemProperty("${productCoordinates["product.slug"]}.development.data.dir", nativeTestData)
            task.systemProperty("jna.library.path", debugLibraryFile.parentFile.absolutePath)
        }
        target.tasks.withType(JavaExec::class.java).configureEach { task ->
            task.dependsOn(buildDebug)
            task.systemProperty("${productCoordinates["product.slug"]}.development", "true")
            task.systemProperty("jna.library.path", debugLibraryFile.parentFile.absolutePath)
        }
        target.tasks.named("check") { task -> task.dependsOn(verifyGeneratedSources) }

        target.extensions.create("harvestCircleRustFfi", HarvestCircleRustFfiExtension::class.java).apply {
            nativeOsName.set(osName)
            nativeArchitecture.set(architecture)
            libraryName.set(nativeTarget.libraryName)
            jnaPlatformPrefix.set(nativeTarget.jnaPrefix)
            debugLibrary.set(debugLibraryFile)
            releaseLibrary.set(releaseLibraryFile)
            this.sourceCommit.set(sourceCommit)
            this.sourceDirty.set(sourceDirty)
            this.radrootsRevision.set(radrootsRevision)
            this.sourceDateEpoch.set(sourceDateEpoch)
        }
        check(baseline["product.coordinate_digest"] == productCoordinates.digest) {
            "FFI compatibility product-coordinate digest is stale"
        }
    }
}

private fun String.sha256(): String =
    MessageDigest
        .getInstance("SHA-256")
        .digest(toByteArray(Charsets.UTF_8))
        .joinToString("") { byte -> "%02x".format(byte) }
