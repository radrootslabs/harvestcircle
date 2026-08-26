package org.harvestcircle.buildlogic.plugins.tasks

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails

class VerificationLanesTest {
    @Test
    fun policyRequiresTheExactLeastPrivilegeLaneMap() {
        val parsed = VerificationLanes.parse(policy, "HARVESTCIRCLE_")

        assertEquals(25, parsed.size)
        assertFails { VerificationLanes.parse(policy + "source.workflow=forbidden\n", "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replaceFirst("schema=", "schema"), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("schema=harvestcircle.verification-lanes.v3\n", ""), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy + "schema=harvestcircle.verification-lanes.v3\n", "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("source.credentials=none", "source.credentials=all"), "HARVESTCIRCLE_") }
        assertFails { VerificationLanes.parse(policy.replace("release.state=deferred-unclaimed", "release.state=passing"), "HARVESTCIRCLE_") }
        assertFails {
            VerificationLanes.parse(
                policy.replace("development.runners=macos-aarch64,linux-x86_64", "development.runners=windows"),
                "HARVESTCIRCLE_",
            )
        }
        assertFails {
            VerificationLanes.parse(
                policy.replace("release.network_advisories=deferred", "release.network_advisories=required"),
                "HARVESTCIRCLE_",
            )
        }
        assertFails {
            VerificationLanes.parse(
                policy.replace("HARVESTCIRCLE_BUILD_SOURCE_COMMIT", "BUILD_SOURCE_COMMIT"),
                "HARVESTCIRCLE_",
            )
        }

        VerificationLanes.verifyDevelopmentMakefile(makefile)
        assertFails {
            VerificationLanes.verifyDevelopmentMakefile(
                makefile.replace(
                    "development-check: development-provenance-check source-check integration-check",
                    "development-check: development-provenance-check source-check integration-check release-check",
                ),
            )
        }
    }

    private val policy =
        """
        schema=harvestcircle.verification-lanes.v3
        orchestration=explicit-make-modes
        source.standalone.command=make source-check
        source.governed.command=make governed-source-check
        source.credentials=none
        integration.standalone.command=make integration-check
        integration.governed.command=make governed-integration-check
        integration.credentials=none
        development.standalone.command=make development-check
        development.governed.command=make governed-development-check
        development.macos_aarch64.command=make governed-development-check
        development.linux_x86_64.command=make governed-linux-x86_64-development-check
        development.runners=macos-aarch64,linux-x86_64
        development.credentials=none
        provenance.commit=HARVESTCIRCLE_BUILD_SOURCE_COMMIT
        provenance.dirty=HARVESTCIRCLE_BUILD_SOURCE_DIRTY
        provenance.radroots=HARVESTCIRCLE_BUILD_RADROOTS_REVISION
        provenance.epoch=SOURCE_DATE_EPOCH
        release.state=deferred-unclaimed
        release.activation=declared-release-candidate-and-fresh-authority
        release.network_advisories=deferred
        release.packages=deferred
        release.evidence=deferred
        release.signing=deferred
        release.nix_oci=deferred
        """.trimIndent() + "\n"

    private val makefile =
        """
        override CARGO := cargo +1.97.1

        source-check: build-logic-check check bindings api-check licenses dev-check

        integration-check: build-logic-check check

        development-check: export HARVESTCIRCLE_BUILD_SOURCE_COMMIT = abcdef
        development-check: export HARVESTCIRCLE_BUILD_SOURCE_DIRTY = false

        development-check: development-provenance-check source-check integration-check

        governed-development-check:

        governed-linux-x86_64-development-check: governed-doctor
        """.trimIndent() + "\n"
}
