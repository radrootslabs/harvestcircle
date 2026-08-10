package org.radroots.harvestcircle.architecture

import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.name
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class LegacyProductIdentityGuardTest {
    @Test
    fun trackedSourcesUseTheHarvestCircleNamingContract() {
        val root = findRepositoryRoot()
        val contract = root.resolve("AGENTS.md").readText()
        assertTrue(contract.contains("`harvestcircle_*`"))
        assertTrue(contract.contains("`org.radroots.harvestcircle`"))
        assertTrue(contract.contains("`HarvestCircle*`"))
        assertTrue(contract.contains("`HARVESTCIRCLE_*`"))

        val legacyProduct = "stu" + "dio"
        val repositoryUrlException = "https://github.com/radrootslabs/" + legacyProduct + "_app"
        val provenanceException = "core/provenance/" + legacyProduct + "-import-v1.toml"
        val textExtensions = setOf("kt", "kts", "rs", "toml", "properties", "yml", "yaml", "md")
        val textNames = setOf("Makefile", ".gitignore", "gradlew", "gradlew.bat")
        val findings =
            trackedFiles(root).flatMap { relative ->
                buildList {
                    if (relative != provenanceException && relative.lowercase().contains(legacyProduct)) {
                        add("$relative: legacy product name in tracked path")
                    }

                    val path = root.resolve(relative)
                    if (relative != provenanceException && (path.extension in textExtensions || path.name in textNames)) {
                        val inspected = path.readText().replace(repositoryUrlException, "")
                        if (inspected.lowercase().contains(legacyProduct)) {
                            add("$relative: legacy product name in tracked text")
                        }
                    }
                }
            }

        assertEquals(emptyList(), findings.sorted())
    }
}

private fun trackedFiles(root: Path): List<String> {
    val process =
        ProcessBuilder("git", "-C", root.toString(), "ls-files", "-z")
            .redirectErrorStream(true)
            .start()
    val output = process.inputStream.readAllBytes()
    check(process.waitFor() == 0) {
        "Unable to enumerate tracked HarvestCircle sources: ${output.toString(StandardCharsets.UTF_8)}"
    }
    return output
        .toString(StandardCharsets.UTF_8)
        .split('\u0000')
        .filter(String::isNotEmpty)
}

private fun findRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("core/Cargo.toml")) && Files.isDirectory(it.resolve("app/desktop")) }
