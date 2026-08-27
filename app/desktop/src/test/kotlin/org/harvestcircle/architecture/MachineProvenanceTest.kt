package org.harvestcircle.architecture

import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MachineProvenanceTest {
    @Test
    fun sourceProvenanceUsesVerifiedImmutableCoordinates() {
        val root = findProvenanceRepositoryRoot()
        val provenance = root.resolve("core/provenance/harvestcircle-v1.toml").readText()

        assertTrue(provenance.contains("schema = \"harvestcircle.source_provenance.v1\""))
        assertTrue(
            provenance.contains(
                "source_repository = \"https://github.com/radrootslabs/harvestcircle\"",
            ),
        )
        assertTrue(
            provenance.contains(
                "foundation_baseline = \"c08d18ea569351dddeef70d4c1410708daf067b6\"",
            ),
        )
        assertTrue(
            provenance.contains(
                "canonical_radroots_revision = \"ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb\"",
            ),
        )
        assertEquals(8, Regex("(?m)^\\[\\[import]]$").findAll(provenance).count())
        assertEquals(8, Regex("(?m)^commit = \"[0-9a-f]{40}\"$").findAll(provenance).count())
    }
}

private fun findProvenanceRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("core/Cargo.toml")) && Files.isDirectory(it.resolve("app/desktop")) }
