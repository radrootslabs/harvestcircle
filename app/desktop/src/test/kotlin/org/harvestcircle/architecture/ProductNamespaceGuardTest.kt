package org.harvestcircle.architecture

import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.name
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ProductNamespaceGuardTest {
    @Test
    fun inheritedNonProductPreferenceIdentifiersAreAbsent() {
        val root = findRepositoryRoot()
        val separator = "_"
        val forbidden =
            listOf(
                listOf("use", "radroots", "dns").joinToString(separator),
                listOf("use", "radroots", "subnets").joinToString(separator),
                listOf("vpn", "on", "demand", "enabled").joinToString(separator),
                listOf("run", "as", "exit", "node").joinToString(separator),
                listOf("automatically", "check", "for", "updates").joinToString(separator),
                listOf("update", "channel").joinToString(separator),
                listOf("last", "update", "check", "summary").joinToString(separator),
                listOf("alternate", "server", "url").joinToString(separator),
            )
        val findings =
            trackedFiles(root)
                .filter { relative ->
                    relative.endsWith(".rs") ||
                        relative.endsWith(".kt") ||
                        relative.endsWith(".kts") ||
                        relative.endsWith(".toml") ||
                        relative.endsWith(".properties")
                }.flatMap { relative ->
                    val source = root.resolve(relative).readText().lowercase()
                    forbidden.filter(source::contains).map { token -> "$relative: $token" }
                }

        assertEquals(emptyList(), findings.sorted())
    }

    @Test
    fun reusableApplicationCoreDoesNotReadTheProcessEnvironment() {
        val root = findRepositoryRoot()
        val environmentRead = listOf("std", "env").joinToString("::")
        val findings =
            trackedFiles(root)
                .filter { it.startsWith("core/crates/harvestcircle_application/src/") && it.endsWith(".rs") }
                .filter { root.resolve(it).readText().contains(environmentRead) }

        assertEquals(emptyList(), findings.sorted())
    }

    @Test
    fun productionKotlinDoesNotMintProcessLocalOperationCounters() {
        val root = findRepositoryRoot()
        val counterType = "Atomic" + "Long"
        val legacyPrefix = "desktop" + "-operation:"
        val findings =
            trackedFiles(root)
                .filter { it.startsWith("app/") && it.contains("/src/") && it.contains("/main/") && it.endsWith(".kt") }
                .filter { relative ->
                    val source = root.resolve(relative).readText()
                    source.contains(counterType) || source.contains(legacyPrefix)
                }

        assertEquals(emptyList(), findings.sorted())
    }

    @Test
    fun standaloneRepositoryDoesNotTrackMonorepoDocumentationOrWorkflowRoots() {
        val forbiddenRoots = listOf("spec/", "docs/", ".github/", ".act/")
        val findings =
            trackedFiles(findRepositoryRoot())
                .filter { relative -> forbiddenRoots.any(relative::startsWith) }

        assertEquals(emptyList(), findings.sorted())
    }

    @Test
    fun trackedSourcesUseTheHarvestCircleNamingContract() {
        val root = findRepositoryRoot()
        val contract = root.resolve("AGENTS.md").readText()
        assertTrue(contract.contains("`harvestcircle_*`"))
        assertTrue(contract.contains("`org.harvestcircle`"))
        assertTrue(contract.contains("`HarvestCircle*`"))
        assertTrue(contract.contains("`HARVESTCIRCLE_*`"))

        val legacyProduct = "stu" + "dio"
        val temporaryNamespace = listOf("org", "radroots", "harvestcircle").joinToString(".")
        val temporaryPath = temporaryNamespace.replace('.', '/')
        val repositoryUrlException = "https://github.com/radrootslabs/" + legacyProduct + "_app"
        val provenanceException = "core/provenance/" + legacyProduct + "-import-v1.toml"
        val textExtensions =
            setOf(
                "gradle",
                "json",
                "kt",
                "kts",
                "lock",
                "md",
                "plist",
                "properties",
                "rs",
                "sql",
                "toml",
                "txt",
                "xml",
                "yaml",
                "yml",
            )
        val textNames = setOf(".gitattributes", ".gitignore", "AGENTS.md", "LICENSE", "Makefile", "NOTICE", "gradlew", "gradlew.bat")
        val findings =
            trackedFiles(root).flatMap { relative ->
                buildList {
                    val normalizedRelative = relative.lowercase()
                    if (relative != provenanceException && normalizedRelative.contains(legacyProduct)) {
                        add("$relative: legacy product name in tracked path")
                    }
                    if (normalizedRelative.contains(temporaryPath)) {
                        add("$relative: temporary product namespace in tracked path")
                    }

                    val path = root.resolve(relative)
                    if (relative != provenanceException && (path.extension in textExtensions || path.name in textNames)) {
                        var inspected = path.readText().replace(repositoryUrlException, "")
                        inspected =
                            inspected
                                .replace("round_${legacyProduct}_screen", "")
                                .replace("Round${legacyProduct.replaceFirstChar { it.uppercase() }}", "")
                                .replace("${legacyProduct.replaceFirstChar { it.uppercase() }}Template", "")
                        if (relative == "NOTICE") {
                            val legacyDisplayName = legacyProduct.replaceFirstChar { it.uppercase() }
                            inspected =
                                inspected
                                    .replace("Radroots $legacyDisplayName application work", "")
                                    .replace(provenanceException, "")
                        }
                        if (inspected.lowercase().contains(legacyProduct)) {
                            add("$relative: legacy product name in tracked text")
                        }
                        if (inspected.contains(temporaryNamespace) || inspected.contains(temporaryPath)) {
                            add("$relative: temporary product namespace in tracked text")
                        }
                    }
                }
            }

        assertEquals(emptyList(), findings.sorted())
    }
}

private fun trackedFiles(root: Path): List<String> {
    if (!Files.exists(root.resolve(".git"))) {
        return Files.walk(root).use { paths ->
            paths
                .filter(Files::isRegularFile)
                .map { root.relativize(it).toString().replace('\\', '/') }
                .filter { relative ->
                    relative.split('/').none { it in setOf(".gradle", ".kotlin", "build", "target", "out") }
                }.sorted()
                .toList()
        }
    }
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
        .filter { Files.isRegularFile(root.resolve(it)) }
}

private fun findRepositoryRoot(): Path =
    generateSequence(Path.of("").toAbsolutePath()) { it.parent }
        .first { Files.isRegularFile(it.resolve("core/Cargo.toml")) && Files.isDirectory(it.resolve("app/desktop")) }
