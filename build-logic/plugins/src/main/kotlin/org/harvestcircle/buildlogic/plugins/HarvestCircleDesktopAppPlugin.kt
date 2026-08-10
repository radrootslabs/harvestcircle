package org.harvestcircle.buildlogic.plugins

import com.github.jk1.license.LicenseReportExtension
import com.github.jk1.license.filter.SpdxLicenseBundleNormalizer
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.plugins.ExtensionAware
import org.gradle.api.tasks.SourceSetContainer
import org.gradle.api.tasks.testing.Test
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.harvestcircle.buildlogic.contracts.resolveNativeTarget
import org.harvestcircle.buildlogic.plugins.tasks.DesktopBuildMetadataTask
import org.harvestcircle.buildlogic.plugins.tasks.GenerateDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestInventory
import org.jetbrains.compose.ComposeExtension
import org.jetbrains.compose.desktop.DesktopExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.dsl.KotlinJvmProjectExtension
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile
import org.owasp.dependencycheck.gradle.extension.DependencyCheckExtension

public abstract class HarvestCircleDesktopAppExtension {
    public abstract val generatedKotlinSources: ConfigurableFileCollection
}

public class HarvestCircleDesktopAppPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target != target.rootProject) {
            "HarvestCircle desktop application convention must be applied to a subproject"
        }
        target.pluginManager.apply("org.jetbrains.kotlin.jvm")
        target.pluginManager.apply("org.jetbrains.compose")
        target.pluginManager.apply("org.jetbrains.kotlin.plugin.compose")
        target.pluginManager.apply("dev.detekt")
        target.pluginManager.apply("org.jlleitschuh.gradle.ktlint")
        target.pluginManager.apply("com.github.jk1.dependency-license-report")
        target.pluginManager.apply("org.owasp.dependencycheck")

        val catalog = target.extensions.getByType(VersionCatalogsExtension::class.java).named("libs")
        val coordinatesFile = target.rootProject.layout.projectDirectory.file("config/product/harvestcircle-v1.properties")
        val productCoordinates = ProductCoordinates.load(coordinatesFile.asFile)
        val cargoManifest = target.rootProject.layout.projectDirectory.file("core/Cargo.toml").asFile
        val baseline =
            FfiCompatibilityBaseline.load(
                target.rootProject.layout.projectDirectory
                    .file("core/compatibility/harvestcircle-ffi-v4.properties")
                    .asFile,
            )
        val appVersion = workspacePackageValue(cargoManifest.readText(), "version")
        check(baseline["product.version"] == appVersion) {
            "FFI compatibility product version must match the Cargo workspace version"
        }
        check(baseline["product.coordinate_digest"] == productCoordinates.digest) {
            "FFI compatibility product-coordinate digest is stale"
        }

        target.group = productCoordinates["desktop.application_id"]
        target.version = appVersion
        target.providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
            target.layout.buildDirectory.set(target.file(outputRoot).resolve("app-desktop"))
        }

        configureKtlint(target)
        configureQuality(target)
        configureKotlin(target)
        configureDependencies(target, catalog)
        configureIntegrationContract(target)
        configureTests(target)
        configureMetadata(target, catalog, appVersion, baseline["package.version"])
        configureCompose(target, productCoordinates["desktop.main_class"])
    }

    private fun configureKtlint(target: Project) {
        val ktlintExtension: Any = target.extensions.getByName("ktlint")
        @Suppress("UNCHECKED_CAST")
        val property =
            ktlintExtension.javaClass
                .getMethod("getAdditionalEditorconfig")
                .invoke(ktlintExtension) as org.gradle.api.provider.MapProperty<String, String>
        property.set(mapOf("ktlint_function_naming_ignore_when_annotated_with" to "Composable"))
    }

    private fun configureQuality(target: Project) {
        target.extensions.configure(LicenseReportExtension::class.java) { extension ->
            extension.projects = arrayOf(target)
            extension.configurations = arrayOf("runtimeClasspath")
            extension.excludeGroups = arrayOf("harvestcircle.app")
            extension.filters = arrayOf(SpdxLicenseBundleNormalizer())
            extension.allowedLicensesFile =
                target.rootProject.layout.projectDirectory.file("config/licenses/allowed-licenses.json")
        }
        target.extensions.configure(DependencyCheckExtension::class.java) { extension ->
            extension.failBuildOnCVSS.set(0.0F)
            extension.failOnError.set(true)
            extension.formats.set(listOf("HTML", "JSON"))
            extension.scanConfigurations.set(listOf("runtimeClasspath"))
            extension.skipTestGroups.set(true)
            target.providers.environmentVariable("NVD_API_KEY").orNull?.takeIf(String::isNotBlank)?.let {
                extension.nvd.apiKey.set(it)
            }
        }
        target.tasks.matching { it.name.startsWith("dependencyCheck") }.configureEach {
            it.notCompatibleWithConfigurationCache("Advisory data and environment-only credentials must not be cached")
        }
    }

    private fun configureKotlin(target: Project) {
        target.extensions.configure(KotlinJvmProjectExtension::class.java) { kotlin ->
            kotlin.compilerOptions.jvmTarget.set(JvmTarget.JVM_21)
            kotlin.jvmToolchain(21)
        }
    }

    private fun configureDependencies(
        target: Project,
        catalog: org.gradle.api.artifacts.VersionCatalog,
    ) {
        target.dependencies.add("implementation", target.project(":app:shared"))
        val composeVersion = catalog.findVersion("compose").get().requiredVersion
        val osName = target.providers.gradleProperty("nativeOs").getOrElse(System.getProperty("os.name"))
        val architecture = target.providers.gradleProperty("nativeArch").getOrElse(System.getProperty("os.arch"))
        val suffix =
            when (resolveNativeTarget(osName, architecture, "harvestcircle_ffi").jnaPrefix) {
                "darwin-aarch64" -> "macos-arm64"
                "darwin-x86-64" -> "macos-x64"
                "linux-aarch64" -> "linux-arm64"
                "linux-x86-64" -> "linux-x64"
                "win32-aarch64" -> "windows-arm64"
                "win32-x86-64" -> "windows-x64"
                else -> error("Unsupported Compose desktop host: $osName/$architecture")
            }
        target.dependencies.add("implementation", "org.jetbrains.compose.desktop:desktop-jvm-$suffix:$composeVersion")
        target.dependencies.add("implementation", catalog.findLibrary("compose-foundation").get())
        target.dependencies.add("implementation", catalog.findLibrary("jna").get())
        target.dependencies.add("implementation", catalog.findLibrary("kotlinx-coroutines-core").get())
        target.dependencies.add(
            "testImplementation",
            "org.jetbrains.kotlin:kotlin-test-junit:${catalog.findVersion("kotlin").get().requiredVersion}",
        )
        target.dependencies.add("testImplementation", catalog.findLibrary("compose-ui-test-junit4").get())
        target.dependencies.add("testImplementation", catalog.findLibrary("kotlinx-coroutines-test").get())
    }

    private fun configureIntegrationContract(target: Project) {
        val sourceSets = target.extensions.getByType(SourceSetContainer::class.java)
        sourceSets.maybeCreate("integrationTest")
        target.configurations.named("integrationTestImplementation") {
            it.extendsFrom(target.configurations.getByName("testImplementation"))
        }
        target.configurations.named("integrationTestRuntimeOnly") {
            it.extendsFrom(target.configurations.getByName("testRuntimeOnly"))
        }
    }

    private fun configureTests(target: Project) {
        val sourceRoot = target.providers.gradleProperty("testInventoryRoot").orElse("src/test/kotlin")
        val inventory =
            target.tasks.register("verifyTestInventory", VerifyTestInventory::class.java) { task ->
                task.testFiles.from(target.fileTree(sourceRoot.get()) { it.include("**/*Test.kt") })
                task.sourceRoot.set(sourceRoot)
            }
        target.tasks.withType(Test::class.java).configureEach { task ->
            task.dependsOn(inventory)
            task.failOnNoDiscoveredTests.set(true)
        }
    }

    private fun configureMetadata(
        target: Project,
        catalog: org.gradle.api.artifacts.VersionCatalog,
        appVersion: String,
        packageVersion: String,
    ) {
        val generatedRoot = target.layout.buildDirectory.dir("generated/compatibility/kotlin")
        val metadataFile =
            generatedRoot.map { it.file("org/harvestcircle/application/generated/DesktopBuildMetadata.kt") }
        fun DesktopBuildMetadataTask.configureValues() {
            productVersion.set(appVersion)
            distributionPackageVersion.set(packageVersion)
            gradleToolchain.set(target.gradle.gradleVersion)
            javaToolchain.set(System.getProperty("java.version"))
            kotlinToolchain.set(catalog.findVersion("kotlin").get().requiredVersion)
            composeMultiplatformVersion.set(catalog.findVersion("compose").get().requiredVersion)
        }
        val generate =
            target.tasks.register("generateDesktopBuildMetadata", GenerateDesktopBuildMetadata::class.java) { task ->
                task.configureValues()
                task.outputFile.set(metadataFile)
            }
        val verify =
            target.tasks.register(
                "verifyGeneratedDesktopBuildMetadata",
                VerifyGeneratedDesktopBuildMetadata::class.java,
            ) { task ->
                task.dependsOn(generate)
                task.configureValues()
                task.generatedFile.set(metadataFile)
            }
        val extension = target.extensions.create("harvestCircleDesktop", HarvestCircleDesktopAppExtension::class.java)
        extension.generatedKotlinSources.from(generatedRoot)
        target.tasks.named("compileKotlin", KotlinCompile::class.java) { task ->
            task.dependsOn(generate)
            task.source(extension.generatedKotlinSources)
        }
        target.tasks.named("check") { task ->
            task.dependsOn(target.rootProject.tasks.named("verifyProductCoordinates"))
            task.dependsOn(target.rootProject.tasks.named("verifyProductCoordinateConsumers"))
            task.dependsOn(verify)
        }
    }

    private fun configureCompose(
        target: Project,
        mainClass: String,
    ) {
        val compose = target.extensions.getByType(ComposeExtension::class.java)
        val desktop = (compose as ExtensionAware).extensions.getByType(DesktopExtension::class.java)
        desktop.application { application -> application.mainClass = mainClass }
    }

    private fun workspacePackageValue(
        manifest: String,
        key: String,
    ): String {
        val workspacePackage = manifest.substringAfter("[workspace.package]", "").substringBefore("\n[")
        require(workspacePackage.isNotBlank()) { "Cargo manifest is missing [workspace.package]" }
        return Regex("(?m)^${Regex.escape(key)}\\s*=\\s*\"([^\"]+)\"\\s*$")
            .find(workspacePackage)
            ?.groupValues
            ?.get(1)
            ?: error("Cargo workspace package metadata is missing $key")
    }
}
