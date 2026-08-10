package org.harvestcircle.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

object GitSourcePolicy {
    private val gitExpression = Regex("""git\s*=\s*"([^"]+)"""")
    private val revisionExpression = Regex("""rev\s*=\s*"([0-9a-f]{40})"""")
    private val forbiddenSpec = Regex("""(?:branch|tag)\s*=""")

    fun validateDependency(
        expression: String,
        allowedGit: Set<String>,
    ): String? {
        val git = gitExpression.find(expression)?.groupValues?.get(1) ?: return null
        require(git in allowedGit) { "Git dependency source is not allowlisted: $git" }
        require(!forbiddenSpec.containsMatchIn(expression)) { "Git dependency uses a branch or tag" }
        val revisions = revisionExpression.findAll(expression).map { it.groupValues[1] }.toList()
        require(revisions.size == 1) { "Git dependency must use exactly one full revision pin" }
        return revisions.single()
    }
}

abstract class VerifyGitSourcePolicy : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val denyConfigFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val cargoLockFile: RegularFileProperty

    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val cargoManifestFiles: ConfigurableFileCollection

    @TaskAction
    fun verify() {
        val denyConfig = denyConfigFile.get().asFile.readText()
        check(Regex("(?m)^required-git-spec\\s*=\\s*\"rev\"$").containsMatchIn(denyConfig)) {
            "cargo-deny must require revision-pinned Git sources"
        }
        val allowedGit =
            Regex("(?s)allow-git\\s*=\\s*\\[(.*?)]")
                .find(denyConfig)
                ?.groupValues
                ?.get(1)
                ?.let { block -> Regex("\"([^\"]+)\"").findAll(block).map { it.groupValues[1] }.toSet() }
                .orEmpty()
        check(allowedGit.isNotEmpty()) { "cargo-deny Git allowlist is empty" }

        val revisions = mutableMapOf<String, MutableSet<String>>()
        cargoManifestFiles.files.sortedBy { it.path }.forEach { manifest ->
            manifest.readLines().forEachIndexed { index, line ->
                if (!line.contains("git")) return@forEachIndexed
                val git = Regex("""git\s*=\s*"([^"]+)""").find(line)?.groupValues?.get(1) ?: return@forEachIndexed
                val revision =
                    runCatching { GitSourcePolicy.validateDependency(line, allowedGit) }
                        .getOrElse { error("${manifest.path}:${index + 1}: ${it.message}") }
                        ?: return@forEachIndexed
                revisions.getOrPut(git) { mutableSetOf() } += revision
            }
        }
        check(revisions.isNotEmpty()) { "No revision-pinned Git dependencies were inspected" }
        check(
            revisions["https://github.com/rust-nostr/nostr.git"] ==
                setOf("5bba5163eb77107f82c4a8262cf29d7f33a73219"),
        ) { "The direct rust-nostr revision changed" }

        cargoLockFile.get().asFile.useLines { lines ->
            lines.filter { it.startsWith("source = \"git+") }.forEach { source ->
                check(Regex("\\?rev=[0-9a-f]{40}#[0-9a-f]{40}\"$").containsMatchIn(source)) {
                    "Cargo.lock contains a Git source without an immutable revision: $source"
                }
            }
        }

        val allowed = allowedGit.first()
        check(
            GitSourcePolicy.validateDependency(
                "dependency = { git = \"$allowed\", rev = \"${"a".repeat(40)}\" }",
                allowedGit,
            ) == "a".repeat(40),
        )
        listOf(
            "dependency = { git = \"$allowed\", branch = \"main\" }",
            "dependency = { git = \"$allowed\", tag = \"v1.0.0\" }",
            "dependency = { git = \"$allowed\" }",
            "dependency = { git = \"https://example.invalid/repository\", rev = \"${"b".repeat(40)}\" }",
        ).forEach { fixture ->
            check(runCatching { GitSourcePolicy.validateDependency(fixture, allowedGit) }.isFailure) {
                "Git source policy accepted a mutable or unknown fixture"
            }
        }
    }
}
