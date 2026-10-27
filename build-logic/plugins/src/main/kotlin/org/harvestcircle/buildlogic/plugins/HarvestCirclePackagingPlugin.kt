package org.harvestcircle.buildlogic.plugins

import org.gradle.api.GradleException
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.gradle.api.plugins.ExtensionAware
import org.gradle.api.tasks.Delete
import org.gradle.jvm.tasks.Jar
import org.harvestcircle.buildlogic.contracts.FfiCompatibilityBaseline
import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.harvestcircle.buildlogic.plugins.tasks.VerifyDesktopBuildMetadataArtifact
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDistribution
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyLinuxPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyPackagedApplicationHealth
import org.jetbrains.compose.ComposeExtension
import org.jetbrains.compose.desktop.DesktopExtension
import org.jetbrains.compose.desktop.application.dsl.TargetFormat

public class HarvestCirclePackagingPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        require(target.plugins.hasPlugin("org.harvestcircle.build.desktop-app")) {
            "HarvestCircle packaging convention requires the desktop application convention"
        }
        require(target.plugins.hasPlugin("org.harvestcircle.build.rust-ffi")) {
            "HarvestCircle packaging convention requires the Rust FFI convention"
        }

        val catalog = target.extensions.getByType(VersionCatalogsExtension::class.java).named("libs")
        val coordinates =
            ProductCoordinates.load(
                target.rootProject.layout.projectDirectory.file("config/product/harvestcircle-v1.properties").asFile,
            )
        val baseline =
            FfiCompatibilityBaseline.load(
                target.rootProject.layout.projectDirectory
                    .file("core/compatibility/harvestcircle-ffi-v5.properties")
                    .asFile,
            )
        val rustFfi = target.extensions.getByType(HarvestCircleRustFfiExtension::class.java)
        val applicationName = coordinates["product.name"]
        val productSlug = coordinates["product.slug"]
        val bundleId = coordinates["desktop.bundle_id"]
        val nativeOsName = rustFfi.nativeOsName.get()
        val isMacOsHost = nativeOsName.lowercase().startsWith("mac")
        val isLinuxHost = nativeOsName.lowercase().startsWith("linux")
        if (!isMacOsHost && !isLinuxHost) {
            throw GradleException("Unsupported desktop package host: $nativeOsName")
        }
        val appVersion = target.version.toString()
        val installableVersion = baseline["package.version"]
        val macOsBuildVersion = "1"
        require(Regex("[1-9]\\d*(\\.\\d+){0,2}").matches(installableVersion)) {
            "Package version must satisfy the macOS jpackage contract"
        }
        val rustLibrary = rustFfi.releaseLibrary.get().asFile
        val expectedNativeEntry = "${rustFfi.jnaPlatformPrefix.get()}/${rustFfi.libraryName.get()}"
        val releaseResources = target.tasks.named("releaseNativeResourcesJar", Jar::class.java)
        val releaseRuntime = target.files(releaseResources.flatMap { it.archiveFile }).builtBy(releaseResources)
        val desktopJar = target.tasks.named("jar", Jar::class.java)

        val verifyMetadata =
            target.tasks.register(
                "verifyDesktopBuildMetadataArtifact",
                VerifyDesktopBuildMetadataArtifact::class.java,
            ) { task ->
                task.dependsOn(desktopJar)
                task.desktopJar.set(desktopJar.flatMap { it.archiveFile })
                task.expectedBuildEvidence.set(
                    listOf(
                        appVersion,
                        installableVersion,
                        target.gradle.gradleVersion,
                        System.getProperty("java.version"),
                        catalog.findVersion("kotlin").get().requiredVersion,
                        catalog.findVersion("compose").get().requiredVersion,
                    ),
                )
            }

        val compose = target.extensions.getByType(ComposeExtension::class.java)
        val desktop = (compose as ExtensionAware).extensions.getByType(DesktopExtension::class.java)
        desktop.application { application ->
            application.disableDefaultConfiguration()
            application.dependsOn(releaseResources.get())
            application.dependsOn(desktopJar.get())
            val applicationJar = desktopJar.flatMap { it.archiveFile }
            application.mainJar.set(applicationJar)
            application.fromFiles(
                applicationJar,
                target.configurations.getByName("runtimeClasspath"),
                releaseRuntime,
            )
            if (isMacOsHost) {
                application.jvmArgs +=
                    listOf(
                        "-Dapple.awt.application.name=$applicationName",
                        "-Dapple.awt.application.appearance=system",
                    )
            }
            application.nativeDistributions { distribution ->
                distribution.targetFormats(
                    when {
                        isMacOsHost -> TargetFormat.Dmg
                        else -> TargetFormat.Deb
                    },
                )
                distribution.packageName = applicationName
                distribution.packageVersion = installableVersion
                distribution.description = "$applicationName $appVersion"
                distribution.copyright = coordinates["copyright.notice"]
                distribution.vendor = coordinates["vendor.name"]
                distribution.macOS { mac ->
                    mac.bundleID = bundleId
                    mac.iconFile.set(target.file("src/main/resources/icons/$productSlug.icns"))
                    mac.packageName = applicationName
                    mac.dockName = applicationName
                    mac.packageBuildVersion = macOsBuildVersion
                }
            }
        }
        val cleanDistributable =
            target.tasks.register("cleanDistributableForPackaging", Delete::class.java) { task ->
                task.delete(target.layout.buildDirectory.dir("compose/binaries/main/app"))
            }
        target.tasks.matching { it.name == "createDistributable" }.configureEach {
            it.dependsOn(releaseResources, cleanDistributable)
        }

        val appDirectory =
            when {
                isMacOsHost -> target.layout.buildDirectory.dir("compose/binaries/main/app/$applicationName.app")
                else -> target.layout.buildDirectory.dir("compose/binaries/main/app/$applicationName")
            }
        val packagedExecutable =
            appDirectory.map { directory ->
                when {
                    isMacOsHost -> directory.file("Contents/MacOS/$applicationName")
                    else -> directory.file("bin/$applicationName")
                }
            }
        val verifyHealth =
            target.tasks.register(
                "verifyPackagedApplicationHealth",
                VerifyPackagedApplicationHealth::class.java,
            ) { task ->
                task.dependsOn("createDistributable")
                task.executable.set(packagedExecutable)
                task.developmentDataEnvironment.set(coordinates["environment.prefix"] + "DEVELOPMENT_DATA_DIR")
                task.timeoutSeconds.set(10_800L)
                task.readyEvidence.set("HARVESTCIRCLE_HEALTH_READY")
                task.closedEvidence.set("HARVESTCIRCLE_HEALTH_CLOSED")
            }

        val hostPackage =
            when {
                isMacOsHost -> {
                    val verifyDistribution =
                        target.tasks.register("verifyMacOsDistribution", VerifyMacOsDistribution::class.java) { task ->
                            task.dependsOn("createDistributable")
                            task.appDirectory.set(appDirectory)
                            task.releaseLibrary.set(rustLibrary)
                            task.iconSource.set(target.layout.projectDirectory.file("src/main/resources/icons/$productSlug.icns"))
                            task.expectedBundleId.set(bundleId)
                            task.expectedPackageVersion.set(installableVersion)
                            task.expectedBuildVersion.set(macOsBuildVersion)
                            task.expectedNativeEntry.set(expectedNativeEntry)
                        }
                    target.tasks.register("verifyMacOsPackage", VerifyMacOsPackage::class.java) { task ->
                        task.dependsOn("packageDmg", verifyDistribution)
                        task.packageDirectory.set(target.layout.buildDirectory.dir("compose/binaries/main/dmg"))
                        task.expectedFileName.set("$applicationName-$installableVersion.dmg")
                    }
                }
                else ->
                    target.tasks.register("verifyLinuxPackage", VerifyLinuxPackage::class.java) { task ->
                        task.dependsOn("packageDeb", "verifyReleaseNativeLibrary")
                        task.packageDirectory.set(target.layout.buildDirectory.dir("compose/binaries/main/deb"))
                        task.releaseLibrary.set(rustLibrary)
                        task.expectedVersion.set(installableVersion)
                        task.expectedNativeEntry.set(expectedNativeEntry)
                    }
            }
        target.tasks.register("verifyHostPackage") { it.dependsOn(hostPackage, verifyHealth) }

        target.tasks.register("sourceReadiness") { task ->
            task.dependsOn(
                ":verifyProductCoordinates",
                ":app:shared:check",
                "check",
                "verifyUniFfiBindings",
            )
        }
        target.tasks.named("check") { it.dependsOn(verifyMetadata) }
    }
}
