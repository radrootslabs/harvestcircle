package org.harvestcircle.buildlogic.plugins

import org.gradle.api.tasks.CacheableTask
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.plugins.tasks.GenerateCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.GenerateDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.StageReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyDesktopBuildMetadataArtifact
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.VerifyHarvestCircleArtifactContract
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDeveloperIdSignature
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDistribution
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsNotarization
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyNativeInstallPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyPackagedApplicationHealth
import org.harvestcircle.buildlogic.plugins.tasks.VerifyProductCoordinates
import org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseBuildProvenance
import org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifySharedBoundary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestBridgeIsolation
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestInventory
import org.harvestcircle.buildlogic.plugins.tasks.VerifyUniFfiBindings
import org.harvestcircle.buildlogic.plugins.tasks.VerifyVerificationLanes
import kotlin.test.Test
import kotlin.test.assertNotNull

class TaskPolicyTest {
    @Test
    fun onlyDeterministicProducersAreCacheable() {
        listOf(
            GenerateDesktopBuildMetadata::class.java,
            GenerateCompatibilityExpectations::class.java,
            StageReleaseNativeLibrary::class.java,
        ).forEach { taskType -> assertNotNull(taskType.getAnnotation(CacheableTask::class.java), taskType.name) }

        listOf(
            VerifyGeneratedDesktopBuildMetadata::class.java,
            VerifyHarvestCircleArtifactContract::class.java,
            VerifyProductCoordinates::class.java,
            VerifyVerificationLanes::class.java,
            VerifySharedBoundary::class.java,
            VerifyTestInventory::class.java,
            VerifyGeneratedCompatibilityExpectations::class.java,
            VerifyUniFfiBindings::class.java,
            VerifyReleaseNativeLibrary::class.java,
            VerifyTestBridgeIsolation::class.java,
            VerifyDesktopBuildMetadataArtifact::class.java,
            VerifyMacOsDistribution::class.java,
            VerifyMacOsPackage::class.java,
            VerifyNativeInstallPackage::class.java,
            VerifyPackagedApplicationHealth::class.java,
            VerifyMacOsDeveloperIdSignature::class.java,
            VerifyMacOsNotarization::class.java,
            VerifyReleaseBuildProvenance::class.java,
        ).forEach { taskType ->
            assertNotNull(taskType.getAnnotation(DisableCachingByDefault::class.java), taskType.name)
        }
    }
}
