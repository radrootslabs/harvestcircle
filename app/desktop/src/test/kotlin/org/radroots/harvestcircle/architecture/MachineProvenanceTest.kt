package org.radroots.harvestcircle.architecture

import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MachineProvenanceTest {
    @Test
    fun studioImportProvenanceUsesVerifiedImmutableCoordinates() {
        val root = findProvenanceRepositoryRoot()
        val provenance = root.resolve("core/provenance/studio-import-v1.toml").readText()

        assertTrue(provenance.contains("schema = \"harvestcircle.source_provenance.v1\""))
        assertTrue(
            provenance.contains(
                "foundation_baseline = \"a2038b3e25b9e34f0b8fd001f26a8ed10b5772cb\"",
            ),
        )
        assertTrue(
            provenance.contains(
                "canonical_radroots_revision = \"09065a610d95e57acdc895a14c07580fa099e7c3\"",
            ),
        )
        assertEquals(8, Regex("(?m)^\\[\\[import]]$").findAll(provenance).count())
        assertEquals(8, Regex("(?m)^commit = \"[0-9a-f]{40}\"$").findAll(provenance).count())
        assertEquals(
            1,
            Regex("(?m)^source_repository = \"https://github.com/radrootslabs/studio_app\"$")
                .findAll(provenance)
                .count(),
        )
    }
}

private fun findProvenanceRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("core/Cargo.toml")) && Files.isDirectory(it.resolve("app/desktop")) }
