package org.harvestcircle.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

object VerificationLanes {
    private val expected =
        linkedMapOf(
            "schema" to "harvestcircle.verification-lanes.v1",
            "orchestration" to "external-forge-agnostic",
            "source.command" to "make source-check",
            "source.runner" to "linux",
            "source.permissions" to "contents:read",
            "source.credentials" to "none",
            "package.command" to "make package-check",
            "package.runners" to "linux,macos,windows",
            "package.permissions" to "contents:read",
            "package.credentials" to "none",
            "signing.command" to "make signing-check",
            "signing.runner" to "macos",
            "signing.permissions" to "contents:read",
            "signing.credentials" to "signing",
            "notarization.command" to "make notarization-check",
            "notarization.runner" to "macos",
            "notarization.permissions" to "contents:read",
            "notarization.credentials" to "notarization",
        )

    fun parse(source: String): Map<String, String> {
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

    @TaskAction
    fun verify() {
        val source = policyFile.get().asFile.readText()
        check(VerificationLanes.parse(source).size == 18)
        check(runCatching { VerificationLanes.parse(source + "source.permissions=write") }.isFailure)
        check(runCatching { VerificationLanes.parse(source.replace("contents:read", "contents:write")) }.isFailure)
        check(runCatching { VerificationLanes.parse(source.replace("credentials=none", "credentials=all")) }.isFailure)
        check(runCatching { VerificationLanes.parse(source.replace("source.runner=linux", "source.runner=macos")) }.isFailure)
    }
}
