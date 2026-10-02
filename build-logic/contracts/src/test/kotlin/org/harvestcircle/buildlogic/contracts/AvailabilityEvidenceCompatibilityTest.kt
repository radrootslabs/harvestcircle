package org.harvestcircle.buildlogic.contracts

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails

public class AvailabilityEvidenceCompatibilityTest {
    @Test
    public fun allocatedEvidenceSchemaAndMinorAreAccepted() {
        val baseline = FfiCompatibilityBaseline.parse(fixture)

        assertEquals("4", baseline["contract.major"])
        assertEquals("5", baseline["contract.minor"])
        assertEquals("1", baseline["snapshot.schema"])
        assertEquals("1", baseline["storage.schema.minimum"])
        assertEquals("3", baseline["storage.schema.current"])
        assertEquals("a".repeat(64), baseline["contract.hash"])
    }

    @Test
    public fun previousCurrentSchemaOrMinorAreRejected() {
        listOf(
            fixture.replace("contract.minor=5", "contract.minor=4"),
            fixture.replace("storage.schema.current=3", "storage.schema.current=2"),
            fixture.replace("contract.minor=5", "contract.minor=4").replace("storage.schema.current=3", "storage.schema.current=2"),
            fixture.replace("contract.minor=5", "contract.minor=6"),
            fixture.replace("storage.schema.current=3", "storage.schema.current=4"),
            fixture.replace("contract.major=4", "contract.major=5"),
            fixture.replace("snapshot.schema=1", "snapshot.schema=2"),
            fixture.replace("storage.schema.minimum=1", "storage.schema.minimum=2"),
        ).forEach { incompatible ->
            assertFails { FfiCompatibilityBaseline.parse(incompatible) }
        }
    }

    // Synthetic parser inputs carry no native artifact or actual contract-hash claim.
    private val fixture =
        """
        schema=harvestcircle.ffi.v4
        contract.id=harvestcircle-desktop-ffi-v4
        contract.major=4
        contract.minor=5
        contract.hash=${"a".repeat(64)}
        product.coordinate_digest=${"b".repeat(64)}
        snapshot.schema=1
        storage.schema.minimum=1
        storage.schema.current=3
        product.version=0.1.0-alpha
        package.version=1.0.0
        source.provenance_digest=${"c".repeat(64)}
        source.foundation_baseline=${"d".repeat(40)}
        """.trimIndent() + "\n"
}
