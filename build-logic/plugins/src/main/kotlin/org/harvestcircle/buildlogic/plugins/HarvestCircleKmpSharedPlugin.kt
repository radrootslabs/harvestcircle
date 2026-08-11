package org.harvestcircle.buildlogic.plugins

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.harvestcircle.buildlogic.contracts.resolveNativeTarget
import org.harvestcircle.buildlogic.plugins.tasks.VerifySharedBoundary
import org.jlleitschuh.gradle.ktlint.KtlintExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.dsl.KotlinMultiplatformExtension
import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType

public class HarvestCircleKmpSharedPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        target.pluginManager.apply("org.jetbrains.kotlin.multiplatform")
        target.pluginManager.apply("org.jetbrains.compose")
        target.pluginManager.apply("org.jetbrains.kotlin.plugin.compose")
        target.pluginManager.apply("dev.detekt")
        target.pluginManager.apply("org.jlleitschuh.gradle.ktlint")

        val catalog = target.extensions.getByType(VersionCatalogsExtension::class.java).named("libs")
        val kotlinVersion = catalog.findVersion("kotlin").get().requiredVersion
        val composeVersion = catalog.findVersion("compose").get().requiredVersion

        target.extensions.configure(KtlintExtension::class.java) { extension ->
            extension.additionalEditorconfig.set(
                mapOf("ktlint_function_naming_ignore_when_annotated_with" to "Composable"),
            )
        }
        target.extensions.configure(KotlinMultiplatformExtension::class.java) { kotlin ->
            kotlin.jvm("desktop") { jvm ->
                jvm.compilerOptions.jvmTarget.set(JvmTarget.JVM_21)
            }
            kotlin.jvmToolchain(21)
            check(
                kotlin.targets
                    .filter { it.platformType != KotlinPlatformType.common }
                    .map { it.name } == listOf("desktop"),
            ) {
                "HarvestCircle shared must declare exactly one KMP platform target named desktop"
            }
        }

        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-foundation").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-runtime").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("compose-ui").get())
        target.dependencies.add("commonMainImplementation", catalog.findLibrary("kotlinx-coroutines-core").get())
        target.dependencies.add("commonTestImplementation", "org.jetbrains.kotlin:kotlin-test:$kotlinVersion")
        target.dependencies.add("commonTestImplementation", catalog.findLibrary("kotlinx-coroutines-test").get())
        target.dependencies.add("desktopTestImplementation", composeDesktopDependency(target, composeVersion))
        target.dependencies.add("desktopTestImplementation", "org.jetbrains.kotlin:kotlin-test-junit:$kotlinVersion")
        target.dependencies.add("desktopTestImplementation", catalog.findLibrary("compose-ui-test-junit4").get())

        target.providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
            target.layout.buildDirectory.set(target.file(outputRoot).resolve("app-shared"))
        }

        val verifySharedBoundary =
            target.tasks.register("verifySharedBoundary", VerifySharedBoundary::class.java) { task ->
                task.commonSources.from(
                    target.fileTree("src/commonMain/kotlin") { tree -> tree.include("**/*.kt") },
                )
            }
        target.tasks.named("check") { task -> task.dependsOn(verifySharedBoundary) }
    }

    private fun composeDesktopDependency(
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
}
