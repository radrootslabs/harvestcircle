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
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDeveloperIdSignature
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDistribution
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsNotarization
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyNativeInstallPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyPackagedApplicationHealth
import org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseBuildProvenance
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
                    .file("core/compatibility/harvestcircle-ffi-v4.properties")
                    .asFile,
            )
        val rustFfi = target.extensions.getByType(HarvestCircleRustFfiExtension::class.java)
        val applicationName = coordinates["product.name"]
        val productSlug = coordinates["product.slug"]
        val bundleId = coordinates["desktop.bundle_id"]
        val nativeOsName = rustFfi.nativeOsName.get()
        val nativeArchitecture = rustFfi.nativeArchitecture.get()
        val isMacOsHost = nativeOsName.lowercase().startsWith("mac")
        val isLinuxHost = nativeOsName.lowercase().startsWith("linux")
        val isWindowsHost = nativeOsName.lowercase().startsWith("windows")
        val isGovernedMacOsTarget =
            isMacOsHost && nativeArchitecture.lowercase() == coordinates["platform.macos.architecture"]
        if (!isMacOsHost && !isLinuxHost && !isWindowsHost) {
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
                        isLinuxHost -> TargetFormat.Deb
                        else -> TargetFormat.Msi
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
                    isLinuxHost -> directory.file("bin/$applicationName")
                    else -> directory.file("$applicationName.exe")
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
                task.timeoutSeconds.set(120L)
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
                isLinuxHost ->
                    target.tasks.register("verifyLinuxPackage", VerifyNativeInstallPackage::class.java) { task ->
                        task.dependsOn("packageDeb", "verifyReleaseNativeLibrary")
                        task.packageDirectory.set(target.layout.buildDirectory.dir("compose/binaries/main/deb"))
                        task.releaseLibrary.set(rustLibrary)
                        task.packageExtension.set("deb")
                        task.expectedVersion.set(installableVersion)
                        task.expectedNativeEntry.set(expectedNativeEntry)
                        task.hostFamily.set("linux")
                    }
                else ->
                    target.tasks.register("verifyWindowsPackage", VerifyNativeInstallPackage::class.java) { task ->
                        task.dependsOn("packageMsi", "verifyReleaseNativeLibrary")
                        task.packageDirectory.set(target.layout.buildDirectory.dir("compose/binaries/main/msi"))
                        task.releaseLibrary.set(rustLibrary)
                        task.packageExtension.set("msi")
                        task.expectedVersion.set(installableVersion)
                        task.expectedNativeEntry.set(expectedNativeEntry)
                        task.hostFamily.set("windows")
                    }
            }
        val verifyHostPackage = target.tasks.register("verifyHostPackage") { it.dependsOn(hostPackage, verifyHealth) }

        val verifySignature =
            target.tasks.register(
                "verifyMacOsDeveloperIdSignature",
                VerifyMacOsDeveloperIdSignature::class.java,
            ) { task ->
                task.dependsOn(hostPackage)
                task.appDirectory.set(appDirectory)
                task.onlyIf { isMacOsHost }
            }
        val verifyNotarization =
            target.tasks.register("verifyMacOsNotarization", VerifyMacOsNotarization::class.java) { task ->
                task.dependsOn(hostPackage)
                task.diskImage.set(
                    target.layout.buildDirectory.file(
                        "compose/binaries/main/dmg/$applicationName-$installableVersion.dmg",
                    ),
                )
                task.onlyIf { isMacOsHost }
            }
        val verifyProvenance =
            target.tasks.register(
                "verifyReleaseBuildProvenance",
                VerifyReleaseBuildProvenance::class.java,
            ) { task ->
                task.sourceCommit.set(rustFfi.sourceCommit)
                task.sourceDirty.set(rustFfi.sourceDirty)
                task.radrootsRevision.set(rustFfi.radrootsRevision)
                task.sourceDateEpoch.set(rustFfi.sourceDateEpoch)
            }
        val sourceReadiness =
            target.tasks.register("sourceReadiness") { task ->
                task.dependsOn(
                    ":verifyProductCoordinates",
                    ":verifyHarvestCircleArtifactContract",
                    ":verifyVerificationLanes",
                    ":app:shared:check",
                    "check",
                    "verifyUniFfiBindings",
                )
            }
        val packageReadiness =
            target.tasks.register("packageReadiness") { task ->
                task.dependsOn(verifyHostPackage, verifyProvenance, verifyMetadata)
            }
        val signingReadiness = target.tasks.register("signingReadiness") { it.dependsOn(verifySignature) }
        val notarizationReadiness = target.tasks.register("notarizationReadiness") { it.dependsOn(verifyNotarization) }
        val unsignedReleaseReadiness = target.tasks.register("unsignedReleaseReadiness") { task ->
            if (!isGovernedMacOsTarget) {
                throw GradleException(
                    "Unsigned release contract requires macOS/aarch64, not $nativeOsName/$nativeArchitecture",
                )
            }
            task.dependsOn(
                "checkLicense",
                sourceReadiness,
                packageReadiness,
                "packageDmg",
                "verifyMacOsPackage",
            )
        }
        target.tasks.register("releaseReadiness") { task ->
            task.dependsOn(
                "dependencyCheckAnalyze",
                unsignedReleaseReadiness,
                signingReadiness,
                notarizationReadiness,
            )
        }
        target.tasks.named("check") { it.dependsOn(verifyMetadata) }
    }
}
