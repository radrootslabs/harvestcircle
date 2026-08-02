package org.radroots.studio.architecture

import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.name
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals

class NostrOnlySourceGuardTest {
    @Test
    fun activeKotlinSourcesContainNoRetiredAccountArchitecture() {
        val sourceRoot = findSourceRoot()
        val forbidden = listOf(
            "server" + "url",
            "account" + " server",
            "editadd" + "server" + "url",
            "login" + "status",
            "java.util." + "uuid",
            "accounts" + "reducer",
            "accounts" + "store",
        )
        val findings = Files.walk(sourceRoot).use { paths ->
            paths
                .filter { it.extension == "kt" && it.name != "NostrOnlySourceGuardTest.kt" }
                .flatMap { path ->
                    val text = path.readText().lowercase()
                    forbidden.stream()
                        .filter(text::contains)
                        .map { term -> "${sourceRoot.relativize(path)}: $term" }
                }
                .sorted()
                .toList()
        }

        assertEquals(emptyList(), findings)
    }
}

private fun findSourceRoot(): Path = generateSequence(Path.of("").toAbsolutePath()) { it.parent }
    .map { it.resolve("app/desktop/src") }
    .first(Files::isDirectory)
