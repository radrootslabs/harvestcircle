package org.harvestcircle.buildlogic.plugins

import org.gradle.api.tasks.CacheableTask
import org.gradle.work.DisableCachingByDefault
import org.harvestcircle.buildlogic.plugins.tasks.GenerateCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.GenerateDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.StageReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyDesktopBuildMetadataArtifact
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedCompatibilityExpectations
import org.harvestcircle.buildlogic.plugins.tasks.VerifyGeneratedDesktopBuildMetadata
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsDistribution
import org.harvestcircle.buildlogic.plugins.tasks.VerifyMacOsPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyLinuxPackage
import org.harvestcircle.buildlogic.plugins.tasks.VerifyPackagedApplicationHealth
import org.harvestcircle.buildlogic.plugins.tasks.VerifyProductCoordinates
import org.harvestcircle.buildlogic.plugins.tasks.VerifyReleaseNativeLibrary
import org.harvestcircle.buildlogic.plugins.tasks.VerifySharedBoundary
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestBridgeIsolation
import org.harvestcircle.buildlogic.plugins.tasks.VerifyTestInventory
import org.harvestcircle.buildlogic.plugins.tasks.VerifyUniFfiBindings
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
            VerifyProductCoordinates::class.java,
            VerifySharedBoundary::class.java,
            VerifyTestInventory::class.java,
            VerifyGeneratedCompatibilityExpectations::class.java,
            VerifyUniFfiBindings::class.java,
            VerifyReleaseNativeLibrary::class.java,
            VerifyTestBridgeIsolation::class.java,
            VerifyDesktopBuildMetadataArtifact::class.java,
            VerifyMacOsDistribution::class.java,
            VerifyMacOsPackage::class.java,
            VerifyLinuxPackage::class.java,
            VerifyPackagedApplicationHealth::class.java,
        ).forEach { taskType ->
            assertNotNull(taskType.getAnnotation(DisableCachingByDefault::class.java), taskType.name)
        }
    }
}
