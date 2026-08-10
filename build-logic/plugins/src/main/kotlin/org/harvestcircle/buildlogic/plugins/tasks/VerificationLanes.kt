package org.harvestcircle.buildlogic.plugins.tasks

import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

object VerificationLanes {
    private fun expected(environmentPrefix: String) =
        linkedMapOf(
            "schema" to "harvestcircle.verification-lanes.v2",
            "orchestration" to "standalone-make",
            "source.command" to "make source-check",
            "source.runner" to "host",
            "source.credentials" to "none",
            "package.command" to "make package-check",
            "package.runners" to "linux,macos,windows",
            "package.credentials" to "none",
            "provenance.commit" to environmentPrefix + "BUILD_SOURCE_COMMIT",
            "provenance.dirty" to environmentPrefix + "BUILD_SOURCE_DIRTY",
            "provenance.radroots" to environmentPrefix + "BUILD_RADROOTS_REVISION",
            "provenance.epoch" to "SOURCE_DATE_EPOCH",
            "signing.command" to "make signing-check",
            "signing.runner" to "macos",
            "signing.credentials" to "signing",
            "notarization.command" to "make notarization-check",
            "notarization.runner" to "macos",
            "notarization.credentials" to "notarization",
        )

    fun parse(
        source: String,
        environmentPrefix: String,
    ): Map<String, String> {
        val expected = expected(environmentPrefix)
        val parsed = linkedMapOf<String, String>()
        source.trimEnd('\n', '\r').lineSequence().forEachIndexed { index, raw ->
            val line = raw.trim()
            require(line.isNotEmpty() && !line.startsWith('#')) {
                "Verification lane policy contains an empty or comment line at ${index + 1}"
            }
            val separator = line.indexOf('=')
            require(separator > 0 && separator < line.lastIndex && line.indexOf('=', separator + 1) == -1) {
                "Verification lane policy contains malformed syntax at ${index + 1}"
            }
            val key = line.substring(0, separator)
            val value = line.substring(separator + 1)
            require(parsed.put(key, value) == null) { "Duplicate verification lane key: $key" }
        }
        require(parsed.keys == expected.keys) { "Verification lane keys do not match the authority" }
        require(parsed == expected) { "Verification lane values do not match the authority" }
        return parsed
    }
}

abstract class VerifyVerificationLanes : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val policyFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val productManifestFile: RegularFileProperty

    @get:Internal
    abstract val repositoryRoot: DirectoryProperty

    @TaskAction
    fun verify() {
        val source = policyFile.get().asFile.readText()
        val environmentPrefix =
            ProductCoordinates.load(productManifestFile.get().asFile)["environment.prefix"]
        val policy = VerificationLanes.parse(source, environmentPrefix)
        check(policy.size == 18)
        check(runCatching { VerificationLanes.parse(source + "source.workflow=forbidden", environmentPrefix) }.isFailure)
        check(
            runCatching {
                VerificationLanes.parse(source.replace("credentials=none", "credentials=all"), environmentPrefix)
            }.isFailure,
        )
        check(
            runCatching {
                VerificationLanes.parse(source.replace("source.runner=host", "source.runner=remote"), environmentPrefix)
            }.isFailure,
        )
        val root = repositoryRoot.get().asFile.toPath()
        val makefile = root.resolve("Makefile").toFile().readText()
        listOf("source.command", "package.command", "signing.command", "notarization.command").forEach { key ->
            val command = policy.getValue(key)
            val target = command.removePrefix("make ")
            check(command == "make $target" && Regex("(?m)^${Regex.escape(target)}:").containsMatchIn(makefile)) {
                "Verification lane $key does not name a standalone Make target"
            }
        }
        check(policy.values.none { ".github/" in it || ".act/" in it }) {
            "Standalone verification policy must not reference an orchestration root"
        }
    }
}
