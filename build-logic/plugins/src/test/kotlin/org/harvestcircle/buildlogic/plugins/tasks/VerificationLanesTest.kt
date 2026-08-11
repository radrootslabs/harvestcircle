package org.harvestcircle.buildlogic.plugins.tasks

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails

class VerificationLanesTest {
    @Test
    fun policyRequiresTheExactLeastPrivilegeLaneMap() {
        val parsed = VerificationLanes.parse(policy, "HARVESTCIRCLE_")

        assertEquals(24, parsed.size)
        assertFails { VerificationLanes.parse(policy + "source.workflow=forbidden\n", "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replaceFirst("schema=", "schema"), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("schema=harvestcircle.verification-lanes.v2\n", ""), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy + "schema=harvestcircle.verification-lanes.v2\n", "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("source.credentials=none", "source.credentials=all"), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("release.mode=governed", "release.mode=standalone"), "HARVESTCIRCLE_") }
        assertFails {
            VerificationLanes.parse(
                policy.replace("HARVESTCIRCLE_BUILD_SOURCE_COMMIT", "BUILD_SOURCE_COMMIT"),
                "HARVESTCIRCLE_",
            )
        }
    }

    private val policy =
        """
        schema=harvestcircle.verification-lanes.v2
        orchestration=explicit-make-modes
        source.standalone.command=make source-check
        source.governed.command=make governed-source-check
        source.credentials=none
        integration.standalone.command=make integration-check
        integration.governed.command=make governed-integration-check
        integration.credentials=none
        package.standalone.command=make host-package-check
        package.governed.command=make governed-package-check
        package.runners=linux,macos,windows
        package.credentials=none
        provenance.commit=HARVESTCIRCLE_BUILD_SOURCE_COMMIT
        provenance.dirty=HARVESTCIRCLE_BUILD_SOURCE_DIRTY
        provenance.radroots=HARVESTCIRCLE_BUILD_RADROOTS_REVISION
        provenance.epoch=SOURCE_DATE_EPOCH
        signing.command=make signing-check
        signing.runner=macos
        signing.credentials=signing
        notarization.command=make notarization-check
        notarization.runner=macos
        notarization.credentials=notarization
        release.command=make release-check
        release.mode=governed
        """.trimIndent() + "\n"
}
