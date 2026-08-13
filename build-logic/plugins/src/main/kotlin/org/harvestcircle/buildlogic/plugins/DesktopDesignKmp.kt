package org.harvestcircle.buildlogic.plugins

import com.github.jk1.license.LicenseReportExtension
import com.github.jk1.license.filter.SpdxLicenseBundleNormalizer
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalog
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.harvestcircle.buildlogic.contracts.resolveNativeTarget
import org.jlleitschuh.gradle.ktlint.KtlintExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.dsl.KotlinMultiplatformExtension
import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType
import org.owasp.dependencycheck.gradle.extension.DependencyCheckExtension

internal fun Project.applyDesktopDesignKmp() {
    pluginManager.apply("org.jetbrains.kotlin.multiplatform")
    pluginManager.apply("org.jetbrains.compose")
    pluginManager.apply("org.jetbrains.kotlin.plugin.compose")
    pluginManager.apply("dev.detekt")
    pluginManager.apply("org.jlleitschuh.gradle.ktlint")
    pluginManager.apply("com.github.jk1.dependency-license-report")
    pluginManager.apply("org.owasp.dependencycheck")

    extensions.configure(KtlintExtension::class.java) { extension ->
        extension.additionalEditorconfig.set(
            mapOf("ktlint_function_naming_ignore_when_annotated_with" to "Composable"),
        )
        extension.filter { filter ->
            filter.exclude { element ->
                element.file.invariantSeparatorsPath.contains("/build/generated/")
            }
        }
    }
    extensions.configure(KotlinMultiplatformExtension::class.java) { kotlin ->
        kotlin.jvm("desktop") { jvm ->
            jvm.compilerOptions.jvmTarget.set(JvmTarget.JVM_21)
        }
        kotlin.jvmToolchain(21)
        check(
            kotlin.targets
                .filter { it.platformType != KotlinPlatformType.common }
                .map { it.name } == listOf("desktop"),
        ) {
            "$path must declare exactly one KMP platform target named desktop"
        }
    }
    extensions.configure(LicenseReportExtension::class.java) { extension ->
        extension.projects = arrayOf(this@applyDesktopDesignKmp)
        extension.configurations = arrayOf("desktopRuntimeClasspath")
        extension.excludeGroups = arrayOf("harvestcircle.app")
        extension.filters = arrayOf(SpdxLicenseBundleNormalizer())
        extension.allowedLicensesFile =
            rootProject.layout.projectDirectory.file("config/licenses/allowed-licenses.json")
    }
    extensions.configure(DependencyCheckExtension::class.java) { extension ->
        extension.failBuildOnCVSS.set(0.0F)
        extension.failOnError.set(true)
        extension.formats.set(listOf("HTML", "JSON"))
        extension.scanConfigurations.set(listOf("desktopRuntimeClasspath"))
        extension.skipTestGroups.set(true)
        providers.environmentVariable("NVD_API_KEY").orNull?.takeIf(String::isNotBlank)?.let {
            extension.nvd.apiKey.set(it)
        }
    }
    tasks.matching { it.name.startsWith("dependencyCheck") }.configureEach {
        it.notCompatibleWithConfigurationCache(
            "Advisory data and environment-only credentials must not be cached",
        )
    }
}

internal fun Project.versionCatalog(): VersionCatalog =
    extensions.getByType(VersionCatalogsExtension::class.java).named("libs")

internal fun Project.routeDesignBuildDirectory(directoryName: String) {
    providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
        layout.buildDirectory.set(file(outputRoot).resolve(directoryName))
    }
}

internal fun composeDesktopDependency(
    target: Project,
    composeVersion: String,
): String {
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
    return "org.jetbrains.compose.desktop:desktop-jvm-$suffix:$composeVersion"
}
