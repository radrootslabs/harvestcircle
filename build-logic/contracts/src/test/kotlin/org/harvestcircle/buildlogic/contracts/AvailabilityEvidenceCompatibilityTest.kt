package org.harvestcircle.buildlogic.contracts

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails

public class AvailabilityEvidenceCompatibilityTest {
    @Test
    public fun allocatedEvidenceSchemaAndMinorAreAccepted() {
        val baseline = FfiCompatibilityBaseline.parse(fixture)

        assertEquals("5", baseline["contract.major"])
        assertEquals("5", baseline["contract.minor"])
        assertEquals("1", baseline["snapshot.schema"])
        assertEquals("1", baseline["storage.schema.minimum"])
        assertEquals("7", baseline["storage.schema.current"])
        assertEquals("a".repeat(64), baseline["contract.hash"])
    }

    @Test
    public fun previousCurrentSchemaOrMinorAreRejected() {
        listOf(
            fixture.replace("contract.minor=5", "contract.minor=0"),
            fixture.replace("contract.minor=5", "contract.minor=1"),
            fixture.replace("contract.minor=5", "contract.minor=2"),
            fixture.replace("contract.minor=5", "contract.minor=3"),
            fixture.replace("contract.minor=5", "contract.minor=4"),
            fixture.replace("contract.minor=5", "contract.minor=6"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=6"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=5"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=4"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=3"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=2"),
            fixture.replace("storage.schema.current=7", "storage.schema.current=8"),
            fixture.replace("contract.major=5", "contract.major=4"),
            fixture.replace("snapshot.schema=1", "snapshot.schema=2"),
            fixture.replace("storage.schema.minimum=1", "storage.schema.minimum=2"),
        ).forEach { incompatible ->
            assertFails { FfiCompatibilityBaseline.parse(incompatible) }
        }
    }

    // Synthetic parser inputs carry no native artifact or actual contract-hash claim.
    private val fixture =
        """
        schema=harvestcircle.ffi.v5
        contract.id=harvestcircle-desktop-ffi-v5
        contract.major=5
        contract.minor=5
        contract.hash=${"a".repeat(64)}
        product.coordinate_digest=${"b".repeat(64)}
        snapshot.schema=1
        storage.schema.minimum=1
        storage.schema.current=7
        product.version=0.1.0-alpha
        package.version=1.0.0
        """.trimIndent() + "\n"
}
